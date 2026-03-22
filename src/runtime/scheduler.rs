use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use log::{info, warn};

use crate::meshing::{
    ALL_FACE_MASK, ChunkNeighborSet, MeshDirtyCause, MeshingJobResult, MeshingState,
    build_greedy_mesh, fine_chunk_boundary_mask,
};
use crate::renderer::RenderDelta;
use crate::streaming::{
    job_queue::{ChunkJobQueue, PrioritizedTask},
    job_runner::spawn_chunk_job,
    octree::StreamingOctree,
    sse::diff_active_set,
    state_store::ChunkStateStore,
    types::{ChunkJobOutcome, ChunkJobResult, ChunkKey, ChunkState, LodConfig, SseConfig},
};

use super::{
    events::{
        BlockEditCommand, BlockEditOperation, BlockPosition, ChunkCoordinate, ChunkLifecycleAction,
        ChunkLifecycleCommand, EventBus, EventBusSnapshot, PlayerAction, PlayerActionCommand,
        RuntimeCommand,
    },
    observability::RuntimeHudOverlay,
    stages::{STAGE_ORDER, Stage},
    trace::{TraceEntry, TransitionKind},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of tasks drained from the job queue per WorldUpdate frame.
const PER_FRAME_CAP: usize = 16;

/// Default job queue capacity (evicts lowest-SSE task when full).
const QUEUE_CAPACITY: usize = 128;

/// Maximum retry attempts before a chunk is transitioned to Inactive.
pub const MAX_RETRIES: u32 = 3;

// ---------------------------------------------------------------------------
// StreamingState
// ---------------------------------------------------------------------------

/// All streaming subsystem state owned by the scheduler.
pub struct StreamingState {
    pub octree: StreamingOctree,
    pub lod_configs: Vec<LodConfig>,
    pub sse_config: SseConfig,
    pub state_store: ChunkStateStore,
    pub job_queue: ChunkJobQueue,
    /// Cancel flags for in-flight jobs keyed by ChunkKey.
    pub cancel_flags: HashMap<ChunkKey, Arc<AtomicBool>>,
    pub result_sender: mpsc::Sender<ChunkJobResult>,
    pub result_receiver: mpsc::Receiver<ChunkJobResult>,
    pub rayon_pool: rayon::ThreadPool,
    pub pending_render_deltas: std::collections::VecDeque<RenderDelta>,
}

impl StreamingState {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            octree: StreamingOctree::build(4, 3),
            lod_configs: vec![
                LodConfig::new(1.0, 16.0),
                LodConfig::new(4.0, 32.0),
                LodConfig::new(16.0, 64.0),
            ],
            sse_config: SseConfig::new(720.0, std::f32::consts::FRAC_PI_3, 1.0, false),
            state_store: ChunkStateStore::new(),
            job_queue: ChunkJobQueue::new(QUEUE_CAPACITY),
            cancel_flags: HashMap::new(),
            result_sender: tx,
            result_receiver: rx,
            rayon_pool: rayon::ThreadPoolBuilder::new()
                .num_threads(4)
                .build()
                .unwrap(),
            pending_render_deltas: std::collections::VecDeque::new(),
        }
    }
}

static STREAMING: OnceLock<Mutex<StreamingState>> = OnceLock::new();
static MESHING: OnceLock<Mutex<MeshingState>> = OnceLock::new();

fn streaming_state() -> &'static Mutex<StreamingState> {
    STREAMING.get_or_init(|| Mutex::new(StreamingState::new()))
}

fn meshing_state() -> &'static Mutex<MeshingState> {
    MESHING.get_or_init(|| Mutex::new(MeshingState::default()))
}

// ---------------------------------------------------------------------------
// FrameExecution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameExecution {
    pub frame_index: u64,
    pub executed_stages: Vec<Stage>,
    pub trace_entries: Vec<TraceEntry>,
    pub overlay: RuntimeHudOverlay,
    pub event_bus: EventBusSnapshot,
}

// ---------------------------------------------------------------------------
// run_frame
// ---------------------------------------------------------------------------

