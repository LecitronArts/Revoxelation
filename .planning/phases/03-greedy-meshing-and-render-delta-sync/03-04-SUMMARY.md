---
phase: 03-greedy-meshing-and-render-delta-sync
plan: 04
subsystem: rendering
tags: [rust, vulkan, compute, indirect-draw, tdd]
requires:
  - phase: 03-greedy-meshing-and-render-delta-sync
    provides: Stable-slot storage, dense draw bookkeeping, and metadata-backed chunk placement from 03-03
provides:
  - descriptor-backed compute wiring for chunk metadata, stable indirect templates, dense draw slots, and dense indirect output
  - dense indirect draw submission that uses active draw count instead of sparse stable-slot ownership
  - incremental dense draw-slot and dense indirect buffer updates that preserve sparse stable storage slots
affects: [renderer, chunk-pool, phase-03-verification]
tech-stack:
  added: []
  patterns: [stable-slot-plus-dense-indirect, metadata-driven-compute-prep, tdd-red-green-for-renderer-gap-closure]
key-files:
  created: []
  modified:
    - tests/phase3_gap_closure.rs
    - src/renderer/chunk_pool.rs
    - src/renderer/cull_pipeline.rs
    - src/renderer/mesh_pipeline.rs
    - src/renderer/mod.rs
    - shaders/chunk_cull.comp
key-decisions:
  - "Kept metadata and indirect templates keyed by stable slot_id while mirroring only the dense draw-slot and dense indirect entries that actually changed."
  - "Used a minimal metadata-driven compute shader that copies stable templates into a dense indirect output list and gates visibility only through instanceCount."
  - "Switched graphics submission to active_draw_count() and the dense indirect buffer so sparse stable slot holes no longer affect draw submission."
patterns-established:
  - "Compute reads stable metadata/templates through descriptor-bound storage buffers and writes dense indirect commands keyed by dense draw indices."
  - "Chunk removals swap-remove only the affected dense draw-slot and dense indirect entries instead of rebuilding whole-world buffers."
requirements-completed: [MESH-01, MESH-03]
duration: 30 min
completed: 2026-03-22
---

# Phase 03 Plan 04: Gap Closure Summary

**Descriptor-backed compute command preparation and dense indirect draw submission for sparse stable chunk slots**

## Performance

- **Duration:** 30 min
- **Started:** 2026-03-22T05:00:00+08:00
- **Completed:** 2026-03-22T05:29:30+08:00
- **Tasks:** 1
- **Files modified:** 6

## Accomplishments
- Added the three `03-04` TDD coverage points for compute metadata consumption, dense indirect draw submission, and sparse-slot correctness.
- Extended `ChunkPool` with GPU-visible dense draw-slot and dense indirect buffers plus incremental updates that only touch changed dense entries.
- Replaced the no-op compute path with descriptor-backed metadata/template/draw-slot wiring and switched render submission to the dense indirect buffer with `active_draw_count()`.

## Task Commits

TDD split the single task into red and green commits:

1. **Task 1 RED: Wire compute to metadata and dense indirect submission** - `2371479` (test)
2. **Task 1 GREEN: Wire compute to metadata and dense indirect submission** - `2a6f21c` (feat)

## Files Created/Modified
- `tests/phase3_gap_closure.rs` - Adds failing-then-passing gap-closure coverage for compute shader inputs, dense indirect draw count usage, and sparse-slot dense-order correctness.
- `src/renderer/chunk_pool.rs` - Keeps stable slot ownership while adding dense draw-slot and dense indirect buffer writes that update only affected entries.
- `src/renderer/cull_pipeline.rs` - Binds metadata, stable indirect templates, dense draw slots, and dense indirect output through a compute descriptor set.
- `src/renderer/mesh_pipeline.rs` - Draws from the dense indirect command buffer.
- `src/renderer/mod.rs` - Uploads delta changes, dispatches compute over active dense draws, barriers the dense indirect buffer, and submits `active_draw_count()` commands.
- `shaders/chunk_cull.comp` - Copies stable indirect templates into the dense indirect list and toggles visibility through `instanceCount`.

## Decisions Made
- Preserved `slot_id` as the stable key for metadata and template commands so remesh/unload updates stay per-slot while draw submission becomes dense.
- Kept compute visibility intentionally minimal and metadata-driven, matching the plan's no-camera/no-frustum scope.
- Mirrored dense indirect entries on the CPU side as well so delta uploads and swap-removes can update only the touched dense indices before compute overwrites them for the frame.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

- `apply_patch` repeatedly failed on `shaders/chunk_cull.comp` with a sandbox refresh error, so that file was updated with a direct PowerShell write fallback after the rest of the patch set landed normally.
- `cargo run` did not reach renderer verification on this machine because Vulkan instance creation failed with `Layer specified does not exist` for `VK_LAYER_KHRONOS_validation`. All required automated tests passed, but the manual window check remains blocked by local Vulkan layer availability.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 3 plan work is now fully implemented and all automated verification commands pass.
- A separate phase-level verification pass can finish the live window check once the local Vulkan validation-layer environment is corrected or disabled.

## Self-Check: PASSED

- Found `.planning/phases/03-greedy-meshing-and-render-delta-sync/03-04-SUMMARY.md`
- Found task commit `2371479`
- Found task commit `2a6f21c`

---
*Phase: 03-greedy-meshing-and-render-delta-sync*
*Completed: 2026-03-22*
