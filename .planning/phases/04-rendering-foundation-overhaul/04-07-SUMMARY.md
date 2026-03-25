---
phase: 04-rendering-foundation-overhaul
plan: 07
subsystem: renderer
tags: [vulkan, pipeline-cache, perf-counters, egui, hot-reload, config, shaderc]

requires:
  - phase: 04-rendering-foundation-overhaul (plans 01-06)
    provides: "Vulkan renderer with DI, camera, swapchain, staging, frustum+Hi-Z culling"
provides:
  - "Persistent pipeline cache (cache/pipeline.bin) — faster subsequent startups"
  - "GpuPerfCounters struct tracking visible/total chunks, frame time"
  - "egui HUD overlay displaying GPU performance statistics"
  - "Shader hot-reload in debug builds (shaderc runtime recompilation)"
  - "RuntimeConfig loaded from optional config.toml"
affects: [05-bindless-architecture, 06-meshlet-pipeline, 07-lighting-shadows]

tech-stack:
  added: [toml, shaderc (runtime, optional)]
  patterns: [pipeline-cache-persistence, debug-only-hot-reload, runtime-config-file]

key-files:
  created:
    - src/renderer/pipeline_cache.rs
    - src/renderer/perf_counters.rs
    - src/renderer/hot_reload.rs
    - src/config.rs
  modified:
    - src/renderer/mod.rs
    - src/renderer/mesh_pipeline.rs
    - src/renderer/cull_pipeline.rs
    - src/renderer/hiz.rs
    - src/renderer/egui_backend.rs
    - src/app.rs
    - Cargo.toml
    - tests/phase4_rendering.rs

key-decisions:
  - "Pipeline cache stored at cache/pipeline.bin, auto-created directory"
  - "All create_*_pipelines calls use shared PipelineCache handle"
  - "Pipeline cache saved in Renderer::drop before pipeline destruction"
  - "GpuPerfCounters: visible_chunks, total_chunks, frame_time_ms, gpu_time_ms"
  - "Shader hot-reload: cfg(debug_assertions) + feature=hot-reload, polls every 60 frames"
  - "RuntimeConfig from config.toml: hiz_enabled, show_hud, camera_speed, camera_fov"
  - "shaderc added as optional runtime dep (hot-reload feature); toml for config parsing"

patterns-established:
  - "Pipeline cache pattern: load on init, pass to all pipeline creations, save on drop"
  - "Feature-gated debug tooling: #[cfg(all(debug_assertions, feature = \"hot-reload\"))]"
  - "Runtime config file pattern: config.toml with serde Deserialize defaults"

requirements-completed: [REND-07]

duration: 17min
completed: 2026-03-25
---

# Phase 4 Plan 07: Pipeline Cache, Perf Counters & Polish Summary

**Persistent pipeline cache with disk load/save, egui HUD performance overlay, shader hot-reload (debug), and runtime config from config.toml**

## Performance

- **Duration:** 17 min
- **Started:** 2026-03-25T13:13:17Z
- **Completed:** 2026-03-25T13:29:57Z
- **Tasks:** 3
- **Files modified:** 12

## Accomplishments
- Persistent Vulkan pipeline cache: loads from `cache/pipeline.bin` on startup, saves on shutdown — reducing pipeline creation time on subsequent runs
- GpuPerfCounters struct and egui HUD overlay showing chunk counts and frame time
- Shader hot-reload in debug builds: polls mtime every 60 frames, recompiles via shaderc, recreates pipelines
- RuntimeConfig loaded from optional `config.toml` with sensible defaults
- All 4 pipeline creation sites (mesh, cull, hiz, egui) use shared cache handle

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement persistent pipeline cache** - `368e73e` (feat)
2. **Task 2: Implement GPU performance counters and egui HUD display** - `6e50a57` (feat)
3. **Task 3: Shader hot-reload and runtime config (debug only)** - `24225af` (feat)

_TDD tasks 1-2 had RED tests added inline with GREEN implementation._

## Files Created/Modified
- `src/renderer/pipeline_cache.rs` - PipelineCache wrapper: load/save/handle/destroy
- `src/renderer/perf_counters.rs` - GpuPerfCounters struct with visible_chunks, total_chunks, frame_time_ms, gpu_time_ms
- `src/renderer/hot_reload.rs` - ShaderHotReload: mtime polling, shaderc recompilation, pipeline recreation
- `src/config.rs` - RuntimeConfig with serde Deserialize, loads from config.toml
- `src/renderer/mod.rs` - Added pipeline_cache, perf_counters, hot_reload modules; PipelineCache field in Renderer; save+destroy in Drop
- `src/renderer/mesh_pipeline.rs` - Uses shared pipeline cache handle instead of null
- `src/renderer/cull_pipeline.rs` - Uses shared pipeline cache handle instead of null
- `src/renderer/hiz.rs` - Uses shared pipeline cache handle instead of null
- `src/renderer/egui_backend.rs` - Uses shared pipeline cache handle instead of null
- `src/app.rs` - egui HUD shows counters; RuntimeConfig loaded; hot-reload wired in
- `Cargo.toml` - Added toml dep, shaderc as optional, hot-reload feature
- `tests/phase4_rendering.rs` - 4 new tests for rend_07

## Decisions Made
- Pipeline cache file path: `cache/pipeline.bin` — simple, out of source tree
- All pipeline creation uses shared cache handle (no PipelineCache::null())
- Hot-reload feature-gated: `#[cfg(all(debug_assertions, feature = "hot-reload"))]`
- shaderc::Compiler::new() returns Result in v0.10.1, handled with `.ok()`
- GpuPerfCounters visible_chunks currently approximated from active draw count (actual GPU readback deferred)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] shaderc::Compiler::new() API change**
- **Found during:** Task 3 (shader hot-reload)
- **Issue:** shaderc 0.10.1 returns `Result<Compiler, Error>` not `Option<Compiler>`
- **Fix:** Used `.ok()` to convert Result to Option
- **Files modified:** src/renderer/hot_reload.rs
- **Verification:** `cargo build` succeeds
- **Committed in:** 24225af (Task 3 commit)

**2. [Rule 3 - Blocking] Also updated hiz.rs and egui_backend.rs pipeline cache usage**
- **Found during:** Task 1 (pipeline cache)
- **Issue:** Test only checked mesh/cull pipelines, but hiz.rs and egui_backend.rs also used PipelineCache::null()
- **Fix:** Updated all 4 pipeline creation sites to use shared cache handle
- **Files modified:** src/renderer/hiz.rs, src/renderer/egui_backend.rs
- **Verification:** `grep PipelineCache::null src/renderer/` shows only fallback paths
- **Committed in:** 368e73e (Task 1 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
Phase 4 is complete — all 7 plans (REND-01 through REND-07) are implemented:
- REND-01: FPS camera with push constants and dynamic viewport
- REND-02: Swapchain lifecycle with resize/OUT_OF_DATE handling
- REND-03: GPU-driven frustum culling
- REND-04: Hi-Z occlusion culling
- REND-05: GpuOnly memory with staging ring
- REND-06: DI refactor (App struct ownership)
- REND-07: Pipeline cache, perf counters, HUD, hot-reload, config

Ready for Phase 5: Bindless Architecture & GPU Scene.

---
*Phase: 04-rendering-foundation-overhaul*
*Completed: 2026-03-25*
