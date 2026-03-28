use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use log::{info, warn};

use crate::meshing::{
    ALL_FACE_MASK, ChunkNeighborSet, MeshDirtyCause, MeshingJobResult, MeshingState,
    build_greedy_mesh, build_meshlets_from_packed, fine_chunk_boundary_mask,
};
use crate::renderer::RenderDelta;
use crate::streaming::{
    job_queue::{ChunkJobQueue, PrioritizedTask},
    job_runner::spawn_chunk_job,
    octree::StreamingOctree,
    sse::{compute_sse, diff_active_set},
    state_store::ChunkStateStore,
    types::{ChunkJobOutcome, ChunkJobResult, ChunkKey, ChunkState, LodConfig, SseConfig, CHUNK_EDGE},
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

/// Block size in metres (1/16 m = 6.25 cm).
const BLOCK_SIZE: f32 = 1.0 / 16.0;

/// Maximum number of tasks drained from the job queue per WorldUpdate frame.
const PER_FRAME_CAP: usize = 16;

/// Default job queue capacity (evicts lowest-SSE task when full).
const QUEUE_CAPACITY: usize = 128;

/// Maximum retry attempts before a chunk is transitioned to Inactive.
pub const MAX_RETRIES: u32 = 3;

// ---------------------------------------------------------------------------
// StreamingState
// ---------------------------------------------------------------------------

/// All streaming subsystem state owned by the App struct.
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

impl Default for StreamingState {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingState {
    pub fn new() -> Self {
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

pub fn run_frame(
    streaming: &mut StreamingState,
    meshing: &mut MeshingState,
    _renderer: Option<&mut crate::renderer::Renderer>,
    frame_index: u64,
    camera_pos: [f32; 3],
    screen_height: f32,
    fov_y: f32,
) -> FrameExecution {
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
                // Renderer interaction only when a renderer is available.
                // Tests pass None; the real app passes Some(&mut renderer).
            }
            Stage::WorldUpdate => {
                // Update SseConfig with actual viewport parameters each frame (D-04).
                streaming.sse_config.screen_height = screen_height;
                streaming.sse_config.fov_y_radians = fov_y;
                run_world_update(streaming, frame_index, camera_pos);
            }
            Stage::MeshSync => run_mesh_sync(streaming, meshing, frame_index),
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

fn run_world_update(ss: &mut StreamingState, frame_index: u64, camera_pos: [f32; 3]) {
    if frame_index < 10 || frame_index.is_multiple_of(60) {
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
            // World-space conversion: CHUNK_EDGE * BLOCK_SIZE * lod_scale (CRIT-05).
            let lod_scale = (1_u32 << key.lod_level) as f32;
            let chunk_edge_world = CHUNK_EDGE as f32 * BLOCK_SIZE * lod_scale;
            let half_edge = chunk_edge_world * 0.5;
            let wx = key.x as f32 * chunk_edge_world + half_edge;
            let wy = key.y as f32 * chunk_edge_world + half_edge;
            let wz = key.z as f32 * chunk_edge_world + half_edge;
            let dx = wx - camera_pos[0];
            let dy = wy - camera_pos[1];
            let dz = wz - camera_pos[2];
            (dx * dx + dy * dy + dz * dz).sqrt().max(0.01)
        },
    );

    if frame_index < 10 || frame_index.is_multiple_of(60) {
        eprintln!(
            "[DIAG-WU] frame={} to_activate={} to_deactivate={}",
            frame_index, diff.to_activate.len(), diff.to_deactivate.len(),
        );
    }

    // Deactivate chunks no longer needed.
    for key in &diff.to_deactivate {
        deactivate_chunk(ss, *key);
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
            // Compute real SSE at enqueue time (MED-06, CRIT-05).
            let lod_scale = (1_u32 << key.lod_level) as f32;
            let chunk_edge_world = CHUNK_EDGE as f32 * BLOCK_SIZE * lod_scale;
            let half_edge = chunk_edge_world * 0.5;
            let wx = key.x as f32 * chunk_edge_world + half_edge;
            let wy = key.y as f32 * chunk_edge_world + half_edge;
            let wz = key.z as f32 * chunk_edge_world + half_edge;
            let dx = wx - camera_pos[0];
            let dy = wy - camera_pos[1];
            let dz = wz - camera_pos[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt().max(0.01);
            let real_sse = ss.lod_configs
                .get(key.lod_level as usize)
                .map(|lod| compute_sse(lod, &ss.sse_config, dist))
                .unwrap_or(1.0);
            ss.job_queue.enqueue(PrioritizedTask::new(*key, key.lod_level, real_sse));
        }
    }

    // Drain up to PER_FRAME_CAP and spawn jobs.
    let tasks = ss.job_queue.drain_up_to(PER_FRAME_CAP);
    let sender = ss.result_sender.clone();

    // Transition all drained tasks to Loading before borrowing the pool.
    for task in &tasks {
        let key = task.key;
        let entry_state = ss.state_store.get(&key).map(|e| e.state);
        if entry_state == Some(ChunkState::Queued)
            && let Err(e) = ss.state_store.transition_to(key, ChunkState::Loading) {
                warn!("chunk {key:?} transition to Loading failed: {e}");
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

/// Maximum number of job results processed per MeshSync frame (MED-08).
const MAX_RESULTS_PER_FRAME: u32 = 16;

fn run_mesh_sync(ss: &mut StreamingState, meshing: &mut MeshingState, frame_index: u64) {
    let mut recv_count = 0u32;
    let max_results = MAX_RESULTS_PER_FRAME;

    loop {
        if recv_count >= max_results {
            break;
        }
        match ss.result_receiver.try_recv() {
            Ok(result) => {
                recv_count += 1;
                handle_job_result(ss, meshing, result, frame_index);
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }

    let dirty_batch = {
        let batch = meshing.take_dirty_batch(PER_FRAME_CAP);
        if frame_index < 10 || frame_index.is_multiple_of(60) {
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
                        let packed = build_greedy_mesh(chunk, &neighbors, &dirty_record);
                        let meshlet_mesh = build_meshlets_from_packed(&packed);
                        Some((packed, meshlet_mesh, dirty_record))
                    }
                    // HIGH-05: Payload absent for dirty key — remove from dirty map.
                    None => {
                        meshing.dirty.remove(&key);
                        None
                    }
                },
                None => None,
            }
        };

        let Some((packed_mesh, meshlet_mesh, dirty_record)) = maybe_mesh else {
            continue;
        };

        let source_revision = ss
            .state_store
            .get(&key)
            .map_or(dirty_record.source_revision, |entry| entry.revision);
        meshing.completed_meshes.retain(|completed| completed.key != key);
        meshing.completed_meshes.push(MeshingJobResult {
            key,
            mesh: packed_mesh,
            source_revision,
        });
        meshing.dirty.remove(&key);
        ss.pending_render_deltas
            .push_back(RenderDelta::Upsert { key, mesh: meshlet_mesh });
    }
}

fn handle_job_result(
    ss: &mut StreamingState,
    meshing: &mut MeshingState,
    result: ChunkJobResult,
    frame_index: u64,
) {
    let key = result.key;
    // Check if this job was cancelled while in-flight (CRIT-06).
    let was_cancelled = ss
        .cancel_flags
        .get(&key)
        .map(|f| f.load(Ordering::Acquire))
        .unwrap_or(false);
    // Remove cancel flag for this key (HIGH-04).
    ss.cancel_flags.remove(&key);

    match result.outcome {
        ChunkJobOutcome::Generated(voxels) => {
            // If cancel flag was set during Loading, go to Inactive instead of Active (CRIT-06).
            if was_cancelled {
                let state = ss.state_store.get(&key).map(|e| e.state);
                if state == Some(ChunkState::Loading) {
                    if let Err(e) = ss.state_store.transition_to(
                        key,
                        ChunkState::Error {
                            retry_count: MAX_RETRIES,
                            next_retry_frame: frame_index,
                        },
                    ) {
                        warn!("chunk {key:?} transition to Error (cancelled Generated) failed: {e}");
                    }
                    if let Err(e) = ss.state_store.transition_to(key, ChunkState::Inactive) {
                        warn!("chunk {key:?} transition to Inactive (cancelled Generated) failed: {e}");
                    }
                    ss.state_store.remove(&key);
                }
                return;
            }
            // Loading -> Active
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Active) {
                warn!("chunk {key:?} transition to Active (Generated) failed: {e}");
            }
            let source_revision = ss.state_store.get(&key).map_or(0, |entry| entry.revision);
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
            if was_cancelled {
                let state = ss.state_store.get(&key).map(|e| e.state);
                if state == Some(ChunkState::Loading) {
                    if let Err(e) = ss.state_store.transition_to(
                        key,
                        ChunkState::Error {
                            retry_count: MAX_RETRIES,
                            next_retry_frame: frame_index,
                        },
                    ) {
                        warn!("chunk {key:?} transition to Error (cancelled Loaded) failed: {e}");
                    }
                    if let Err(e) = ss.state_store.transition_to(key, ChunkState::Inactive) {
                        warn!("chunk {key:?} transition to Inactive (cancelled Loaded) failed: {e}");
                    }
                    ss.state_store.remove(&key);
                }
                return;
            }
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Active) {
                warn!("chunk {key:?} transition to Active (Loaded) failed: {e}");
            }
        }
        ChunkJobOutcome::Cancelled => {
            // Intentional cancel: transition Loading -> Inactive.
            let state = ss.state_store.get(&key).map(|e| e.state);
            if state == Some(ChunkState::Loading) {
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
                ss.state_store.remove(&key);
            }
        }
        ChunkJobOutcome::Unloaded => {
            // Unloading -> Inactive
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Inactive) {
                warn!("chunk {key:?} transition to Inactive (Unloaded) failed: {e}");
            }
            let source_revision = ss.state_store.get(&key).map_or(0, |entry| entry.revision);
            meshing.payloads.remove(&key);
            meshing.dirty.remove(&key);
            meshing.queued.retain(|queued| *queued != key);
            meshing.queued_set.remove(&key);
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
            ss.state_store.remove(&key); // HIGH-03: no unbounded growth
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
                ss.state_store.remove(&key); // HIGH-03: cleanup after max retries
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
    let state = ss.state_store.get(&key).map(|entry| entry.state);

    match state {
        // CRIT-06: Queued → Inactive directly.
        Some(ChunkState::Queued) => {
            ss.job_queue.cancel_queued(key);
            ss.cancel_flags.remove(&key); // HIGH-04: cleanup cancel flag
            if let Err(e) = ss.state_store.transition_to(key, ChunkState::Inactive) {
                warn!("chunk {key:?} transition Queued→Inactive (deactivate) failed: {e}");
            }
            ss.state_store.remove(&key); // HIGH-03: no unbounded growth
        }
        // CRIT-06: Loading → set cancel flag for pending deactivation.
        Some(ChunkState::Loading) => {
            if let Some(flag) = ss.cancel_flags.get(&key) {
                flag.store(true, Ordering::Release);
            }
            // handle_job_result will check cancel flag and transition to Inactive.
        }
        // Active/Upgrading/Downgrading → Unloading → Inactive (via Unloaded result).
        Some(ChunkState::Active | ChunkState::Upgrading | ChunkState::Downgrading) => {
            if let Some(flag) = ss.cancel_flags.get(&key) {
                flag.store(true, Ordering::Release);
            }
            ss.job_queue.cancel_queued(key);
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
        _ => {
            // Other states (Inactive, Error, Unloading): no-op or already handled.
        }
    }
}

/// Drain pending render deltas from streaming into the renderer.
/// Called from app.rs event loop with owned references.
pub fn drain_pending_render_deltas_into_renderer(
    streaming: &mut StreamingState,
    renderer: &mut crate::renderer::Renderer,
) {
    while let Some(delta) = streaming.pending_render_deltas.pop_front() {
        renderer.enqueue_chunk_delta(delta);
    }
}

pub fn debug_deactivate_active_chunk_for_tests(key: ChunkKey, frame_index: u64) -> Vec<RenderDelta> {
    let mut ss = StreamingState::new();
    let mut meshing = MeshingState::default();

    ss.state_store.insert_inactive(key);
    ss.state_store.transition_to(key, ChunkState::Queued).unwrap();
    ss.state_store.transition_to(key, ChunkState::Loading).unwrap();
    ss.state_store.transition_to(key, ChunkState::Active).unwrap();
    deactivate_chunk(&mut ss, key);

    run_mesh_sync(&mut ss, &mut meshing, frame_index);

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
