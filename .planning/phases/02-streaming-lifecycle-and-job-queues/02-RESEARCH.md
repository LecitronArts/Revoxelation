# Phase 2: Streaming Lifecycle and Job Queues - Research

**Researched:** 2026-03-21
**Domain:** Rust voxel chunk streaming — SSE-driven active set, 7-state lifecycle, bounded rayon job queue
**Confidence:** HIGH

---

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- Block size: 1/16m (6.25cm); chunk size: 64^3 blocks = 4m per side
- LOD: 3 levels — LOD0=4m, LOD1=32m, LOD2=256m (configurable)
- Active-set driver: SSE, not fixed radius. Formula: `sse = (geo_err * screen_h) / (2 * dist * tan(fov/2))`
- SSE threshold: 2px (configurable constant)
- Octree per-node data: (x, y, z, lod_level: u8)
- Frustum culling: configurable; culled chunks treated as SSE=0
- Active set recomputed every frame
- LOD switching: immediate replacement, no coexistence transition
- Chunk lifecycle: 7 states + Error — Inactive, Queued, Loading, Active, Upgrading, Downgrading, Unloading, Error
- Revision ID increments only on entry to Active or Inactive
- Error retry: exponential backoff; max retries exceeded transitions to Inactive
- Queue depth: configurable, default 128
- Queue-full policy: evict lowest-SSE existing task
- Executor: rayon thread pool (already in Cargo.toml)
- Cancellation: remove from queue; or set AtomicBool if already executing
- Per-frame submit cap: configurable, default 16/frame
- Extend ChunkLifecycleCommand with lod_level: u8
- WorldUpdate stage: SSE traversal + active-set diff + task submission
- MeshSync stage: drain completed results + advance chunk state

### Claude's Discretion

- Chunk state data structure details (HashMap key type, lock granularity)
- Octree memory layout (pointer tree vs flat array)
- Result delivery channel type from background threads
- Test module layout (unit vs integration split)

### Deferred Ideas (OUT OF SCOPE)

- LOD blend transitions — Phase 3
- Greedy mesh generation — Phase 3
- LOD4/LOD5 distant layers — later phases
- Multiplayer chunk state broadcast — Phase 7

</user_constraints>

---

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| STRM-01 | Player movement drives chunk activation/deactivation | SSE traversal in WorldUpdate produces desired active set; diff against previous set dispatches ChunkLifecycleCommand with lod_level |
| STRM-02 | Chunk transitions represented by explicit lifecycle states and revision IDs | 7-state ChunkState enum + u64 revision in ChunkEntry stored in DashMap; revision increments only on Active/Inactive entry |
| STRM-03 | Heavy generation work through bounded background queues with cancellation/backpressure | Mutex<BinaryHeap<PrioritizedTask>> with SSE-based eviction; rayon pool spawn; Arc<AtomicBool> cancel per task; mpsc drained non-blocking in MeshSync |

</phase_requirements>

---

## Summary

Phase 2 introduces two major subsystems: the SSE-driven active-set manager and the bounded background job queue. Both operate inside the existing stage pipeline. `WorldUpdate` drives SSE octree traversal and submits work; `MeshSync` drains completed results and advances chunk state. No new stage or external crate is required.

The entire stack is already in `Cargo.toml`: `rayon 1.10`, `dashmap 6.1`, `glam 0.30`, `noise 0.9`. Stdlib provides `AtomicU64`, `AtomicBool`, `Mutex`, `BinaryHeap`, and `mpsc::channel`.

The chunk state machine is the single source of truth. All scheduling, cancellation, and revision logic must derive from valid state transitions only. Every state change must flow through one `transition_to` function that enforces valid edges and gates revision increments. Bypassing this from multiple code paths is the most common rewrite-forcing bug in this domain.

**Primary recommendation:** Implement `ChunkStateStore` (DashMap-backed) and `ChunkJobQueue` (Mutex<BinaryHeap>-backed) as independent testable structs under `src/world/`. Wire them into the `WorldUpdate` and `MeshSync` arms of `scheduler::run_frame`. Keep rayon task closures self-contained: capture only owned `ChunkKey` and `Arc<AtomicBool>`, generate data, send `ChunkJobResult` through a cloned `mpsc::Sender`.

