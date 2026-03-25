---
phase: 04-rendering-foundation-overhaul
plan: 04
subsystem: renderer
tags: [vulkan, gpu-allocator, staging-ring, vkCmdCopyBuffer, GpuOnly]

requires:
  - phase: 04-01
    provides: "App struct DI, Renderer ownership"
provides:
  - "StagingRing — per-frame ring buffer allocator for GPU uploads"
  - "ChunkPool with GpuOnly buffers and vkCmdCopyBuffer upload path"
  - "Depth image SAMPLED usage flag for Hi-Z pyramid"
affects: [04-05, 04-06, 05-bindless-architecture]

tech-stack:
  added: []
  patterns: ["staging ring with fence-based reclamation", "vkCmdCopyBuffer for GpuOnly uploads", "transfer→compute memory barrier"]

key-files:
  created:
    - "src/renderer/staging_ring.rs"
  modified:
    - "src/renderer/chunk_pool.rs"
    - "src/renderer/submit.rs"
    - "src/renderer/mod.rs"
    - "src/renderer/swapchain.rs"
    - "src/app.rs"
    - "tests/phase4_rendering.rs"
    - "tests/phase3_meshing.rs"
    - "tests/phase25_vulkan.rs"

key-decisions:
  - "StagingRing is 32MB with 2 frame regions (16MB each), created in app.rs"
  - "All 6 chunk pool buffers changed from CpuToGpu to GpuOnly"
  - "record_upload/record_remove replace apply_upload/apply_remove for staging-based path"
  - "Global memory barrier (TRANSFER_WRITE → SHADER_READ|VERTEX_ATTRIBUTE_READ|INDEX_READ) placed after staging copies"
  - "Depth image includes SAMPLED usage flag for future Hi-Z pyramid (04-06)"

patterns-established:
  - "Staging ring pattern: allocate → write_bytes → cmd_copy_buffer per region"
  - "Fence-based reclamation: reset cursor after wait_for_fences, advance after submit"

requirements-completed: [REND-05]

duration: 28min
completed: 2026-03-25
---

# Phase 04 Plan 04: GpuOnly Memory Migration Summary

**StagingRing allocator with per-frame regions enables vkCmdCopyBuffer uploads to GpuOnly chunk pool buffers, eliminating CpuToGpu memory and queue_wait_idle from the hot path.**

## Performance

- **Duration:** 28 min
- **Started:** 2026-03-25T11:09:55Z
- **Completed:** 2026-03-25T11:37:49Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments

- Implemented StagingRing with frame-partitioned CpuToGpu buffer (32MB, 2 frames) and alignment-aware allocation
- Migrated all 6 chunk pool buffers from CpuToGpu to GpuOnly memory
- Replaced direct mapped memory writes with vkCmdCopyBuffer commands through staging ring
- Added transfer→compute memory barrier to ensure copy completion before shader reads
- Added SAMPLED usage flag to depth image for future Hi-Z pyramid generation

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement StagingRing allocator** - `d06674d` (feat)
2. **Task 2: Migrate ChunkPool to GpuOnly memory with copy-based uploads** - `d43a7d9` (feat)

## Files Created/Modified

- `src/renderer/staging_ring.rs` — New StagingRing allocator with per-frame regions, alignment support, fence-safe reclamation
- `src/renderer/chunk_pool.rs` — GpuOnly buffers, record_upload/record_remove/record_uploads via staging ring + cmd_copy_buffer
- `src/renderer/submit.rs` — staging_ring reset after fence wait, transfer→compute barrier, advance_frame after submit
- `src/renderer/mod.rs` — staging_ring field on Renderer, record_chunk_delta_uploads uses staging path
- `src/renderer/swapchain.rs` — SAMPLED usage flag on depth image
- `src/app.rs` — StagingRing creation (32MB, 2 frames) during renderer init
- `tests/phase4_rendering.rs` — 7 new rend_05_* tests for staging ring and GpuOnly verification
- `tests/phase3_meshing.rs` — Updated submit_frame_sequence to match new staging steps
- `tests/phase25_vulkan.rs` — Fixed submit_frame signature test for camera_uniforms param

## Decisions Made

- StagingRing uses 32MB total (16MB per frame), matching the locked decision D-10
- Global memory barrier (not per-buffer) after staging copies — simpler and sufficient since all chunk buffers are written each frame
- Old write_allocation_bytes and apply_upload/apply_remove retained in code but no longer used from hot path — chunk_pool allocations kept as Option<Allocation> for destroy() cleanup
- submit_frame_sequence updated to include staging_ring_reset and transfer_to_compute_barrier steps

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed submit_frame signature mismatch in phase25 and phase3 tests**
- **Found during:** Task 1 (StagingRing implementation, initial compile)
- **Issue:** Previous plan (04-02) added camera_uniforms parameter to submit_frame but phase25_vulkan test still expected old 2-arg signature. Phase3 test expected old submit_frame_sequence.
- **Fix:** Updated tests/phase25_vulkan.rs and tests/phase3_meshing.rs to match current signatures
- **Files modified:** tests/phase25_vulkan.rs, tests/phase3_meshing.rs
- **Verification:** All 23+ tests pass
- **Committed in:** d06674d (Task 1), d43a7d9 (Task 2)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary fix for pre-existing test/code mismatch from prior plan. No scope creep.

## Issues Encountered

None — the existing old `apply_upload`/`write_allocation_bytes` code was replaced cleanly by staging ring path.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- REND-05 fully satisfied: all chunk buffers use GpuOnly, staging ring provides zero-wait uploads
- Depth image SAMPLED flag ready for Hi-Z pyramid in plan 04-06
- Ready for Plan 04-05 (next in phase)

---
*Phase: 04-rendering-foundation-overhaul*
*Completed: 2026-03-25*
