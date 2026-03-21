---
phase: 03-greedy-meshing-and-render-delta-sync
plan: 01
subsystem: meshing
tags: [greedy-meshing, voxels, invalidation, scheduler, packed-mesh]
requires:
  - phase: 02.5-vulkan-bootstrap-and-render-infrastructure
    provides: MeshSync stage hook plus Vulkan renderer/staging infrastructure for later render handoff
provides:
  - typed ChunkVoxels payloads and packed mesh contracts
  - dedicated MeshingState ownership with neighbor invalidation and finer-neighbor face masks
  - halo-aware greedy meshing that emits PackedMesh results during MeshSync
affects: [phase-03-02, render-sync, chunk-pool, indirect-draw]
tech-stack:
  added: []
  patterns: [typed chunk payload contract, separate meshing ownership, bounded dirty-batch meshing]
key-files:
  created:
    - src/meshing/mod.rs
    - src/meshing/packing.rs
    - src/meshing/invalidation.rs
    - src/meshing/greedy.rs
    - tests/phase3_meshing.rs
  modified:
    - src/lib.rs
    - src/streaming/types.rs
    - src/streaming/job_runner.rs
    - src/runtime/scheduler.rs
key-decisions:
  - "Kept meshing dirtiness in a dedicated MeshingState instead of extending ChunkState so lifecycle and remesh ownership stay separate."
  - "Made greedy meshing halo-aware and skirt-flagged at the packed vertex contract so later renderer work can consume seam-safe meshes without reinterpretation."
patterns-established:
  - "ChunkJobOutcome::Generated carries ChunkVoxels instead of raw bytes, so downstream phases only consume validated payloads."
  - "Stage::MeshSync drains a bounded dirty queue and emits PackedMesh results instead of reprocessing the whole active world."
requirements-completed: [MESH-01, MESH-02]
duration: 15 min
completed: 2026-03-22
---

# Phase 03 Plan 01: Greedy Meshing Summary

**Typed chunk voxel payloads, dedicated meshing invalidation state, and halo-aware greedy mesh emission feeding PackedMesh outputs**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-22T02:18:35.838+08:00
- **Completed:** 2026-03-22T02:33:57+08:00
- **Tasks:** 3
- **Files modified:** 9

## Accomplishments
- Replaced raw generated chunk payloads with validated `ChunkVoxels` data and explicit `PackedVertex` / `PackedMesh` renderer contracts.
- Split meshing ownership into `MeshingState`, including dirty-cause tracking, same-LOD border invalidation, and coarse-face finer-neighbor masks for skirts.
- Implemented halo-aware greedy meshing in `Stage::MeshSync`, producing `PackedMesh` outputs from bounded dirty batches with requirement-focused tests.

## Task Commits

Each task was committed atomically:

1. **Task 1: Create the typed meshing contracts and phase test scaffold** - `ce827cb` (feat)
2. **Task 2: Introduce separate meshing dirty state and neighbor invalidation wiring** - `af607f0` (feat)
3. **Task 3: Implement greedy quad generation and produce PackedMesh results** - `6bf98b6` (feat)

## Files Created/Modified
- `src/lib.rs` - exports the new `meshing` module.
- `src/streaming/types.rs` - defines `CHUNK_EDGE`, `CHUNK_VOXEL_COUNT`, `ChunkVoxels`, and the typed `ChunkJobOutcome::Generated`.
- `src/streaming/job_runner.rs` - emits validated typed chunk voxel payloads from generation jobs.
- `src/runtime/scheduler.rs` - stores meshing payloads, marks dirty neighbors, and drains dirty batches into `MeshingJobResult` outputs.
- `src/meshing/mod.rs` - declares the meshing contracts, quad/result types, and re-exports.
- `src/meshing/packing.rs` - defines packed vertex/mesh layout and quad emission helpers.
- `src/meshing/invalidation.rs` - owns dirty-cause records, neighbor invalidation helpers, and bounded dirty-batch queuing.
- `src/meshing/greedy.rs` - implements halo-aware greedy quad generation and skirt emission.
- `tests/phase3_meshing.rs` - covers contract validation, border invalidation, greedy quad emission, and skirt mask behavior.

## Decisions Made
- Kept meshing dirtiness separate from streaming lifecycle state to preserve a clear ownership boundary between activation and remesh work.
- Encoded skirt geometry in the packed vertex format so the renderer can distinguish coarse-edge skirts without re-deriving topology.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `03-02` can now consume `MeshingState.completed_meshes` and `PackedMesh` outputs to build incremental renderer deltas.
- Greedy meshing, border invalidation, and coarse-face skirt masks are now test-covered, so renderer-side work can focus on upload/culling/draw wiring rather than CPU mesh correctness.

---
*Phase: 03-greedy-meshing-and-render-delta-sync*
*Completed: 2026-03-22*