---

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|---------------|
| rayon | 1.10 | Background thread pool | In Cargo.toml; `ThreadPoolBuilder::new().build()` for dedicated pool; `pool.spawn(FnOnce)` fire-and-forget |
| dashmap | 6.1 | Concurrent ChunkKey-to-ChunkEntry registry | In Cargo.toml; shard-locked; `entry().and_modify()` gives atomic read-modify-write |
| glam | 0.30 | Vec3 position, frustum math, SSE formula inputs | In Cargo.toml; `Vec3::distance()` for dist; frustum from view-projection Mat4 |
| noise | 0.9 | Voxel density generation in rayon tasks | In Cargo.toml; `Fbm::<Perlin>::new(seed)` standard pattern |
| std::sync::atomic | stdlib | AtomicU64 revision; AtomicBool cancel flags | `fetch_add(1, Relaxed)` for revision; `store(true, Release)` / `load(Acquire)` for cancel |
| std::sync::Mutex | stdlib | Guards BinaryHeap priority queue | Held only during enqueue/eviction, never during task execution |
| std::sync::mpsc | stdlib | Result channel: rayon workers send, MeshSync try_recv drains | `channel()` -> Sender (Clone+Send) + Receiver; non-blocking drain |
| std::collections::BinaryHeap | stdlib | Max-heap for SSE-ordered task queue | Wrapped in Mutex; tasks implement Ord via `f32::to_bits()` for total order |
| std::collections::HashSet | stdlib | Active-set snapshots for per-frame diff | Symmetric difference yields activate + deactivate lists |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| std::sync::mpsc | crossbeam-channel | crossbeam has bounded variant and is faster; not needed — result volume is low |
| Mutex<BinaryHeap> | crossbeam-deque | Work-stealing overkill for single-priority-queue driven from one thread |
| DashMap | RwLock<HashMap> | DashMap shards reduce contention with concurrent WorldUpdate reads + MeshSync writes |
| Pointer-based octree | Flat array octree | Flat (child = parent*8 + offset) is cache-friendlier; use flat layout |
| ordered-float crate | f32::to_bits() | Bit-cast achieves total order for non-NaN SSE; avoids new dep |

**No new dependencies needed.** All required crates already in Cargo.toml.

---

## Architecture Patterns

### Recommended Project Structure

```
src/
├── world/
│   ├── mod.rs          # ChunkKey, LodConfig, CameraParams, WorldConfig, WorldState
│   ├── chunk_state.rs  # ChunkState enum, ChunkEntry, can_transition_to()
│   ├── chunk_store.rs  # ChunkStateStore: DashMap<ChunkKey, ChunkEntry>
│   ├── job_queue.rs    # ChunkJobQueue: Mutex<BinaryHeap<PrioritizedTask>>
│   ├── job_runner.rs   # spawn_chunk_task(), ChunkJobResult, ChunkJobOutcome
│   ├── octree.rs       # LodOctree: flat Vec<OctreeNode>, traverse()
│   └── sse.rs          # compute_sse(), frustum_planes(), active_set_diff()
├── runtime/
│   ├── scheduler.rs    # EXTEND: WorldUpdate + MeshSync arms
│   ├── events/
│   │   └── command.rs  # EXTEND: lod_level: u8 on ChunkLifecycleCommand
│   └── boundaries/
│       ├── world.rs    # EXTEND: register SseTraversalSystem
│       └── meshing.rs  # EXTEND: register MeshSyncSystem
```

### Pattern 1: 7-State Chunk Lifecycle with Revision Gate

**What:** Rust enum covers all states. `can_transition_to` enforces the directed graph of valid edges. Revision increments only on `Active` and `Inactive` entry.

**When to use:** Every code path that changes chunk state — no exceptions.