pub fn run_frame(frame_index: u64) -> FrameExecution {
    let mut executed_stages = Vec::with_capacity(STAGE_ORDER.len());
    let mut trace_entries = Vec::with_capacity(STAGE_ORDER.len() * 2);
    let mut event_bus = EventBus::new(frame_index);

    for (stage_index, stage) in STAGE_ORDER.into_iter().enumerate() {
        let begin_sequence = stage_index * 2;
        let begin = TraceEntry::new(frame_index, stage, TransitionKind::Begin, begin_sequence);
        info!(target: "runtime::trace", "{}", begin.to_structured_log());
        trace_entries.push(begin);

        match stage {
            Stage::Input => seed_input_commands(&mut event_bus),
            Stage::Simulation => event_bus.process_pending_commands(),
            Stage::RenderSubmit => {
                let _ = event_bus.consume_emitted();
                if let Some(renderer) = crate::renderer::renderer_state() {
                    if let Ok(mut renderer) = renderer.lock() {
                        drain_pending_render_deltas_into_renderer(&mut renderer);
                        let _ = crate::renderer::submit_frame(&mut renderer, frame_index);
                    }
                }
            }
            Stage::WorldUpdate => run_world_update(frame_index),
            Stage::MeshSync => run_mesh_sync(frame_index),
        }

        executed_stages.push(stage);

        let end_sequence = begin_sequence + 1;
        let end = TraceEntry::new(frame_index, stage, TransitionKind::End, end_sequence);
        info!(target: "runtime::trace", "{}", end.to_structured_log());
        trace_entries.push(end);
    }

    let overlay = RuntimeHudOverlay::from_trace_entries(&trace_entries);

    FrameExecution {
        frame_index,
        executed_stages,
        trace_entries,
        overlay,
        event_bus: event_bus.snapshot(),
    }
}

// ---------------------------------------------------------------------------
// WorldUpdate arm
// ---------------------------------------------------------------------------

fn run_world_update(frame_index: u64) {
    let mut ss = streaming_state().lock().unwrap();

    // Camera at origin for default/test scenarios.
    let camera_pos = [0.0f32, 0.0, 0.0];

    if frame_index < 10 || (frame_index % 60 == 0) {
        eprintln!(
            "[DIAG-WU] frame={} octree_nodes={} active_set_size={}",
            frame_index,
            ss.octree.nodes().len(),
            ss.state_store.active_set().len(),
        );
    }

    // Compute diff against current active set.
    let current_active = ss.state_store.active_set();
    let diff = diff_active_set(
        &ss.octree,
        &ss.lod_configs,
        &ss.sse_config,
        &current_active,
        |key: &ChunkKey| {
            let dx = key.x as f32 - camera_pos[0];
            let dy = key.y as f32 - camera_pos[1];
            let dz = key.z as f32 - camera_pos[2];
            (dx * dx + dy * dy + dz * dz).sqrt().max(0.01)
        },
    );

    if frame_index < 10 || (frame_index % 60 == 0) {
        eprintln!(
            "[DIAG-WU] frame={} to_activate={} to_deactivate={}",
            frame_index, diff.to_activate.len(), diff.to_deactivate.len(),
        );
    }

    // Deactivate chunks no longer needed.
    for key in &diff.to_deactivate {
        deactivate_chunk(&mut ss, *key);
    }

    // Activate new chunks: insert into state store, then enqueue.
    for key in &diff.to_activate {
        if ss.state_store.get(key).is_none() {
            ss.state_store.insert_inactive(*key);
        }
        let state = ss.state_store.get(key).map(|e| e.state);
        if state == Some(ChunkState::Inactive) {
            // Inactive -> Queued
            if let Err(e) = ss.state_store.transition_to(*key, ChunkState::Queued) {
                warn!("chunk {:?} transition to Queued failed: {e}", key);
            }
            let sse_bits = 1.0f32.to_bits(); // placeholder SSE; refined on drain
            ss.job_queue.enqueue(PrioritizedTask {
                key: *key,
                lod_level: key.lod_level,
                sse_bits,
            });
        }
    }

    // Drain up to PER_FRAME_CAP and spawn jobs.
    let tasks = ss.job_queue.drain_up_to(PER_FRAME_CAP);
    let sender = ss.result_sender.clone();

    // Transition all drained tasks to Loading before borrowing the pool.
    for task in &tasks {
        let key = task.key;
        let entry_state = ss.state_store.get(&key).map(|e| e.state);
        if entry_state == Some(ChunkState::Queued) {
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Loading) {
                warn!("chunk {key:?} transition to Loading failed: {e}");
            }
        }
    }

    // Now borrow pool and spawn.
    let pool = &ss.rayon_pool;
    // Collect (key, flag) pairs before modifying cancel_flags map.
    let mut spawned: Vec<(ChunkKey, Arc<AtomicBool>)> = Vec::with_capacity(tasks.len());
    for task in tasks {
        let key = task.key;
        let flag = spawn_chunk_job(pool, task, sender.clone());
        spawned.push((key, flag));
    }
    for (key, flag) in spawned {
        ss.cancel_flags.insert(key, flag);
    }
}

