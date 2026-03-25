---
phase: 04-rendering-foundation-overhaul
plan: 05
subsystem: renderer
tags: [vulkan, compute-shader, frustum-culling, AABB, indirect-draw, gpu-driven]

# Dependency graph
requires:
  - phase: 04-02
    provides: FpsCamera, CameraUniforms, view_proj matrix
  - phase: 04-04
    provides: GpuOnly memory model, StagingRing, create_allocated_buffer
provides:
  - Real 6-plane frustum culling via compute shader (AABB P-vertex method)
  - FrustumPlanes struct (96 bytes) with Gribb-Hartmann extraction
  - Frustum planes SSBO (binding 4) uploaded per-frame
  - Draw count buffer (binding 5) with atomicAdd compaction
  - Compacted dense indirect output for visible-only chunks
  - Workgroup size 64 for proper GPU occupancy
affects: [04-06, 05-05, 06-02]

# Tech tracking
tech-stack:
  added: []
  patterns: [P-vertex AABB frustum test, Gribb-Hartmann plane extraction, atomicAdd compaction, vkCmdFillBuffer reset]

key-files:
  created: []
  modified:
    - src/renderer/camera.rs
    - shaders/chunk_cull.comp
    - src/renderer/cull_pipeline.rs
    - src/renderer/submit.rs
    - src/app.rs
    - tests/phase4_rendering.rs

key-decisions:
  - "Frustum planes via SSBO (96 bytes) not push constants — combined with camera push constants exceeds 128B minimum"
  - "FrustumPlanes: 6 x [f32; 4] = 96 bytes, Gribb-Hartmann extraction with normalized normals"
  - "Cull shader workgroup: local_size_x=64 with ceil(count/64) dispatch groups"
  - "AABB vs frustum: P-vertex method — for each plane compute corner most in normal direction"
  - "Draw count buffer: single u32, GpuOnly with TRANSFER_DST, reset via vkCmdFillBuffer each frame"
  - "Visible chunks written to compacted output via atomicAdd as write index — dense command list for IndirectCount"
  - "Frustum planes buffer: CpuToGpu for mapped CPU writes, no staging needed"

patterns-established:
  - "P-vertex AABB frustum test: standard for all future GPU culling (Hi-Z, meshlet)"
  - "vkCmdFillBuffer + barrier pattern for resetting GPU counters each frame"
  - "atomicAdd compaction pattern for building dense draw lists from sparse input"

requirements-completed: [REND-03]

# Metrics
duration: 8min
completed: 2026-03-25
---

# Plan 04-05: Frustum Culling Summary

**GPU-driven frustum culling via compute shader with 6-plane AABB P-vertex test, atomicAdd-compacted dense indirect output, and per-frame draw count buffer**

## Performance

- **Duration:** 8 min
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- Real 6-plane frustum culling replaces stub visibility check in compute shader
- Chunks outside camera frustum produce zero draw commands (not just instanceCount=0)
- FrustumPlanes extracted from view_proj via Gribb-Hartmann method, normalized for correct distance
- Compacted output buffer via atomicAdd produces dense draw list for future IndirectCount
- Workgroup size upgraded from 1 to 64 for proper GPU occupancy

## Task Commits

Each task was committed atomically:

1. **Task 1: Frustum plane extraction** - `3bcee25` (feat) — committed in prior session
2. **Task 2: Cull shader + pipeline + submit integration** - `4990dcf` (feat)

## Files Created/Modified
- `src/renderer/camera.rs` - FrustumPlanes struct (96 bytes) + extract_frustum_planes (Gribb-Hartmann)
- `shaders/chunk_cull.comp` - Full rewrite: 6-plane AABB P-vertex test, atomicAdd compaction, local_size_x=64
- `src/renderer/cull_pipeline.rs` - Frustum planes SSBO (binding 4), draw count buffer (binding 5), vkCmdFillBuffer reset, push constants for active_draw_count
- `src/renderer/submit.rs` - Per-frame frustum extraction, dispatch with frustum planes, draw count barrier
- `src/app.rs` - Updated ChunkCullPipeline::new to take &mut Renderer
- `tests/phase4_rendering.rs` - 8 rend_03 tests covering plane extraction, inside/outside classification, shader content, pipeline bindings

## Decisions Made
- Frustum planes SSBO (not push constants) — 96 bytes exceeds what can be combined with camera push constants in 128B minimum
- CpuToGpu for frustum planes buffer — 96 bytes is tiny, mapped writes are simpler than staging
- GpuOnly + TRANSFER_DST for draw count buffer — reset via vkCmdFillBuffer avoids staging overhead
- Compacted output (atomicAdd) rather than in-place instanceCount=0 — prepares for vkCmdDrawIndexedIndirectCount

## Deviations from Plan

None - plan executed exactly as written. Task 1 was already completed in a prior session.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Frustum culling active; ready for Plan 04-06 (Hi-Z occlusion culling) which builds on the same cull shader
- Draw count buffer ready for vkCmdDrawIndexedIndirectCount when VK_KHR_draw_indirect_count is wired
- 3 failing tests from Plan 04-03 (swapchain recreation) remain — that plan is not yet fully executed

---
*Phase: 04-rendering-foundation-overhaul*
*Completed: 2026-03-25*