```rust
// src/world/chunk_state.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkState {
    Inactive,
    Queued,
    Loading,
    Active,
    Upgrading,
    Downgrading,
    Unloading,
    Error { retry_count: u32, next_retry_frame: u64 },
}

impl ChunkState {
    pub fn can_transition_to(&self, next: &ChunkState) -> bool {
        use ChunkState::*;
        matches!(
            (self, next),
            (Inactive, Queued)
            | (Queued, Loading)
            | (Queued, Inactive)         // cancelled before pickup
            | (Loading, Active)
            | (Loading, Error { .. })
            | (Active, Upgrading)
            | (Active, Downgrading)
            | (Active, Unloading)
            | (Upgrading, Active)
            | (Upgrading, Error { .. })
            | (Downgrading, Active)
            | (Downgrading, Error { .. })
            | (Unloading, Inactive)
            | (Error { .. }, Queued)     // retry
            | (Error { .. }, Inactive)   // max retries exceeded
        )
    }

    pub fn increments_revision(&self) -> bool {
        matches!(self, ChunkState::Active | ChunkState::Inactive)
    }
}

pub struct ChunkEntry {
    pub state: ChunkState,
    pub revision: u64,
    pub lod_level: u8,
    pub cancel_flag: Arc<AtomicBool>,
}
```

### Pattern 2: ChunkStateStore with Guarded Transition

**What:**DashMap wrapping all chunk entries. A single `transition` method is the only write path — no caller may modify `ChunkEntry.state` directly.

**When to use:** Both WorldUpdate (queuing) and MeshSync (completing) call this.

```rust
// src/world/chunk_store.rs
pub struct ChunkStateStore {
    map: DashMap<ChunkKey, ChunkEntry>,
}

impl ChunkStateStore {
    pub fn transition(&self, key: ChunkKey, next: ChunkState) -> Result<u64, TransitionError> {
        let mut entry = self.map.get_mut(&key).ok_or(TransitionError::NotFound)?;
        if !entry.state.can_transition_to(&next) {
            return Err(TransitionError::InvalidEdge {
                from: entry.state.clone(), to: next,
            });
        }
        let increments = next.increments_revision();
        entry.state = next;
        if increments { entry.revision += 1; }
        Ok(entry.revision)
    }

    pub fn insert_inactive(&self, key: ChunkKey, lod_level: u8) {
        self.map.entry(key).or_insert_with(|| ChunkEntry {
            state: ChunkState::Inactive,
            revision: 0,
            lod_level,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        });
    }

    pub fn cancel(&self, key: &ChunkKey) {
        if let Some(entry) = self.map.get(key) {
            entry.cancel_flag.store(true, Ordering::Release);
        }
    }
}
```

### Pattern 3: Bounded Priority Queue with Eviction

**What:** `Mutex<BinaryHeap<PrioritizedTask>>` capped at configurable capacity. On insert when full, evict the lowest-SSE task. SSE stored as `f32::to_bits()` (u32) for total ordering.

**When to use:** WorldUpdate calls `enqueue` for each new chunk needing load; MeshSync does not touch the queue.

```rust
// src/world/job_queue.rs
#[derive(Eq, PartialEq)]
pub struct PrioritizedTask {
    pub sse_bits: u32,   // f32::to_bits() — higher = more important
    pub key: ChunkKey,
    pub lod_level: u8,
    pub cancel_flag: Arc<AtomicBool>,
}
impl Ord for PrioritizedTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.sse_bits.cmp(&other.sse_bits)
    }
}
impl PartialOrd for PrioritizedTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> { Some(self.cmp(other)) }
}

pub struct ChunkJobQueue {
    inner: Mutex<BinaryHeap<PrioritizedTask>>,
    pub capacity: usize,
}

impl ChunkJobQueue {
    /// Returns the evicted task if queue was full.
    pub fn enqueue(&self, task: PrioritizedTask) -> Option<PrioritizedTask> {
        let mut heap = self.inner.lock().unwrap();
        if heap.len() < self.capacity {
            heap.push(task);
            return None;
        }
        // Evict lowest SSE: drain, drop minimum, re-insert remainder + new task.
        // At capacity=128 and 16 inserts/frame this is 128 comparisons — acceptable.
        let mut tasks: Vec<_> = heap.drain().collect();
        tasks.sort_unstable(); // ascending sse_bits
        let evicted = tasks.remove(0);
        for t in tasks { heap.push(t); }
        heap.push(task);
        Some(evicted)
    }

    pub fn drain_up_to(&self, n: usize) -> Vec<PrioritizedTask> {
        let mut heap = self.inner.lock().unwrap();
        (0..n).filter_map(|_| heap.pop()).collect()
    }
}
```

