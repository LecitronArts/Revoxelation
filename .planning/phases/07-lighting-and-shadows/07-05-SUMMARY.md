---
phase: 07-lighting-and-shadows
plan: 05
subsystem: renderer
tags: [sky, atmosphere, preetham, hosek-wilkie, day-night, fog, vulkan, glsl]

requires:
  - phase: 07-lighting-and-shadows (plan 01-04)
    provides: PBR lighting, CSM shadows, SSAO, voxel AO, bindless binding 23 reserved
provides:
  - Fullscreen Preetham/Hosek-Wilkie procedural sky rendering at depth=1.0
  - Day-night cycle with sun orbit, color temperature, moonlight transition
  - 4-type distance fog (linear, exponential, exp², height) with sky-tracking color
  - Sky/atmosphere/fog egui control panels
affects: [submit, lighting, camera, shaders]

tech-stack:
  added: []
  patterns: [fullscreen-triangle-trick, sky-ssbo-binding-23, day-night-driven-lighting]

key-files:
  created:
    - src/renderer/sky.rs
    - shaders/sky.vert
    - shaders/sky.frag
  modified:
    - src/renderer/lighting.rs
    - src/renderer/submit.rs
    - src/renderer/mod.rs
    - src/app.rs
    - shaders/common.glsl
    - shaders/meshlet_draw.frag
    - shaders/chunk_mesh.frag
    - build.rs
    - tests/phase3_meshing.rs

key-decisions:
  - "LGHT-05-01: Sky renders as fullscreen triangle at depth=1.0 BEFORE geometry (geometry naturally overwrites at closer depth)"
  - "LGHT-05-02: Double-buffered sky params SSBO at binding 23 with inv_view_proj for ray direction reconstruction"
  - "LGHT-05-03: DayNightCycle drives sun_elevation/azimuth/color/intensity and syncs to LightingState when use_day_night_cycle=true"
  - "LGHT-05-04: Fog color tracks sky horizon color through day-night cycle for seamless blending"
  - "LGHT-05-05: Dynamic clear color computed from fog_color to roughly match procedural sky (reduces flicker)"
  - "LGHT-05-06: Day-night cycle starts paused (user explores controls first); default day_speed=600s (10min day)"

patterns-established:
  - "Fullscreen triangle: 3 vertices from gl_VertexIndex, no VBO, depth=1.0"
  - "Sky-fog integration: fog_color = sky horizon color at current time_of_day"

requirements-completed: [LGHT-05]

duration: 11min
completed: 2026-03-29
---

# Phase 07 Plan 05: Sky/Atmosphere/Fog Summary

**Preetham/Hosek-Wilkie procedural sky rendering with day-night cycle, moonlight, sun disk, night stars, and 4-type configurable distance fog**

## Performance

- **Duration:** 11 min
- **Started:** 2026-03-29T08:42:58Z
- **Completed:** 2026-03-29T08:54:07Z
- **Tasks:** 3
- **Files modified:** 11

## Accomplishments
- Fullscreen procedural sky with Preetham analytical model (sun, zenith, horizon gradients)
- Day-night cycle: sun orbits, warm dawn/dusk, blue moonlight at night, star field
- Distance fog fades far objects seamlessly to sky horizon color (4 fog types)
- Complete egui control panels for atmosphere model, turbidity, day speed, fog parameters

## Task Commits

Each task was committed atomically:

1. **Task 1: Sky Renderer + Fullscreen Sky Shader + Preetham Model** - `044ebca` (feat)
2. **Task 2: Day-Night Cycle + Moonlight + Distance Fog** - `971c395` (feat)
3. **Task 3: Sky Render Pass Integration + egui Controls** - `ea7b198` (feat)

## Files Created/Modified
- `src/renderer/sky.rs` — SkyRenderer: pipeline, double-buffered SSBO, fullscreen draw
- `shaders/sky.vert` — Fullscreen triangle vertex shader (depth=1.0)
- `shaders/sky.frag` — Preetham + Hosek-Wilkie atmosphere, sun disk, night sky
- `src/renderer/lighting.rs` — DayNightCycle, FogConfig, FogType, sun orbit + color temperature
- `src/renderer/submit.rs` — Dynamic clear color, sky draw integration, sky SSBO upload
- `src/renderer/mod.rs` — `pub mod sky`, sky_renderer field, Drop cleanup
- `src/app.rs` — Sky initialization, day-night tick, 3 new egui panels
- `shaders/common.glsl` — apply_distance_fog() with 4 fog types
- `shaders/meshlet_draw.frag` — Distance fog post-lighting
- `shaders/chunk_mesh.frag` — Distance fog post-lighting (legacy path)
- `build.rs` — Shader sources expanded to 16 (sky.vert + sky.frag)
- `tests/phase3_meshing.rs` — Updated submit_frame_sequence assertion

## Decisions Made
- LGHT-05-01: Sky renders BEFORE geometry at depth=1.0 with LESS_OR_EQUAL depth test, no depth write
- LGHT-05-02: Sky params SSBO (binding 23) carries inv_view_proj for fragment shader ray reconstruction
- LGHT-05-03: DayNightCycle auto-updates sun direction/color/ambient when use_day_night_cycle=true
- LGHT-05-04: Fog color tracks sky horizon through day-night cycle for seamless blending
- LGHT-05-05: Clear color dynamically derived from fog_color * 0.8 to match sky
- LGHT-05-06: Cycle starts paused by default; day_speed=600s (10-minute game day)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Test assertion mismatch for submit_frame_sequence**
- **Found during:** Task 3 (Sky render pass integration)
- **Issue:** phase3_meshing test expected old 12-element sequence without sky_draw or ssao_compute
- **Fix:** Updated both submit_frame_sequence() and test assertion to include sky_draw + ssao_compute
- **Files modified:** src/renderer/submit.rs, tests/phase3_meshing.rs
- **Verification:** cargo test passes (all 13 tests pass)
- **Committed in:** ea7b198 (Task 3 commit)

**2. [Rule 3 - Blocking] PipelineCache field access**
- **Found during:** Task 1 (SkyRenderer::new)
- **Issue:** PipelineCache uses private `handle` field, not public `cache`
- **Fix:** Changed `pc.cache` to `pc.handle()` accessor method
- **Files modified:** src/renderer/sky.rs
- **Verification:** cargo build succeeds
- **Committed in:** 044ebca (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes required for compilation/test correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 07 (Lighting and Shadows) is now COMPLETE with all 5 plans executed
- All lighting requirements resolved: PBR (LGHT-01), CSM shadows (LGHT-02), SSAO (LGHT-03), voxel AO (LGHT-04), sky/atmosphere/fog (LGHT-05)
- Ready for Phase 8 or next milestone phase

---
*Phase: 07-lighting-and-shadows*
*Completed: 2026-03-29*