// ---------------------------------------------------------------------------
// MeshSync arm
// ---------------------------------------------------------------------------

fn run_mesh_sync(frame_index: u64) {
    let mut ss = streaming_state().lock().unwrap();
    let mut recv_count = 0u32;

    loop {
        match ss.result_receiver.try_recv() {
            Ok(result) => {
                recv_count += 1;
                handle_job_result(&mut ss, result, frame_index);
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }

    let dirty_batch = {
        let mut meshing = meshing_state().lock().unwrap();
        let batch = meshing.take_dirty_batch(PER_FRAME_CAP);
        if frame_index < 10 || (frame_index % 60 == 0) {
            eprintln!(
                "[DIAG-MS] frame={} results_received={} dirty_batch_size={} queued_remaining={} pending_render_deltas={}",
                frame_index, recv_count, batch.len(),
                meshing.queued.len(),
                ss.pending_render_deltas.len(),
            );
        }
        batch
    };

    for key in dirty_batch {
        let maybe_mesh = {
            let meshing = meshing_state().lock().unwrap();
            match meshing.dirty.get(&key).cloned() {
                Some(dirty_record) => match meshing.payloads.get(&key) {
                    Some(chunk) => {
                        let neighbors = ChunkNeighborSet {
                            px: meshing
                                .payloads
                                .get(&ChunkKey::new(key.x + 1, key.y, key.z, key.lod_level)),
                            nx: meshing
                                .payloads
                                .get(&ChunkKey::new(key.x - 1, key.y, key.z, key.lod_level)),
                            py: meshing
                                .payloads
                                .get(&ChunkKey::new(key.x, key.y + 1, key.z, key.lod_level)),
                            ny: meshing
                                .payloads
                                .get(&ChunkKey::new(key.x, key.y - 1, key.z, key.lod_level)),
                            pz: meshing
                                .payloads
                                .get(&ChunkKey::new(key.x, key.y, key.z + 1, key.lod_level)),
                            nz: meshing
                                .payloads
                                .get(&ChunkKey::new(key.x, key.y, key.z - 1, key.lod_level)),
                            finer_neighbor_face_mask: dirty_record.finer_neighbor_face_mask,
                        };
                        Some((build_greedy_mesh(chunk, &neighbors, &dirty_record), dirty_record))
                    }
                    None => None,
                },
                None => None,
            }
        };

        let Some((mesh, dirty_record)) = maybe_mesh else {
            continue;
        };

        let source_revision = ss
            .state_store
            .get(&key)
            .map_or(dirty_record.source_revision, |entry| entry.revision);
        let mut meshing = meshing_state().lock().unwrap();
        meshing.completed_meshes.retain(|completed| completed.key != key);
        meshing.completed_meshes.push(MeshingJobResult {
            key,
            mesh: mesh.clone(),
            source_revision,
        });
        meshing.dirty.remove(&key);
        ss.pending_render_deltas
            .push_back(RenderDelta::Upsert { key, mesh });
    }
}

fn handle_job_result(ss: &mut StreamingState, result: ChunkJobResult, frame_index: u64) {
    let key = result.key;
    // Remove cancel flag for this key.
    ss.cancel_flags.remove(&key);

    match result.outcome {
        ChunkJobOutcome::Generated(voxels) => {
            // Loading -> Active
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Active) {
                warn!("chunk {key:?} transition to Active (Generated) failed: {e}");
            }
            let source_revision = ss.state_store.get(&key).map_or(0, |entry| entry.revision);
            let mut meshing = meshing_state().lock().unwrap();
            meshing.payloads.insert(key, voxels);
            meshing.mark_dirty(key, MeshDirtyCause::GeneratedPayload, source_revision);
            meshing.mark_face_neighbors_dirty(key, ALL_FACE_MASK, source_revision);
            if key.lod_level == 0 {
                meshing.mark_coarse_lod_neighbors_dirty(
                    key,
                    fine_chunk_boundary_mask(key),
                    true,
                    source_revision,
                );
            }
        }
        ChunkJobOutcome::Loaded => {
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Active) {
                warn!("chunk {key:?} transition to Active (Loaded) failed: {e}");
            }
        }
        ChunkJobOutcome::Cancelled => {
            // Intentional cancel: transition Loading -> Inactive (or leave as-is).
            // Guard: only transition if currently in Loading state.
            let state = ss.state_store.get(&key).map(|e| e.state);
            if state == Some(ChunkState::Loading) {
                // Loading is not directly -> Inactive; go through Queued->Inactive path.
                // Use Error path: Loading -> Error, then Error -> Inactive.
                if let Err(e) = ss.state_store.transition_to(
                    key,
                    ChunkState::Error {
                        retry_count: MAX_RETRIES,
                        next_retry_frame: frame_index,
                    },
                ) {
                    warn!("chunk {key:?} transition to Error (Cancelled) failed: {e}");
                }
                if let Err(e) = ss.state_store.transition_to(key, ChunkState::Inactive) {
                    warn!("chunk {key:?} transition to Inactive (Cancelled) failed: {e}");
                }
            }
        }
        ChunkJobOutcome::Unloaded => {
            // Loading/Unloading -> Inactive
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Inactive) {
                warn!("chunk {key:?} transition to Inactive (Unloaded) failed: {e}");
            }
            let source_revision = ss.state_store.get(&key).map_or(0, |entry| entry.revision);
            let mut meshing = meshing_state().lock().unwrap();
            meshing.payloads.remove(&key);
            meshing.dirty.remove(&key);
            meshing.queued.retain(|queued| *queued != key);
            meshing.completed_meshes.retain(|mesh| mesh.key != key);
            if key.lod_level == 0 {
                meshing.mark_coarse_lod_neighbors_dirty(
                    key,
                    fine_chunk_boundary_mask(key),
                    false,
                    source_revision,
                );
            }
            ss.pending_render_deltas.push_back(RenderDelta::Remove { key });
        }
        ChunkJobOutcome::Failed(_msg) => {
            let current_retry = match ss.state_store.get(&key).map(|e| e.state) {
                Some(ChunkState::Error { retry_count, .. }) => retry_count,
                _ => 0,
            };
            let next_retry_frame = frame_index + 2u64.pow(current_retry);
            if current_retry >= MAX_RETRIES {
                if let Err(e) = ss.state_store.transition_to(
                    key,
                    ChunkState::Error {
                        retry_count: current_retry + 1,
                        next_retry_frame,
                    },
                ) {
                    warn!("chunk {key:?} transition to Error (Failed, max retries) failed: {e}");
                }
                if let Err(e) = ss.state_store.transition_to(key, ChunkState::Inactive) {
                    warn!("chunk {key:?} transition to Inactive (Failed, max retries) failed: {e}");
                }
            } else if let Err(e) = ss.state_store.transition_to(
                    key,
                    ChunkState::Error {
                        retry_count: current_retry + 1,
                        next_retry_frame,
                    },
                ) {
                    warn!("chunk {key:?} transition to Error (Failed) failed: {e}");
                }
        }
    }
}