### Pattern 4: Rayon Task with Cancel Flag

**What:** `pool.spawn` captures owned data and `Arc<AtomicBool>`. Task checks flag at generation checkpoints. Sends `ChunkJobResult` via cloned `mpsc::Sender` regardless of outcome.

**When to use:** WorldUpdate calls this for each task dequeued from `ChunkJobQueue.drain_up_to(per_frame_cap)`.

```rust
// src/world/job_runner.rs
pub struct ChunkJobResult {
    pub key: ChunkKey,
    pub lod_level: u8,
    pub outcome: ChunkJobOutcome,
}
pub enum ChunkJobOutcome {
    Generated(Box<ChunkData>),
    Cancelled,
    Failed(String),
}

pub fn spawn_chunk_task(
    pool: &rayon::ThreadPool,
    task: PrioritizedTask,
    sender: std::sync::mpsc::Sender<ChunkJobResult>,
) {
    pool.spawn(move || {
        if task.cancel_flag.load(Ordering::Acquire) {
            let _ = sender.send(ChunkJobResult {
                key: task.key, lod_level: task.lod_level,
                outcome: ChunkJobOutcome::Cancelled,
            });
            return;
        }
        match generate_chunk_data(task.key, task.lod_level, &task.cancel_flag) {
            Ok(data) => { let _ = sender.send(ChunkJobResult {
                key: task.key, lod_level: task.lod_level,
                outcome: ChunkJobOutcome::Generated(Box::new(data)),
            }); }
            Err(e) => { let _ = sender.send(ChunkJobResult {
                key: task.key, lod_level: task.lod_level,
                outcome: ChunkJobOutcome::Failed(e.to_string()),
            }); }
        }
    });
}
```

### Pattern 5: SSE Computation and Active-Set Diff

**When to use:** Inside WorldUpdate stage arm each frame.

```rust
// src/world/sse.rs
const GEOMETRIC_ERROR_M: [f32; 3] = [4.0, 32.0, 256.0]; // LOD0, LOD1, LOD2

pub fn compute_sse(lod_level: u8, chunk_center: glam::Vec3, cam: &CameraParams) -> f32 {
    let dist = cam.position.distance(chunk_center).max(0.001);
    let geo_err = GEOMETRIC_ERROR_M[lod_level as usize];
    (geo_err * cam.screen_height_px) / (2.0 * dist * (cam.fov_y_radians * 0.5).tan())
}

// Returns (to_activate, to_deactivate)
pub fn active_set_diff(
    prev: &HashSet<ChunkKey>,
    next: &HashSet<ChunkKey>,
) -> (Vec<ChunkKey>, Vec<ChunkKey>) {
    let to_activate = next.difference(prev).copied().collect();
    let to_deactivate = prev.difference(next).copied().collect();
    (to_activate, to_deactivate)
}
```

### Pattern 6: WorldUpdate and MeshSync Stage Arms

**What:** The two previously empty stage arms in `scheduler::run_frame` are filled.

