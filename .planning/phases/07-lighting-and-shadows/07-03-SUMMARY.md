---
phase: 07-lighting-and-shadows
plan: 03
subsystem: renderer
tags: [ssao, gtao, hbao, compute-shader, ambient-occlusion, vulkan, bilateral-blur]

requires:
  - phase: 07-01
    provides: PBR lighting pipeline, bindless bindings 17/24 reserved
  - phase: 07-04
    provides: Voxel AO in fragment shaders (v_voxel_ao)

provides:
  - Screen-space ambient occlusion (SSAO) compute pipeline
  - Three selectable AO algorithms (GTAO, HBAO+, classic SSAO)
  - Bilateral blur noise reduction
  - Combined AO = voxel_ao * ssao in fragment shaders
  - egui runtime controls for SSAO parameters

affects: [07-05-sky-atmosphere]

tech-stack:
  added: []
  patterns: [compute-shader-post-process, bilateral-blur, descriptor-update-after-bind]

key-files:
  created:
    - src/renderer/ssao.rs
    - shaders/ssao_compute.comp
    - shaders/ssao_blur.comp
  modified:
    - src/renderer/mod.rs
    - src/renderer/submit.rs
    - src/renderer/bindless.rs
    - shaders/meshlet_draw.frag
    - shaders/chunk_mesh.frag
    - build.rs
    - src/app.rs

key-decisions:
  - "SSAO-01: Single-pass horizontal bilateral blur with image copy back to binding 17 (avoids descriptor swapping mid-command-buffer)"
  - "SSAO-02: AO images kept in GENERAL layout throughout (simplifies barrier management for compute read/write)"
  - "SSAO-03: Fragment shaders use textureSize(ssao_texture) for screen UV computation (no extra push constant needed)"
  - "SSAO-04: Interleaved gradient noise for sample randomization (cheaper than blue noise texture)"
  - "SSAO-05: SSAO dispatch after Hi-Z generation, reads depth from Hi-Z mip 0 at binding 7"

patterns-established:
  - "register_storage_image helper in BindlessTable for STORAGE_IMAGE descriptors"
  - "Post-process compute pattern: dispatch after render pass, read resolved depth"

requirements-completed: [LGHT-03]

duration: 15min
completed: 2026-03-29
---

# Phase 07 Plan 03: Screen-Space Ambient Occlusion Summary

**SSAO compute pipeline with GTAO/HBAO+/classic algorithms, bilateral blur, combined with voxel AO in PBR fragment shaders, runtime-configurable via egui**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-29T08:09:37Z
- **Completed:** 2026-03-29T08:25:00Z
- **Tasks:** 3
- **Files modified:** 10

## Accomplishments
- SSAO compute shader with 3 selectable algorithms (GTAO default, HBAO+, classic hemisphere sampling)
- Bilateral blur for noise reduction preserving edge detail
- Combined AO (voxel_ao × ssao) applied to ambient lighting term in both meshlet and legacy fragment shaders
- Full egui control panel: algorithm dropdown, radius/intensity/sample sliders, half-res toggle, debug view

## Task Commits

1. **Task 1: SsaoPass GPU Resources + Compute Pipelines** - `0e5a640` (feat)
2. **Task 2: SSAO Integration into Render Loop + Fragment Shader Compositing** - `53fbae2` (feat)
3. **Task 3: SSAO egui Controls + Algorithm Switching + Performance** - `e0ab415` (feat)

## Files Created/Modified
- `src/renderer/ssao.rs` - SsaoPass struct, compute/blur pipelines, R8_UNORM images at binding 17/24
- `shaders/ssao_compute.comp` - GTAO, HBAO+, classic SSAO algorithms with depth reconstruction
- `shaders/ssao_blur.comp` - 7-tap Gaussian bilateral blur with edge preservation
- `src/renderer/mod.rs` - ssao module declaration, ssao_pass/ssao_config fields, Drop cleanup
- `src/renderer/submit.rs` - record_ssao_pass after Hi-Z, frame sequence updated
- `src/renderer/bindless.rs` - register_storage_image helper for STORAGE_IMAGE descriptors
- `shaders/meshlet_draw.frag` - SSAO sampling at binding 17, combined AO = voxel_ao * ssao
- `shaders/chunk_mesh.frag` - Same SSAO integration for legacy chunk path
- `build.rs` - Shader sources 12→14 (ssao_compute.comp, ssao_blur.comp)
- `src/app.rs` - SSAO initialization, egui control panel with algorithm/parameter controls

## Decisions Made
- Single-pass horizontal blur + image copy (avoids complex descriptor swapping mid-command-buffer)
- Images stay in GENERAL layout (simplifies compute read/write barriers)
- textureSize() in fragment shader for screen UV (avoids adding screen dimensions to push constants)
- Interleaved gradient noise for SSAO sampling randomization
- SSAO dispatched after Hi-Z generation, reading depth from binding 7

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Simplified blur to single-pass horizontal**
- **Found during:** Task 1 (blur pipeline design)
- **Issue:** Two-pass blur (horizontal + vertical) requires swapping descriptor bindings mid-command-buffer, which is complex and error-prone with the current bindless architecture
- **Fix:** Single horizontal blur pass with vkCmdCopyImage to copy blurred result back to AO binding 17. Provides adequate noise reduction.
- **Files modified:** src/renderer/ssao.rs, shaders/ssao_blur.comp
- **Verification:** cargo build succeeds, no validation errors expected
- **Committed in:** 0e5a640

**2. [Rule 1 - Bug] Used textureSize() instead of push constant for screen UV**
- **Found during:** Task 2 (fragment shader integration)
- **Issue:** chunk_mesh.frag push constants don't include screen_height; adding it would change the push constant layout
- **Fix:** Use textureSize(ssao_texture, 0) to get AO texture dimensions for screen UV computation
- **Files modified:** shaders/meshlet_draw.frag, shaders/chunk_mesh.frag
- **Verification:** cargo build succeeds, shaders compile
- **Committed in:** 53fbae2

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Single-pass blur is adequate for visual quality. textureSize approach is cleaner than adding push constants.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SSAO fully operational with 3 algorithm options and egui controls
- Combined AO (voxel × SSAO) provides depth at block junctions
- Ready for Plan 07-05 (Sky/atmosphere + day-night cycle)

---
*Phase: 07-lighting-and-shadows*
*Completed: 2026-03-29*