fn deactivate_chunk(ss: &mut StreamingState, key: ChunkKey) {
    if let Some(flag) = ss.cancel_flags.get(&key) {
        flag.store(true, Ordering::Relaxed);
    }
    ss.job_queue.cancel_queued(key);

    let state = ss.state_store.get(&key).map(|entry| entry.state);
    if matches!(
        state,
        Some(ChunkState::Active | ChunkState::Upgrading | ChunkState::Downgrading)
    ) {
        if let Err(e) = ss.state_store.transition_to(key, ChunkState::Unloading) {
            warn!("chunk {key:?} transition to Unloading (deactivate) failed: {e}");
        }
        if let Err(e) = ss
            .result_sender
            .send(ChunkJobResult::new(key, ChunkJobOutcome::Unloaded))
        {
            warn!("chunk {key:?} failed to send Unloaded result: {e}");
        }
    }
}

fn drain_pending_render_deltas_into_renderer(renderer: &mut crate::renderer::Renderer) {
    let mut ss = streaming_state().lock().unwrap();
    while let Some(delta) = ss.pending_render_deltas.pop_front() {
        renderer.enqueue_chunk_delta(delta);
    }
}

pub fn debug_deactivate_active_chunk_for_tests(key: ChunkKey, frame_index: u64) -> Vec<RenderDelta> {
    {
        let mut ss = streaming_state().lock().unwrap();
        *ss = StreamingState::new();
        ss.state_store.insert_inactive(key);
        ss.state_store.transition_to(key, ChunkState::Queued).unwrap();
        ss.state_store.transition_to(key, ChunkState::Loading).unwrap();
        ss.state_store.transition_to(key, ChunkState::Active).unwrap();
        deactivate_chunk(&mut ss, key);
    }

    {
        let mut meshing = meshing_state().lock().unwrap();
        *meshing = MeshingState::default();
    }

    run_mesh_sync(frame_index);

    let mut ss = streaming_state().lock().unwrap();
    ss.pending_render_deltas.drain(..).collect()
}

