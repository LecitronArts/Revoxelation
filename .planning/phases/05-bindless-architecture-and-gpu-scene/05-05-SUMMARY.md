---
phase: 05-bindless-architecture-and-gpu-scene
plan: 05
subsystem: renderer
tags: [vulkan, indirect-count, dynamic-capacity, bindless, gpu-driven]

requires:
  - phase: 05-bindless-architecture-and-gpu-scene (plan 03)
    provides: Unified scene_buffer, GpuChunkInstance, SlotAllocator
  - phase: 05-bindless-architecture-and-gpu-scene (plan 04)
    provides: BlockMaterial, texture array, fragment shader sampling
provides:
  - Dynamic capacity growth for ChunkPool (1024 initial, 2x doubling)
  - vkCmdDrawIndexedIndirectCount replacing vkCmdDrawIndexedIndirect
  - GPU-driven draw count (CPU no longer controls draw count)
  - BIND-05 satisfied, Phase 5 complete
affects: [06-lod-streaming-and-virtual-texturing, 07-terrain-generation]

tech-stack:
  added: []
  patterns:
    - "Dynamic GPU buffer growth with copy-and-swap between frames"
    - "IndirectCount draw for GPU-driven rendering pipeline"

key-files:
  created: []
  modified:
    - src/renderer/chunk_pool.rs
    - src/renderer/mesh_pipeline.rs
    - src/renderer/submit.rs
    - src/renderer/perf_counters.rs
    - src/app.rs
    - tests/phase5_bindless.rs
    - tests/phase3_meshing.rs

key-decisions:
  - "INITIAL_CAPACITY=1024 replaces MAX_RENDER_CHUNKS=881; capacity is runtime-dynamic"
  - "Growth trigger: active > capacity * 0.9; growth factor: 2x doubling"
  - "Growth between frames via submit_one_shot_commands (after fence wait, before command recording)"
  - "vkCmdDrawIndexedIndirectCount uses draw_count_buffer from cull shader; max_draw_count = capacity"
  - "CPU active_draw_count only used for cull dispatch workgroup count"

patterns-established:
  - "Dynamic buffer growth: allocate 2x, copy, destroy old, re-register with BindlessTable"
  - "IndirectCount draw pattern: GPU writes count, CPU provides capacity as upper bound"

requirements-completed: [BIND-05]

duration: 11min
completed: 2026-03-26
---

# Phase 05 Plan 05: Dynamic Capacity and IndirectCount Summary

**Dynamic chunk pool capacity (1024 initial, 2x growth on 90% utilization) with vkCmdDrawIndexedIndirectCount for GPU-driven draw count management**

## Performance

- **Duration:** 11 min
- **Started:** 2026-03-26T04:59:59Z
- **Completed:** 2026-03-26T05:11:23Z
- **Tasks:** 3
- **Files modified:** 7

## Accomplishments
- Replaced fixed MAX_RENDER_CHUNKS=881 with INITIAL_CAPACITY=1024 and dynamic 2x growth
- Implemented SlotAllocator::grow() extending all internal vectors and free slot pool
- Switched from vkCmdDrawIndexedIndirect to vkCmdDrawIndexedIndirectCount (Vulkan 1.2 core)
- Wired growth check into frame loop (after fence wait, before command recording)
- HUD now shows "Slots: active/capacity" for runtime monitoring

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement dynamic capacity growth in ChunkPool and SlotAllocator** - `050b252` (feat)
2. **Task 2: Switch to vkCmdDrawIndexedIndirectCount** - `9141be3` (feat)
3. **Task 3: Wire growth check into frame loop and update shaders** - `56f011f` (feat)

## Files Created/Modified
- `src/renderer/chunk_pool.rs` - INITIAL_CAPACITY=1024, SlotAllocator::grow(), ChunkPool::needs_grow()/grow_capacity()
- `src/renderer/mesh_pipeline.rs` - cmd_draw_indexed_indirect_count with draw_count_buffer + max_draw_count
- `src/renderer/submit.rs` - Growth check after fence wait; capacity-based IndirectCount draw path
- `src/renderer/perf_counters.rs` - Added chunk_capacity field
- `src/app.rs` - HUD shows Slots: active/capacity
- `tests/phase5_bindless.rs` - 6 new tests for dynamic capacity and IndirectCount
- `tests/phase3_meshing.rs` - Updated to use INITIAL_CAPACITY instead of removed MAX_RENDER_CHUNKS

## Decisions Made
- INITIAL_CAPACITY=1024 (enough for initial octree + headroom for growth)
- Growth uses submit_one_shot_commands with queue_wait_idle (acceptable since growth is rare)
- BindlessTable binding 0 re-registered after growth to point to new scene_buffer
- phase3 test loosened from assert_eq to assert!(>=) for INITIAL_CAPACITY vs octree size

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Borrow checker conflict in submit_frame growth path**
- **Found during:** Task 3 (wire growth into frame loop)
- **Issue:** Cannot borrow renderer as mutable (for grow_capacity) while bindless is borrowed immutably from renderer
- **Fix:** Take both chunk_pool and bindless out of renderer via Option::take(), then put them back after growth
- **Files modified:** src/renderer/submit.rs
- **Verification:** cargo build succeeds
- **Committed in:** 56f011f (Task 3 commit)

**2. [Rule 3 - Blocking] phase3 test referenced removed MAX_RENDER_CHUNKS constant**
- **Found during:** Task 1 (replacing MAX_RENDER_CHUNKS)
- **Issue:** tests/phase3_meshing.rs used MAX_RENDER_CHUNKS which no longer exists
- **Fix:** Updated to use INITIAL_CAPACITY with >= assertion (1024 >= 881)
- **Files modified:** tests/phase3_meshing.rs
- **Verification:** cargo test --test phase3_meshing passes (13/13)
- **Committed in:** 050b252 (Task 1 commit)

**3. [Rule 3 - Blocking] phase5 chunk_pool_three_buffers test used exact count**
- **Found during:** Task 1 (adding grow_capacity with additional buffer allocations)
- **Issue:** Test asserted exactly 3 create_allocated_buffer calls; grow_capacity adds 3 more
- **Fix:** Changed assertion from == 3 to >= 3
- **Files modified:** tests/phase5_bindless.rs
- **Verification:** cargo test --test phase5_bindless passes (22/22)
- **Committed in:** 050b252 (Task 1 commit)

---

**Total deviations:** 3 auto-fixed (1 bug, 2 blocking)
**Impact on plan:** All necessary for correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 5 (Bindless Architecture and GPU Scene) is COMPLETE
- All 5 requirements satisfied: BIND-01 through BIND-05
- Ready for Phase 6 (LOD Streaming and Virtual Texturing) or next milestone phase

---
*Phase: 05-bindless-architecture-and-gpu-scene*
*Completed: 2026-03-26*
