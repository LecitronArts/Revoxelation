---
phase: 05-bindless-architecture-and-gpu-scene
plan: 02
subsystem: renderer
tags: [vulkan, bindless, descriptor-set, compute, graphics]

requires:
  - phase: 05-bindless-architecture-and-gpu-scene/01
    provides: "Vulkan 1.2 feature enforcement (descriptor indexing, drawIndirectCount)"
provides:
  - "BindlessTable managing global descriptor set 0 with UPDATE_AFTER_BIND + PARTIALLY_BOUND"
  - "Shared set 0 between cull_pipeline and mesh_pipeline — no per-pipeline descriptors"
  - "register_buffer/register_image API for dynamic descriptor updates"
affects: [05-03, 05-04, 05-05, 06, 07]

tech-stack:
  added: []
  patterns:
    - "Unified bindless descriptor set 0 shared by all pipelines"
    - "Pipeline constructors take bindless_layout parameter instead of creating own descriptors"
    - "Dispatch/draw methods take bindless_set parameter for binding"

key-files:
  created:
    - "src/renderer/bindless.rs"
  modified:
    - "src/renderer/cull_pipeline.rs"
    - "src/renderer/mesh_pipeline.rs"
    - "src/renderer/submit.rs"
    - "src/renderer/mod.rs"
    - "src/renderer/hot_reload.rs"
    - "src/app.rs"

key-decisions:
  - "BindlessTable owns set 0 with 10 bindings (0-7 active, 8-9 reserved for Plan 04)"
  - "Each pipeline keeps its own pipeline_layout with shared set_layout + own push constant range"
  - "Auxiliary buffers (frustum planes, draw count, Hi-Z config) stay in cull_pipeline but registered with BindlessTable"
  - "Bindless set bound at each cmd_bind_pipeline point (compute for cull, graphics for mesh)"

patterns-established:
  - "Bindless registration: BindlessTable created before pipelines, buffers registered at init"
  - "Pipeline layout sharing: pipelines take descriptor_set_layout as constructor parameter"

requirements-completed: [BIND-02]

duration: 15min
completed: 2026-03-26
---

# Phase 5 Plan 02: Bindless Set 0 Migration Summary

**BindlessTable with Vulkan 1.2 UPDATE_AFTER_BIND descriptor set 0 shared by cull and mesh pipelines, replacing per-pipeline descriptor infrastructure**

## Performance

- **Duration:** 15 min
- **Started:** 2026-03-26T04:13:42Z
- **Completed:** 2026-03-26T04:29:05Z
- **Tasks:** 2
- **Files modified:** 9

## Accomplishments
- Created BindlessTable struct managing global descriptor set 0 with 10 bindings, PARTIALLY_BOUND and UPDATE_AFTER_BIND flags
- Eliminated all per-pipeline descriptor pool/layout/set code from ChunkCullPipeline and ChunkMeshPipeline
- Both pipelines now share the bindless set 0 via constructor parameters and dispatch/draw method parameters
- Registered all GPU buffers (chunk pool metadata, indirect templates, draw slots, dense indirect, frustum planes, draw count, Hi-Z config) with BindlessTable at initialization

## Task Commits

Each task was committed atomically:

1. **Task 1: Create BindlessTable with set 0 descriptor infrastructure** - `cb3171f` (feat)
2. **Task 2: Migrate cull_pipeline and mesh_pipeline to shared set 0** - `f8888da` (feat)

## Files Created/Modified
- `src/renderer/bindless.rs` - New BindlessTable struct with descriptor set 0, register_buffer/register_image methods
- `src/renderer/cull_pipeline.rs` - Removed descriptor_pool/set_layout/set, constructor takes bindless_layout, dispatch takes bindless_set
- `src/renderer/mesh_pipeline.rs` - Removed descriptor_pool/set_layout/set, constructor takes bindless_layout, draw takes bindless_set
- `src/renderer/submit.rs` - Passes bindless.descriptor_set to both cull dispatch and mesh draw
- `src/renderer/mod.rs` - Added pub mod bindless, bindless field on Renderer, cleanup in Drop
- `src/renderer/hot_reload.rs` - Pass bindless_layout when recreating pipelines on hot-reload
- `src/app.rs` - BindlessTable creation and buffer registration before pipeline initialization
- `tests/phase5_bindless.rs` - Added source-grep tests for Task 1 and Task 2
- `tests/phase3_gap_closure.rs` - Fixed stale test referencing removed metadata_descriptor_layout_binding
- `tests/phase3_meshing.rs` - Fixed stale test referencing removed required_device_features_error
- `tests/phase4_rendering.rs` - Updated Hi-Z binding test to check bindless.rs instead of cull_pipeline.rs

## Decisions Made
- BindlessTable has 10 bindings: 0-7 for existing resources, 8-9 reserved (PARTIALLY_BOUND) for Plan 04 materials
- Each pipeline keeps its own pipeline_layout (push constant ranges differ) but shares the descriptor set layout
- Auxiliary buffers remain owned by ChunkCullPipeline for locality, registered with BindlessTable at construction
- Bindless descriptor set bound at each pipeline bind point (compute for cull, graphics for mesh)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed stale tests referencing removed APIs**
- **Found during:** Task 2 (pipeline migration)
- **Issue:** phase3_meshing.rs, phase3_gap_closure.rs, and phase4_rendering.rs referenced removed functions/bindings
- **Fix:** Updated tests to check source-grep patterns instead of calling removed APIs
- **Files modified:** tests/phase3_meshing.rs, tests/phase3_gap_closure.rs, tests/phase4_rendering.rs
- **Verification:** All 131 tests pass
- **Committed in:** f8888da (Task 2 commit)

**2. [Rule 3 - Blocking] Fixed hot_reload.rs calling old pipeline constructors**
- **Found during:** Task 2 (compile check)
- **Issue:** hot_reload.rs still called old 1-arg constructors for mesh and cull pipelines
- **Fix:** Updated to pass bindless_layout from renderer.bindless
- **Files modified:** src/renderer/hot_reload.rs
- **Verification:** cargo build succeeds
- **Committed in:** f8888da (Task 2 commit)

---

**Total deviations:** 2 auto-fixed (1 bug, 1 blocking)
**Impact on plan:** Both fixes necessary for correctness. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- BindlessTable ready for Plan 03 (scene buffer merge) and Plan 04 (materials + textures) to register additional buffers/textures at bindings 8-9
- Hi-Z pyramid image not yet registered with bindless set (binding 7) — will be done when Hi-Z is created during runtime initialization

---
*Phase: 05-bindless-architecture-and-gpu-scene*
*Completed: 2026-03-26*