```rust
// In scheduler::run_frame  (extends existing match)
Stage::WorldUpdate => {
    // 1. Traverse octree, compute SSE per node, build desired active set
    let desired = world_state.octree.traverse_sse(&world_state.camera, SSE_THRESHOLD_PX);
    // 2. Diff against previous frame
    let (to_activate, to_deactivate) = active_set_diff(&world_state.prev_active, &desired);
    // 3. Enqueue activations (up to per_frame_cap)
    let mut submitted = 0;
    for key in to_activate.iter().take(world_state.config.per_frame_cap) {
        let sse = compute_sse(key.lod_level, key.center(), &world_state.camera);
        let cancel_flag = Arc::new(AtomicBool::new(false));
        world_state.chunk_store.transition(*key, ChunkState::Queued).ok();
        let evicted = world_state.job_queue.enqueue(PrioritizedTask {
            sse_bits: sse.to_bits(), key: *key,
            lod_level: key.lod_level, cancel_flag: cancel_flag.clone(),
        });
        if let Some(evicted_task) = evicted {
            evicted_task.cancel_flag.store(true, Ordering::Release);
        }
        submitted += 1;
    }
    // 4. Dequeue and spawn up to per_frame_cap
    let to_spawn = world_state.job_queue.drain_up_to(
        world_state.config.per_frame_cap.saturating_sub(submitted)
    );
    for task in to_spawn {
        world_state.chunk_store.transition(task.key, ChunkState::Loading).ok();
        spawn_chunk_task(&world_state.pool, task, world_state.result_tx.clone());
    }
    // 5. Signal deactivations
    for key in &to_deactivate {
        world_state.chunk_store.cancel(key);
        world_state.chunk_store.transition(*key, ChunkState::Unloading).ok();
    }
    world_state.prev_active = desired;
}

Stage::MeshSync => {
    // Drain all completed results non-blocking
    while let Ok(result) = world_state.result_rx.try_recv() {
        match result.outcome {
            ChunkJobOutcome::Generated(_data) => {
                world_state.chunk_store.transition(result.key, ChunkState::Active).ok();
                // Phase 3 will wire _data into render pipeline
            }
            ChunkJobOutcome::Cancelled => {
                world_state.chunk_store.transition(result.key, ChunkState::Inactive).ok();
            }
            ChunkJobOutcome::Failed(ref msg) => {
                // Retrieve current retry_count from store to compute next
                if let Some(entry) = world_state.chunk_store.get(result.key) {
                    let retry_count = match &entry.state {
                        ChunkState::Error { retry_count, .. } => *retry_count,
                        _ => 0,
                    };
                    let max_retries = world_state.config.max_retry_count;
                    let next_state = if retry_count >= max_retries {
                        ChunkState::Inactive
                    } else {
                        let backoff = 1u64 << retry_count.min(6);
                        ChunkState::Error {
                            retry_count: retry_count + 1,
                            next_retry_frame: frame_index + backoff,
                        }
                    };
                    world_state.chunk_store.transition(result.key, next_state).ok();
                }
            }
        }
    }
    // Re-queue Error chunks whose backoff has elapsed
    world_state.chunk_store.retry_eligible(frame_index, |key| {
        world_state.chunk_store.transition(key, ChunkState::Queued).ok();
    });
}
```

### Anti-Patterns to Avoid

- **Mutating ChunkEntry.state directly:** Bypasses valid-edge enforcement and revision gating. All writes through `ChunkStateStore::transition`.
- **Holding the queue Mutex during rayon spawn:** Release the Mutex from BinaryHeap operations before calling `pool.spawn`.
- **One mpsc channel per task:** Create one `(Sender, Receiver)` pair for the whole job system; clone Sender into each task.
- **f32::NAN in BinaryHeap:** NaN breaks total order; clamp SSE to `f32::MAX` before `to_bits()`.
- **Drain-rebuild eviction at large capacity:** Acceptable at 128; replace with a min-max heap if capacity grows beyond 1024.
- **Checking cancel flag only at task start:** Check at each generation loop iteration (every Z-slice, every octave) to allow early exit on large chunks.
- **Publishing ChunkLifecycleApplied before state is Advanced:** The EventBus emits `ChunkLifecycleApplied` on command acceptance in Simulation; this is correct. The actual state advance (Queued/Loading/Active) happens in WorldUpdate/MeshSync. Do not conflate command acknowledgment with state machine progress.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Thread pool | Custom spawn loop with std::thread | rayon ThreadPool | Work-stealing, panic propagation, thread count tuning already solved |
| Concurrent chunk registry | Mutex<HashMap> for full map | dashmap | Sharded locks; much lower contention under concurrent WorldUpdate reads + MeshSync writes |
| SSE formula | Custom distance-radius heuristic | The locked## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + Cargo integration tests |
| Config file | None — Cargo discovers `tests/*.rs` automatically |
| Quick run command | `cargo test -p revoxelation phase2 -- --nocapture` |
| Full suite command | `cargo test -p revoxelation` |

