---
phase: 02-streaming-lifecycle-and-job-queues
plan: 02
subsystem: streaming
tags: [rust, rayon, mpsc, job-queue, sse, chunk-lifecycle, retry]

# Dependency graph
requires:
  - phase: 02-01
    provides: ChunkStateStore, StreamingOctree, diff_active_set, ChunkKey/ChunkState/ChunkJobOutcome types
provides:
  - ChunkJobQueue with capacity-bounded SSE-priority eviction
  - spawn_chunk_job returning Arc<AtomicBool> cancel flag via rayon ThreadPool
  - StreamingState singleton (OnceLock) wiring octree + state_store + job_queue + rayon_pool
  - WorldUpdate stage: diff_active_set -> enqueue -> drain_up_to(16) -> spawn_chunk_job
  - MeshSync stage: try_recv loop with Generated/Cancelled/Failed retry logic
  - MAX_RETRIES=3 exponential backoff (next_retry_frame = frame + 2^retry_count)
  - WorldStreamingSystem and MeshSyncSystem boundary registry stubs
affects: [03-world-mutation, 04-rendering, job-queue-consumers]

# Tech tracking
tech-stack:
  added: [rayon ThreadPool per-pool (not global), std::sync::OnceLock, std::sync::mpsc]
  patterns:
    - OnceLock<Mutex<StreamingState>> singleton for frame-scoped shared state
    - Drain-then-spawn pattern: collect tasks, mutate state, then borrow pool
    - AtomicBool cancel flags per in-flight chunk key
    - Exponential retry backoff tracked in ChunkState::Error { retry_count, next_retry_frame }

key-files:
  created:
    - src/streaming/job_queue.rs
    - src/streaming/job_runner.rs
    - tests/phase2_streaming.rs
  modified:
    - src/streaming/types.rs
    - src/streaming/state_store.rs
    - src/streaming/mod.rs
    - src/runtime/scheduler.rs
    - src/runtime/boundaries/world.rs
    - src/runtime/boundaries/meshing.rs

key-decisions:
  - "ChunkState::Error promoted to struct variant with retry_count: u32 and next_retry_frame: u64 to track retry bookkeeping inline with state"
  - "ChunkJobOutcome::Generated(Box<[u8]>) added alongside existing Loaded/Unloaded/Cancelled/Failed for explicit background generation payloads"
  - "Drain-then-spawn ordering: all Queued->Loading transitions complete before pool borrow to satisfy Rust borrow checker (immutable pool ref cannot coexist with mutable state_store ref)"
  - "active_set() added to ChunkStateStore to return HashSet of Active-state keys for diff_active_set input"
  - "SseConfig::new argument order confirmed: screen_height, fov_y_radians, threshold_px, frustum_culling"

patterns-established:
  - "OnceLock singleton pattern: streaming state initialized once per process, frame functions lock/unlock Mutex"
  - "Cancel-flag pattern: Arc<AtomicBool> stored per ChunkKey in cancel_flags HashMap; cleared on MeshSync drain"
  - "Retry gate: Failed outcome reads current Error retry_count (default 0), computes backoff, transitions to Inactive at MAX_RETRIES"

requirements-completed: [STRM-01, STRM-02, STRM-03]

# Metrics
duration: 12min
completed: 2026-03-21
---

# Phase 2 Plan 02: Job Queue and Scheduler Wiring Summary

**Bounded rayon job queue with SSE-priority eviction, AtomicBool cancellation, and full WorldUpdate/MeshSync frame pipeline with exponential-backoff retry.**

## Performance

- **Duration:** 12 min
- **Started:** 2026-03-21T10:50:32Z
- **Completed:** 2026-03-21T11:02:54Z
- **Tasks:** 3 completed
- **Files modified:** 9

## Accomplishments

