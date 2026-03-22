---
phase: 03-greedy-meshing-and-render-delta-sync
plan: 02
subsystem: rendering
tags: [vulkan, indirect-draw, chunk-pool, shaderc, winit]
requires:
  - phase: 03-greedy-meshing-and-render-delta-sync
    provides: PackedMesh outputs, meshing dirty queue, and scheduler MeshSync handoff from 03-01
provides:
  - fixed slot-pool contracts and shared chunk buffers for indirect draw
  - scheduler-to-renderer chunk delta sync for upsert/remove flows
  - build-time shader compilation and visible winit app bootstrap for Vulkan rendering
affects: [phase-04, phase-05, renderer, app-bootstrap]
tech-stack:
  added: [shaderc]
  patterns: [stable chunk slot pool, render delta queue, build-time shader compilation, single indirect draw path]
key-files:
  created:
    - build.rs
    - shaders/chunk_mesh.vert
    - shaders/chunk_mesh.frag
    - shaders/chunk_cull.comp
    - src/app.rs
    - src/renderer/cull_pipeline.rs
    - src/renderer/mesh_pipeline.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/lib.rs
    - src/main.rs
    - src/renderer/chunk_pool.rs
    - src/renderer/device.rs
    - src/renderer/mod.rs
    - src/runtime/scheduler.rs
    - tests/phase3_meshing.rs
key-decisions:
  - "Required samplerAnisotropy, multiDrawIndirect, and drawIndirectFirstInstance at device selection time and fail fast instead of introducing a per-draw fallback path."
  - "Kept one shared slot allocator plus one indirect command per active chunk so remeshes and unloads only touch the affected slot ranges."
patterns-established:
  - "MeshSync emits RenderDelta::Upsert/Remove and RenderSubmit drains that queue into renderer-owned state before submit_frame."
  - "Shader sources live under shaders/ and compile through build.rs into OUT_DIR SPIR-V artifacts consumed by renderer pipeline modules."
requirements-completed: [MESH-01, MESH-03]
duration: 24 min
completed: 2026-03-22
---

# Phase 03 Plan 02: Render Delta Sync Summary

**Fixed chunk slot-pool renderer, scheduler-driven delta sync, shaderc build pipeline, and visible Vulkan app bootstrap through a single indirect draw path**

## Performance

- **Duration:** 24 min
- **Started:** 2026-03-22T02:45:00+08:00
- **Completed:** 2026-03-22T03:07:55+08:00
- **Tasks:** 3
- **Files modified:** 15

## Accomplishments
- Added `ChunkPool`, feature-gated Vulkan device selection, and pure slot-shadow tests so chunk renderer state is tracked per slot rather than by whole-world buffer rebuilds.
- Wired `RenderDelta::Upsert/Remove` from scheduler meshing results and unload events into renderer-owned pending delta state, keeping updates scoped to changed chunks.
- Added build-time GLSL compilation, chunk graphics/compute pipeline scaffolding, and a real `winit` app bootstrap that installs the renderer and drives `run_frame()` on redraw.

## Task Commits

Each task was committed atomically where the runtime allowed:

1. **Task 1: Add the feature gate and fixed slot-pool contracts** - `c5dfbba` (feat)
2. **Task 2: Implement chunk-delta upload wiring and scheduler-to-renderer sync** - `c9b3f9a` (mixed refactor/feat commit)
3. **Task 3: Add shader build, indirect draw pipelines, and the visible app bootstrap** - `575ecf7` (feat)

## Files Created/Modified
- `Cargo.toml` - enables the build script and adds `shaderc` for GLSL -> SPIR-V compilation.
- `Cargo.lock` - captures the shaderc dependency graph used by the build pipeline.
- `build.rs` - recompiles the chunk mesh/cull shaders into `OUT_DIR`.
- `shaders/chunk_mesh.vert` - decodes packed vertices and maps chunk-local geometry into clip space.
- `shaders/chunk_mesh.frag` - emits a block-id-derived debug color for chunk surfaces.
- `shaders/chunk_cull.comp` - establishes the compute pipeline stage used before indirect drawing.
- `src/app.rs` - builds the `winit` window, initializes Vulkan renderer state, and drives redraw-based frame execution.
- `src/main.rs` - reduces startup to error-handled `app::run()` bootstrap.
- `src/renderer/chunk_pool.rs` - owns slot allocation, host-visible shared buffers, shadow metadata, and indirect command preparation.
- `src/renderer/device.rs` - enforces the required indirect-draw Vulkan feature gate.
- `src/renderer/mod.rs` - exports render delta contracts, manages chunk pool/pipeline lifetime, and sequences upload -> cull -> barrier -> indirect draw -> egui.
- `src/renderer/mesh_pipeline.rs` - creates the chunk graphics pipeline and issues `cmd_draw_indexed_indirect`.
- `src/renderer/cull_pipeline.rs` - creates the compute pipeline and dispatch path used before drawing.
- `src/runtime/scheduler.rs` - queues render deltas during MeshSync and unload handling.
- `tests/phase3_meshing.rs` - adds MESH-03 contract coverage for slot reuse, remove deltas, indirect submit ordering, and build-script tracking.

## Decisions Made
- Chose fail-fast device selection for unsupported hardware rather than adding a fallback that would violate the plan's single indirect draw requirement.
- Preserved stable chunk slot identity across remeshes so only changed chunk ranges and one indirect command entry need updates.

## Deviations from Plan

### Auto-fixed Issues

**1. [Workflow - Mixed Commit] Task 2 landed inside a broader refactor commit**
- **Found during:** Task 2 (Implement chunk-delta upload wiring and scheduler-to-renderer sync)
- **Issue:** Commit `c9b3f9a` unexpectedly bundled the intended Task 2 scheduler/render-delta work with unrelated formatting and pre-existing context files already in the worktree.
- **Fix:** Continued from the resulting `HEAD` without reverting unrelated files, then isolated all subsequent renderer/bootstrap work and documentation in dedicated commits.
- **Files modified:** `src/runtime/scheduler.rs`, `src/renderer/mod.rs`, `tests/phase3_meshing.rs`, plus unrelated pre-existing formatting/context files outside this plan's scope.
- **Verification:** `cargo test --test phase3_meshing`, `cargo test`
- **Committed in:** `c9b3f9a`

---

**Total deviations:** 1 auto-fixed (workflow/commit-boundary)
**Impact on plan:** Code behavior and verification are correct, but Task 2 commit hygiene is noisier than the planned one-task-one-commit boundary.

## Issues Encountered

- The `gsd-executor` runtime did not make meaningful progress on `03-02`, so the orchestrator took over local execution and preserved the phase plan's task boundaries manually.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 3 now has a single indirect draw submission path, build-time shader compilation, and a visible app bootstrap that later gameplay phases can target.
- Manual verification of the live window path (`cargo run`) was not exercised in this session, so the verifier may still require a human visual pass even though automated checks are green.

---
*Phase: 03-greedy-meshing-and-render-delta-sync*
*Completed: 2026-03-22*
