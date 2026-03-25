---
phase: 04-rendering-foundation-overhaul
plan: 02
subsystem: renderer
tags: [camera, push-constants, vulkan, glam, fps-camera, dynamic-viewport]

# Dependency graph
requires:
  - phase: 04-rendering-foundation-overhaul/01
    provides: "App struct DI, OnceLock elimination, Renderer ownership"
provides:
  - "FpsCamera with WASD+mouse navigation and perspective projection"
  - "CameraUniforms push constant struct (80 bytes) for vertex shader"
  - "Dynamic viewport/scissor pipeline (resolution-independent rendering)"
  - "Real view_proj matrix replacing debug_project() in vertex shader"
affects: [04-rendering-foundation-overhaul, 05-bindless-architecture]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Push constants for per-frame camera data", "Dynamic viewport/scissor state", "FPS camera with yaw/pitch from mouse delta"]

key-files:
  created:
    - src/renderer/camera.rs
  modified:
    - src/renderer/mod.rs
    - src/renderer/mesh_pipeline.rs
    - src/renderer/submit.rs
    - src/app.rs
    - shaders/chunk_mesh.vert
    - tests/phase4_rendering.rs

key-decisions:
  - "CameraUniforms is 80 bytes: Mat4 view_proj + Vec3 camera_pos + f32 pad — fits in 128-byte push constant minimum"
  - "Dynamic viewport/scissor replaces baked pipeline viewport — enables resolution-independent rendering"
  - "Push constants bound to VERTEX stage only since fragment shader doesn't need camera data"
  - "FpsCamera defaults to position (32, 48, -60) facing toward world center"
  - "Pitch clamped to ±89 degrees to prevent gimbal lock"

patterns-established:
  - "Push constants pattern: CameraUniforms struct → bytemuck::bytes_of → cmd_push_constants"
  - "Dynamic state pattern: DynamicState::VIEWPORT + SCISSOR in pipeline, set per-frame before draw"
  - "Input forwarding pattern: DeviceEvent::MouseMotion → camera, KeyboardInput → key state tracking → per-frame camera update"

requirements-completed: [REND-01]

# Metrics
duration: 15min
completed: 2026-03-25
---

# Phase 4 Plan 02: FPS Camera and Push Constants Summary

**FPS camera with WASD+mouse navigation, push constant view_proj delivery, and dynamic viewport/scissor replacing hardcoded debug_project()**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-25T11:12:47Z
- **Completed:** 2026-03-25T11:27:59Z
- **Tasks:** 3
- **Files modified:** 7

## Accomplishments
- Implemented FpsCamera with position/yaw/pitch, perspective projection via glam, and WASD+mouse input handling
- Replaced debug_project() in vertex shader with push_constant view_proj matrix delivery
- Switched mesh pipeline to dynamic viewport/scissor state for resolution-independent rendering
- Wired keyboard and mouse events through App event loop to camera for real-time navigation

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement FpsCamera and CameraUniforms** - `1d793d9` (feat — TDD: RED+GREEN combined)
2. **Task 2: Wire push constants and dynamic viewport into mesh pipeline** - `a2890b8` (feat — TDD: RED+GREEN combined)
3. **Task 3: Wire input events to camera and verify real-time navigation** - `be236d9` (feat)

## Files Created/Modified
- `src/renderer/camera.rs` - FpsCamera struct with view_proj(), process_keyboard(), process_mouse(); CameraUniforms #[repr(C)] push constant struct (80 bytes)
- `src/renderer/mod.rs` - Added `pub mod camera`
- `shaders/chunk_mesh.vert` - Replaced debug_project() with push_constant CameraUniforms block; gl_Position = camera.view_proj * vec4(world_position, 1.0)
- `src/renderer/mesh_pipeline.rs` - Added push constant range (80 bytes, VERTEX stage); dynamic viewport/scissor state; cmd_set_viewport/scissor/push_constants in draw()
- `src/renderer/submit.rs` - submit_frame now accepts CameraUniforms; passes to mesh_pipeline.draw()
- `src/app.rs` - Added FpsCamera field, KeysPressed state, delta time tracking; DeviceEvent::MouseMotion → camera; WASD/Space/LShift key state → per-frame camera update
- `tests/phase4_rendering.rs` - 7 new tests: 4 camera unit tests + 3 source-grep verification tests

## Decisions Made
- CameraUniforms is 80 bytes (Mat4 + Vec3 + pad) fitting in 128-byte push constant minimum
- Dynamic viewport/scissor via VK_DYNAMIC_STATE — pipeline no longer bakes resolution
- Push constants bound to VERTEX stage only (fragment doesn't need camera data)
- Camera starts at (32, 48, -60) for good initial world view
- Pitch clamped to ±89° to prevent gimbal lock

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- FPS camera and perspective projection operational, ready for Plan 04-03 (frustum culling / Hi-Z)
- Dynamic viewport/scissor foundation enables future resolution changes without pipeline recreation

---
*Phase: 04-rendering-foundation-overhaul*
*Completed: 2026-03-25*