- Implemented `ChunkJobQueue` (capacity 128, max-heap by SSE bits) with enqueue/evict, drain_up_to, cancel_queued; and `spawn_chunk_job` on rayon `ThreadPool` returning `Arc<AtomicBool>` cancel flag
- Extended `ChunkState::Error` to struct variant and added `ChunkJobOutcome::Generated` to support retry bookkeeping and generation payloads
- Wired full streaming frame pipeline: WorldUpdate diffs active set, enqueues, drains 16/frame, spawns jobs; MeshSync drains channel, advances state, retries Failed outcomes with exponential backoff up to MAX_RETRIES=3

## Task Commits

Each task was committed atomically:

1. **Task 1: ChunkJobQueue and spawn_chunk_job** - `e8ac572` (feat)
2. **Task 2: WorldStreamingSystem and MeshSyncSystem boundary stubs** - `24a5237` (feat)
3. **Task 3: WorldUpdate/MeshSync scheduler arms and integration tests** - `0be455d` (feat)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] ChunkState::Error was a unit variant; plan requires struct variant with retry fields**
- **Found during:** Task 1 (type extension)
- **Issue:** Plan spec referenced `Error { retry_count: u32, next_retry_frame: u64 }` but actual code had `Error` as a unit variant
- **Fix:** Promoted `ChunkState::Error` to struct variant; updated all `matches!` patterns in `state_store.rs` to use `Error { .. }` wildcards
- **Files modified:** `src/streaming/types.rs`, `src/streaming/state_store.rs`
- **Commit:** e8ac572

**2. [Rule 2 - Missing critical] active_set() missing from ChunkStateStore**
- **Found during:** Task 3 (scheduler wiring)
- **Issue:** `diff_active_set` requires a `HashSet<ChunkKey>` of currently active chunks; `ChunkStateStore` had no method to produce this
- **Fix:** Added `active_set()` method to `ChunkStateStore` filtering entries by `ChunkState::Active`
- **Files modified:** `src/streaming/state_store.rs`
- **Commit:** 0be455d

**3. [Rule 1 - Bug] SseConfig::new argument mismatch**
- **Found during:** Task 3 (scheduler compilation)
- **Issue:** Plan context showed a 3-arg constructor; actual signature is `(screen_height, fov_y_radians, threshold_px, frustum_culling)` with 4 args
- **Fix:** Corrected call to `SseConfig::new(720.0, FRAC_PI_3, 1.0, false)`
- **Files modified:** `src/runtime/scheduler.rs`
- **Commit:** 0be455d

**4. [Rule 1 - Bug] Borrow checker: mutable state_store ref conflicted with immutable pool ref**
- **Found during:** Task 3 (scheduler compilation)
- **Issue:** `let pool = &ss.rayon_pool` held an immutable borrow while the loop tried to call `ss.state_store.transition_to` (mutable)
- **Fix:** Restructured WorldUpdate to complete all state mutations before borrowing the pool (drain-then-spawn ordering)
- **Files modified:** `src/runtime/scheduler.rs`
- **Commit:** 0be455d

## Test Coverage

| Requirement | Tests |
|-------------|-------|
| STRM-01 (queue priority/eviction) | queue_evicts_lowest_sse, queue_drain_highest_first, cancel_queued_removes |
| STRM-02 (background execution) | spawn_sends_result, spawn_cancelled_result |
| STRM-03 (bounded queue + cancellation + retry) | mesh_sync_failed_outcome_increments_retry, full_round_trip, cancel_in_flight_no_panic |

**Total tests: 37 (21 unit + 16 integration) — all passing, zero regressions**

## Self-Check: PASSED

| Item | Status |
|------|--------|
| src/streaming/job_queue.rs | FOUND |
| src/streaming/job_runner.rs | FOUND |
| tests/phase2_streaming.rs | FOUND |
| .planning/.../02-02-SUMMARY.md | FOUND |
| commit e8ac572 | FOUND |
| commit 24a5237 | FOUND |
| commit 0be455d | FOUND |