// ---------------------------------------------------------------------------
// Input seeding
// ---------------------------------------------------------------------------

fn seed_input_commands(event_bus: &mut EventBus) {
    let _ = event_bus.publish_command(RuntimeCommand::PlayerAction(PlayerActionCommand {
        actor_entity_id: 1,
        action: PlayerAction::Jump,
    }));

    let _ = event_bus.publish_command(RuntimeCommand::ChunkLifecycle(ChunkLifecycleCommand {
        chunk: ChunkCoordinate { x: 0, y: 0, z: 0 },
        action: ChunkLifecycleAction::Activate,
        lod_level: 0,
    }));

    let _ = event_bus.publish_command(RuntimeCommand::BlockEdit(BlockEditCommand {
        actor_entity_id: 1,
        position: BlockPosition { x: 0, y: 64, z: 0 },
        edit: BlockEditOperation::Place {
            block_id: "stone".to_string(),
        },
    }));
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::MAX_RETRIES;
    use crate::streaming::{
        state_store::ChunkStateStore,
        types::{ChunkKey, ChunkState},
    };

    fn key(n: i32) -> ChunkKey {
        ChunkKey::new(n, 0, 0, 0)
    }

    // Helper: apply the MeshSync Failed-outcome logic directly without run_frame.
    fn apply_failed(store: &mut ChunkStateStore, key: ChunkKey, frame_index: u64) {
        let current_retry = match store.get(&key).map(|e| e.state) {
            Some(ChunkState::Error { retry_count, .. }) => retry_count,
            _ => 0,
        };
        let next_retry_frame = frame_index + 2u64.pow(current_retry);
        if current_retry >= MAX_RETRIES {
            let _ = store.transition_to(
                key,
                ChunkState::Error {
                    retry_count: current_retry + 1,
                    next_retry_frame,
                },
            );
            let _ = store.transition_to(key, ChunkState::Inactive);
        } else {
            let _ = store.transition_to(
                key,
                ChunkState::Error {
                    retry_count: current_retry + 1,
                    next_retry_frame,
                },
            );
        }
    }

    // -----------------------------------------------------------------------
    // world_update_no_panic
    // -----------------------------------------------------------------------
    #[test]
    fn world_update_no_panic() {
        // run_frame touches OnceLock global state; just assert it completes.
        let result = super::run_frame(1000);
        assert_eq!(result.executed_stages.len(), 5);
    }

    // -----------------------------------------------------------------------
    // mesh_sync_no_panic
    // -----------------------------------------------------------------------
    #[test]
    fn mesh_sync_no_panic() {
        let result = super::run_frame(1001);
        assert_eq!(result.executed_stages.len(), 5);
    }

    // -----------------------------------------------------------------------
    // mesh_sync_failed_outcome_increments_retry
    // -----------------------------------------------------------------------
    #[test]
    fn mesh_sync_failed_outcome_increments_retry() {
        let k = key(9999);
        let frame: u64 = 42;

        // Build an isolated state store and apply failed logic.
        let mut store = ChunkStateStore::new();
        store.insert_inactive(k);
        store.transition_to(k, ChunkState::Queued).unwrap();
        store.transition_to(k, ChunkState::Loading).unwrap();

        // Simulate a Failed outcome from MeshSync.
        apply_failed(&mut store, k, frame);

        let entry = store.get(&k).expect("entry must exist");
        match entry.state {
            ChunkState::Error {
                retry_count,
                next_retry_frame,
            } => {
                assert_eq!(
                    retry_count, 1,
                    "retry_count should be 1 after first failure"
                );
                // next_retry_frame = frame + 2^0 = frame + 1
                assert_eq!(
                    next_retry_frame,
                    frame + 1,
                    "next_retry_frame should be frame + 2^0 = frame + 1"
                );
            }
            other => panic!("expected Error state, got {:?}", other),
        }
    }
}
