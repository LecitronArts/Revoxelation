---
phase: 05-bindless-architecture-and-gpu-scene
plan: 03
subsystem: renderer
tags: [vulkan, ssbo, bindless, gpu-scene, indirect-draw]

requires:
  - phase: 05-bindless-architecture-and-gpu-scene (plan 02)
    provides: BindlessTable with unified set 0, shared descriptor layout for cull+mesh pipelines
provides:
  - Unified scene_buffer SSBO merging 4 per-chunk buffers into 1
  - GpuChunkInstance (48 bytes) replacing ChunkDrawMetadata
  - ChunkPool reduced from 6 GPU buffers to 3 (vertex, index, scene)
  - scene_buffer_region_offsets() for capacity-derived layout
affects: [05-04-materials, 05-05-indirect-count, phase-06-meshlets]

tech-stack:
  added: []
  patterns:
    - "Unified SSBO with capacity-derived region offsets (computed in-shader)"
    - "Raw uint array SSBO access with typed load/store accessors in compute shader"

key-files:
  created: []
  modified:
    - src/renderer/chunk_pool.rs
    - src/renderer/cull_pipeline.rs
    - src/renderer/submit.rs
    - src/renderer/mesh_pipeline.rs
    - src/renderer/bindless.rs
    - src/app.rs
    - shaders/chunk_mesh.vert
    - shaders/chunk_cull.comp
    - tests/phase5_bindless.rs

key-decisions:
  - "GpuChunkInstance (48 bytes) replaces ChunkDrawMetadata — removes first_index, vertex_offset, index_count, slot_id, _padding0; adds material_id"
  - "scene_buffer layout: 4 contiguous regions (instances, indirect templates, draw slots, dense indirect) with 16-byte alignment at boundaries"
  - "Cull shader reads scene_buffer via raw uint array with capacity-derived byte offsets calculated in-shader"
  - "Vertex shader reads GpuChunkInstance from scene_data.instances[gl_InstanceIndex] via single binding 0"
  - "Cull push constant expanded to 8 bytes: { active_draw_count: u32, capacity: u32 }"
  - "BindlessTable binding 0 now points to scene_buffer (WHOLE_SIZE); bindings 1-3 freed"

patterns-established:
  - "scene_buffer_region_offsets(capacity) → (inst, indirect, slot, dense, total) tuple for all region calculations"
  - "Typed load/store accessors in compute shader for structured region access over raw uint array"

requirements-completed: [BIND-03]

duration: 22min
completed: 2026-03-26
---

# Phase 5 Plan 03: Unified GPU Scene Buffer Summary

**Merged 4 per-chunk SSBO buffers into unified scene_buffer with GpuChunkInstance, reducing ChunkPool from 6 to 3 GPU buffers**

## Performance

- **Duration:** 22 min
- **Started:** 2026-03-26T04:34:06Z
- **Completed:** 2026-03-26T04:56:07Z
- **Tasks:** 3
- **Files modified:** 11