### Phase Requirements to Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|--------------|
| STRM-01 | Active-set diff produces correct activate/deactivate lists | unit | `cargo test -p revoxelation active_set_diff` | Wave 0 |
| STRM-01 | SSE formula matches expected value for known inputs | unit | `cargo test -p revoxelation compute_sse` | Wave 0 |
| STRM-01 | WorldUpdate stage dispatches ChunkLifecycleCommand with lod_level | integration | `cargo test -p revoxelation phase2_world_update` | Wave 0 |
| STRM-02 | State machine rejects invalid transitions | unit | `cargo test -p revoxelation chunk_state_invalid_transition` | Wave 0 |
| STRM-02 | Revision increments only on Active and Inactive entry | unit | `cargo test -p revoxelation chunk_state_revision_gate` | Wave 0 |
| STRM-02 | Full lifecycle round-trip: Inactive->Queued->Loading->Active->Unloading->Inactive | integration | `cargo test -p revoxelation phase2_lifecycle_roundtrip` | Wave 0 |
| STRM-03 | Queue evicts lowest-SSE task when at capacity | unit | `cargo test -p revoxelation job_queue_eviction` | Wave 0 |
| STRM-03 | Cancelled task sends Cancelled result via mpsc | unit | `cargo test -p revoxelation job_runner_cancel` | Wave 0 |
| STRM-03 | MeshSync drains completed results and advances state | integration | `cargo test -p revoxelation phase2_mesh_sync_drain` | Wave 0 |
| STRM-03 | Error retry with exponential backoff re-queues at correct frame | unit | `cargo test -p revoxelation chunk_error_retry_backoff` | Wave 0 |

### Sampling Rate

- **Per task commit:** `cargo test -p revoxelation 2>&1 | tail -5`
- **Per wave merge:** `cargo test -p revoxelation`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `tests/phase2_world_update.rs` — covers STRM-01 WorldUpdate integration
- [ ] `tests/phase2_lifecycle_roundtrip.rs` — covers STRM-02 full state machine
- [ ] `tests/phase2_mesh_sync_drain.rs` — covers STRM-03 MeshSync integration
- [ ] `src/world/mod.rs` and all submodules — new module, none exist yet
- [ ] Unit tests inline in `src/world/chunk_state.rs`, `src/world/job_queue.rs`, `src/world/sse.rs`

Framework install: none needed — `#[test]` is built into Rust.

---

## Sources

### Primary (HIGH confidence)
- Cargo.toml — exact versions of rayon 1.10, dashmap 6.1, glam 0.30, noise 0.9, rand 0.9 confirmed from project file
- `src/runtime/scheduler.rs` — confirmed WorldUpdate and MeshSync are empty arms; exact extension points identified
- `src/runtime/events/command.rs` — confirmed ChunkLifecycleCommand structure; lod_level field absent, confirmed addition target
- `src/runtime/events/bus.rs` — confirmed EventBus mpsc-style pattern; publish/consume architecture
- `src/runtime/boundaries/` — confirmed DomainSystem registration pattern for new systems
- Rust stdlib documentation (training data, stable since 1.0) — AtomicU64, AtomicBool, Mutex, BinaryHeap, mpsc::channel APIs
- rayon 1.10 (training data, stable API) — ThreadPoolBuilder, pool.spawn, FnOnce closure capture
- dashmap 6.1 (training data, stable API) — entry(), and_modify(), get_mut()

### Secondary (MEDIUM confidence)
- CONTEXT.md locked decisions — SSE formula, state machine topology, queue policy all verbatim from user decisions
- Phase 1 integration test pattern (`tests/phase1_events.rs`) — confirmed test file naming, import style, selector bootstrap pattern

### Tertiary (LOW confidence)
- Web search results: returned Cursor support bot responses instead of actual results; no external sources consulted
- docs.rs: blocked by network policy; API knowledge from training data only

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all crates confirmed in Cargo.toml; APIs are stable stdlib + rayon/dashmap stable interfaces
- Architecture: HIGH — derived directly from locked CONTEXT.md decisions and existing Phase 1 code structure
- Pitfalls: HIGH — standard Rust concurrency pitfalls (Mutex across spawn, NaN in BinaryHeap, stale cancel flags) well-documented in Rust community
- SSE formula: HIGH — formula locked in CONTEXT.md verbatim
- Octree layout: MEDIUM — flat array recommended but pointer tree equally valid; choice deferred to planner per Claude's Discretion

**Research date:** 2026-03-21
**Valid until:** 2026-04-21 (stable crates; unlikely to change)
