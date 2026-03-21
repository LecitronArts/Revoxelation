---
phase: 02-streaming-lifecycle-and-job-queues
plan: 01
subsystem: streaming
tags: [streaming, sse, octree, state-machine, lod, chunk-lifecycle]
dependency_graph:
  requires: []
  provides:
    - src/streaming/types.rs
    - src/streaming/state_store.rs
    - src/streaming/octree.rs
    - src/streaming/sse.rs
  affects:
    - src/runtime/events/command.rs
    - src/runtime/scheduler.rs
tech_stack:
  added: []
  patterns:
    - Flat-array octree (breadth-first storage, parent/child index links)
    - State machine with explicit valid-edge table (matches! macro)
    - SSE formula with divide-by-zero and NaN guards returning f32::MAX
key_files:
  created:
    - src/streaming/mod.rs
    - src/streaming/types.rs
    - src/streaming/state_store.rs
    - src/streaming/octree.rs
    - src/streaming/sse.rs
  modified:
    - src/lib.rs
    - src/runtime/events/command.rs
    - src/runtime/scheduler.rs
    - tests/phase1_events.rs
decisions:
  - ChunkState Error variant treated as 8th state (context spec lists 7+Error)
  - StreamingOctree built coarsest-to-finest so parent indices are always resolved before children
  - compute_sse returns f32::MAX (not NaN) for zero/negative dist and infinite geometric_error
  - diff_active_set uses camera_dist_fn closure for testability without camera struct dependency
metrics:
  duration: 6 min
  completed: 2026-03-21
  tasks_completed: 3
  files_created: 5
  files_modified: 4
---

# Phase 2 Plan 01: Streaming Module Scaffold Summary

**One-liner:** SSE-driven chunk lifecycle foundation — 7-state machine, flat-array octree, and screen-space error diff engine with full unit coverage.

## What Was Built

### Task 1 — Streaming module scaffold and shared types

Created `src/streaming/mod.rs` and `src/streaming/types.rs`. The types file is the single source of truth for all streaming contracts:

- `ChunkKey` — `(x, y, z, lod_level: u8)` chunk identifier
- `ChunkState` — 7 canonical states (`Inactive`, `Queued`, `Loading`, `Active`, `Upgrading`, `Downgrading`, `Unloading`) plus `Error`
- `ChunkEntry` — key + state + revision counter
- `LodConfig` — per-level geometric error and world-space chunk size
- `SseConfig` — camera parameters (screen height, FOV, threshold, frustum culling flag)
- `ChunkJobResult` / `ChunkJobOutcome` — background task result envelope
- `ActiveSet` — `HashSet<ChunkKey>` type alias

Wired `pub mod streaming` into `src/lib.rs`.

### Task 2 — ChunkStateStore

Created `src/streaming/state_store.rs`:

- `ChunkStateStore` holds a `HashMap<ChunkKey, ChunkEntry>`
- `insert_inactive` — idempotent insertion at `Inactive`
- `transition_to` — validates edges via `is_valid_transition`, rejects invalid edges with `TransitionError`, increments `revision` only on entry to `Active` or `Inactive`
- Full set of 15 valid edges encoded in a single `matches!` call

4 unit tests: `transition_inactive_to_queued`, `transition_invalid_inactive_to_loading`, `revision_increments_on_active`, `revision_increments_on_inactive`.

### Task 3 — StreamingOctree, compute_sse, diff_active_set, lod_level extension

**octree.rs:** `StreamingOctree::build(radius_chunks, levels)` populates a flat `Vec<OctreeNode>` coarsest-to-finest. Each node stores `ChunkKey` plus parent/children indices. Coarser levels cover a proportionally smaller radius in chunk-space (each coarse cell maps to `2^lod` fine cells).

**sse.rs:**
- `compute_sse(lod, cfg, dist)` — formula `(geo_err * screen_h) / (2 * dist * tan(fov/2))`. Returns `f32::MAX` on zero/negative dist, zero FOV, or non-finite intermediate values. Never returns NaN.
- `diff_active_set` — iterates all octree nodes, computes SSE via a `camera_dist_fn` closure, collects the desired set, then diffs against `current_active` to produce `to_activate` / `to_deactivate` vecs.

**command.rs:** Added `lod_level: u8` to `ChunkLifecycleCommand`.

**scheduler.rs:** Updated seed call with `lod_level: 0`.

**tests/phase1_events.rs:** Added `lod_level: 0` to two `ChunkLifecycleCommand` literals that were broken by the struct extension (Rule 1 auto-fix).

7 unit tests: `sse_known_value`, `sse_zero_dist`, `sse_no_nan`, `diff_activate_all`, `diff_deactivate_none`, `octree_builds_without_panic`, `octree_nodes_have_correct_lod`.

## Test Results

```
running 11 tests
test streaming::sse::tests::sse_known_value ... ok
test streaming::octree::tests::octree_builds_without_panic ... ok
test streaming::sse::tests::diff_activate_all ... ok
test streaming::sse::tests::sse_no_nan ... ok
test streaming::octree::tests::octree_nodes_have_correct_lod ... ok
test streaming::sse::tests::sse_zero_dist ... ok
test streaming::state_store::tests::revision_increments_on_active ... ok
test streaming::sse::tests::diff_deactivate_none ... ok
test streaming::state_store::tests::revision_increments_on_inactive ... ok
test streaming::state_store::tests::transition_inactive_to_queued ... ok
test streaming::state_store::tests::transition_invalid_inactive_to_loading ... ok
test result: ok. 11 passed; 0 failed

+ 14 Phase 1 regression tests: all pass
```

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed missing `lod_level` field in Phase 1 test fixtures**
- **Found during:** Task 3 (after adding `lod_level: u8` to `ChunkLifecycleCommand`)
- **Issue:** `tests/phase1_events.rs` had two `ChunkLifecycleCommand` struct literals without `lod_level`, causing compile error E0063
- **Fix:** Added `lod_level: 0` to both literals
- **Files modified:** `tests/phase1_events.rs`
- **Commit:** d1c7ef9

**2. [Rule 1 - Warning cleanup] Removed unused import and renamed unused variable**
- **Found during:** Task 3 (`cargo check` warnings)
- **Issue:** `OctreeNode` imported but unused in `sse.rs`; `max_lod` variable unused in `octree.rs`
- **Fix:** Removed `OctreeNode` from import; prefixed `max_lod` with `_`
- **Files modified:** `src/streaming/sse.rs`, `src/streaming/octree.rs`
- **Commit:** d1c7ef9

## Commits

| Hash | Message |
|------|---------|
| 050ca19 | feat(02-01): add streaming module scaffold with shared types |
| 9878062 | feat(02-01): add ChunkStateStore with transition enforcement and revision gating |
| d1c7ef9 | feat(02-01): add StreamingOctree, compute_sse, diff_active_set; extend ChunkLifecycleCommand with lod_level |

## Self-Check: PASSED

- src/streaming/types.rs: FOUND
- src/streaming/state_store.rs: FOUND
- src/streaming/octree.rs: FOUND
- src/streaming/sse.rs: FOUND
- src/streaming/mod.rs: FOUND
- .planning/phases/02-streaming-lifecycle-and-job-queues/02-01-SUMMARY.md: FOUND
- commit 050ca19: FOUND
- commit 9878062: FOUND
- commit d1c7ef9: FOUND