## Accomplishments
- Defined GpuChunkInstance (48 bytes, #[repr(C)]) replacing ChunkDrawMetadata, removing draw-command fields that don't belong in instance data
- Implemented scene_buffer_region_offsets() for capacity-derived 4-region layout with 16-byte alignment
- Refactored ChunkPool from 6 separate GPU buffers to 3 (vertex, index, scene_buffer)
- Updated vertex shader to read GpuChunkInstance from scene_data.instances[gl_InstanceIndex]
- Updated cull compute shader to read all data from single scene_buffer binding 0 via raw uint array with typed accessors
- Expanded cull push constant to 8 bytes (active_draw_count + capacity) for in-shader offset calculation
- Updated BindlessTable: binding 0 → scene_buffer (WHOLE_SIZE), bindings 1-3 now free

## Task Commits

Each task was committed atomically:

1. **Task 1: Define GpuChunkInstance and unified scene_buffer layout** - `fe1edd4` (feat) — also covers Task 2 upload/remove paths
2. **Task 1 fixup: Update legacy tests** - `9c75398` (fix) — adapted phase3/phase4 tests for new API
3. **Task 3: Update shaders for unified scene_buffer** - `e03adff` (feat) — vertex + compute shader rewrite, cull pipeline push constant expansion

## Files Created/Modified
- `src/renderer/chunk_pool.rs` — GpuChunkInstance struct, scene_buffer_region_offsets(), 3-buffer ChunkPool, unified upload/remove paths
- `shaders/chunk_mesh.vert` — GpuChunkInstance struct, scene_data.instances[gl_InstanceIndex] access
- `shaders/chunk_cull.comp` — Single scene_buffer binding 0, raw uint array with typed load/store, capacity push constant
- `src/renderer/cull_pipeline.rs` — 8-byte push constant range { active_draw_count, capacity }
- `src/renderer/submit.rs` — Pass scene_buffer_capacity to cull dispatch, barrier on scene_buffer
- `src/renderer/mesh_pipeline.rs` — Draw from scene_buffer at dense_indirect_region_offset
- `src/renderer/bindless.rs` — No changes (binding 0 type already STORAGE_BUFFER)
- `src/app.rs` — Register scene_buffer at binding 0, remove bindings 1-3 registration
- `tests/phase5_bindless.rs` — 3 new tests: GpuChunkInstance size, region offsets, 3-buffer count
- `tests/phase3_meshing.rs` — Updated for instance_shadow() / indirect_shadow() API
- `tests/phase3_gap_closure.rs` — Updated for GpuChunkInstance naming, relaxed dense_indirect assertions

## Decisions Made
- Tasks 1 and 2 merged into a single commit because upload/remove paths and struct refactoring were tightly coupled
- Cull shader uses raw uint array (`buffer SceneBuffer { uint data[]; }`) rather than typed struct overlay to allow flexible region access with capacity-derived offsets
- Vertex shader retains simple typed `GpuChunkInstance instances[]` since it only reads region 0

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Legacy test breakage from metadata_shadow → instance_shadow rename**
- **Found during:** Task 1
- **Issue:** 11 tests in phase3_meshing.rs and phase3_gap_closure.rs referenced the removed `metadata_shadow()` method and `SlotUpload.metadata` field
- **Fix:** Updated all references to `instance_shadow()` and `SlotUpload.instance`, adapted index_count checks to use `indirect_shadow()`
- **Files modified:** tests/phase3_meshing.rs, tests/phase3_gap_closure.rs, tests/phase4_rendering.rs
- **Verification:** All 137 tests pass
- **Committed in:** 9c75398

**2. [Rule 1 - Bug] phase3 cull shader test expected old binding names**
- **Found during:** Task 3
- **Issue:** `mesh_03_cull_shader_consumes_metadata_and_dense_draw_slots` asserted `ChunkDrawMetadata` and `draw_slots` string literals in shader source
- **Fix:** Relaxed assertions to accept both old and new naming (`GpuChunkInstance`, `draw_slot`, `store_dense_indirect`)
- **Files modified:** tests/phase3_gap_closure.rs
- **Verification:** All 137 tests pass
- **Committed in:** e03adff

**3. [Rule 1 - Bug] phase4 GpuOnly count assertion expected 6, now 3**
- **Found during:** Task 1
- **Issue:** `rend_05_chunk_pool_uses_gpu_only` asserted `>= 6` GpuOnly allocations; unified scene_buffer reduces to 3
- **Fix:** Relaxed threshold from 6 to 3
- **Files modified:** tests/phase4_rendering.rs
- **Verification:** Test passes
- **Committed in:** 9c75398

---

**Total deviations:** 3 auto-fixed (3 bug fixes for legacy test compatibility)
**Impact on plan:** All auto-fixes necessary for correctness after the API refactoring. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Unified scene_buffer ready for Plan 04 (materials) to add material_id population
- Bindings 1-3 now free for future use
- Capacity push constant in cull shader enables future dynamic capacity growth (Plan 05)
- Ready for Plan 05-04 (block materials and texture array)

---
*Phase: 05-bindless-architecture-and-gpu-scene*
*Completed: 2026-03-26*
