---
phase: 04-rendering-foundation-overhaul
plan: 06
subsystem: renderer
tags: [vulkan, hiz, occlusion-culling, depth-pyramid, compute-shader]

requires:
  - phase: 04-rendering-foundation-overhaul/04-05
    provides: "GPU frustum culling with cull_pipeline and chunk_cull.comp"
  - phase: 04-rendering-foundation-overhaul/04-04
    provides: "Depth image with SAMPLED usage flag for Hi-Z pyramid sourcing"
provides:
  - "Hi-Z depth pyramid (R32_SFLOAT, full mip chain, per-frame generation)"
  - "GPU occlusion culling via Hi-Z sampling in cull shader"
  - "Runtime toggle (hiz_enabled) for Hi-Z culling"
affects: [phase-5-bindless, phase-6-meshlet]

tech-stack:
  added: []
  patterns: ["1-frame temporal depth pyramid for occlusion culling", "per-mip compute dispatch with barriers"]

key-files:
  created:
    - "shaders/hiz_generate.comp"
    - "src/renderer/hiz.rs"
  modified:
    - "shaders/chunk_cull.comp"
    - "src/renderer/cull_pipeline.rs"
    - "src/renderer/submit.rs"
    - "src/renderer/mod.rs"
    - "build.rs"
    - "tests/phase4_rendering.rs"
    - "tests/phase3_meshing.rs"

key-decisions:
  - "Hi-Z image format: R32_SFLOAT with full mip chain from ceil(log2(max(w,h)))+1 levels"
  - "Hi-Z generation: per-mip compute dispatch with 8x8 workgroup, 2x2 max downsampling via sampler2D"
  - "Conservative AABB projection: inflate screen-space rect by 1 texel in each direction"
  - "1-frame temporal latency: cull shader reads last frame's Hi-Z, generation after current frame's render pass"
  - "Hi-Z config SSBO (binding 6) carries view_proj + hiz_size + hiz_enabled + mip_count"
  - "4-corner sampling of Hi-Z pyramid for conservative max-depth test"

patterns-established:
  - "Temporal depth pyramid: depth→shader_read→compute generate→shader_read→depth cycle"
  - "Per-mip compute dispatch with inter-mip barriers (GENERAL→SHADER_READ for src, GENERAL for dst)"

requirements-completed: [REND-04]

duration: 11min
completed: 2026-03-25
---

# Phase 4 Plan 06: Hi-Z Occlusion Culling Summary

**Hi-Z depth pyramid with per-frame compute generation and conservative screen-space AABB occlusion test integrated into GPU-driven cull shader**

## Performance

- **Duration:** 11 min
- **Started:** 2026-03-25T12:57:33Z
- **Completed:** 2026-03-25T13:08:33Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- Hi-Z depth pyramid (R32_SFLOAT, full mip chain) generated per frame from previous frame's depth buffer
- Conservative screen-space AABB projection with 1-texel inflation prevents popping
- GPU occlusion culling rejects chunks fully behind nearer geometry before draw
- Runtime toggle (`hiz_enabled`) for debugging without Hi-Z

## Task Commits

Each task was committed atomically:

1. **Task 1: Hi-Z pyramid image and generation shader** - `2398642` (feat)
2. **Task 2: Integrate Hi-Z occlusion test into cull shader** - `ccd956d` (feat)

## Files Created/Modified
- `shaders/hiz_generate.comp` - Compute shader: 2×2 max downsampling per mip level (8×8 workgroup)
- `src/renderer/hiz.rs` - HiZPyramid struct: image, mip views, sampler, pipeline, generate() dispatch
- `shaders/chunk_cull.comp` - Extended with Hi-Z occlusion test after frustum cull, hiz_enabled toggle
- `src/renderer/cull_pipeline.rs` - Added bindings 6 (HiZConfig SSBO) and 7 (Hi-Z combined image sampler), HiZConfig struct
- `src/renderer/submit.rs` - Hi-Z generation after render pass with depth layout transitions
- `src/renderer/mod.rs` - Added hiz module, HiZPyramid field, shader source list, cleanup
- `build.rs` - Added hiz_generate.comp to shader compilation list
- `tests/phase4_rendering.rs` - Added 6 Hi-Z tests (mip count, shader content, module existence, cull integration, toggle)
- `tests/phase3_meshing.rs` - Updated submit_frame_sequence to include hiz_generate step

## Decisions Made
- Hi-Z image: R32_SFLOAT, mip count = ceil(log2(max(w,h)))+1. Full mip chain from swapchain resolution.
- Generation shader reads source via sampler2D (textureLod) for automatic filtering, writes via imageStore.
- Cull shader projects all 8 AABB corners, finds screen-space rect, determines mip level where rect ≤ 2×2 texels, samples 4 corners at that mip for conservative max-depth comparison.
- HiZConfig SSBO (76 bytes) carries view_proj, hiz_size, hiz_enabled, hiz_mip_count for the cull shader.
- Depth image layout cycle: DEPTH_ATTACHMENT → SHADER_READ (for Hi-Z gen) → DEPTH_ATTACHMENT (for next frame).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Updated phase3_meshing test for new submit_frame_sequence**
- **Found during:** Task 2 (submit.rs update)
- **Issue:** mesh_03_build_script_and_indirect_submit_contract expected old 9-step sequence, now 10 steps with hiz_generate
- **Fix:** Updated expected sequence in tests/phase3_meshing.rs
- **Files modified:** tests/phase3_meshing.rs
- **Verification:** cargo test passes
- **Committed in:** ccd956d (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary fix for correctness of existing test. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Hi-Z occlusion culling infrastructure complete
- Ready for Plan 04-07 (pipeline cache, performance counters, shader hot-reload)
- After 04-07, Phase 4 is complete, ready for Phase 5 (Bindless Architecture)

---
*Phase: 04-rendering-foundation-overhaul*
*Completed: 2026-03-25*
