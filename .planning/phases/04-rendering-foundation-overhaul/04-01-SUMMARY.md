---
phase: 04-rendering-foundation-overhaul
plan: 01
subsystem: runtime
tags: [dependency-injection, oncelock, app-struct, env_logger, error-propagation]

requires:
  - phase: 03-greedy-meshing-and-render-delta-sync
    provides: OnceLock-based renderer/scheduler globals that this plan eliminates

provides:
  - App struct owning Renderer, StreamingState, MeshingState directly
  - run_frame accepting explicit &mut references instead of locking globals
  - env_logger initialization for runtime diagnostics
  - submit_frame error propagation (logged, not silently discarded)

affects: [04-02, 04-03, 04-04, 04-05, 04-06, 04-07]

tech-stack:
  added: []
  patterns: [dependency-injection, explicit-ownership]

key-files:
  created:
    - tests/phase4_rendering.rs
  modified:
    - src/app.rs
    - src/renderer/mod.rs
    - src/runtime/scheduler.rs
    - src/runtime/mod.rs
    - src/main.rs
    - tests/phase1_stage_order.rs
    - tests/phase1_observability.rs
    - tests/phase1_events.rs
    - tests/phase2_streaming.rs

key-decisions:
  - "App struct owns Renderer, StreamingState, MeshingState as direct fields — no trait objects, no Arc, no OnceLock"
  - "run_frame accepts Option<&mut Renderer> to allow tests to pass None (no Vulkan context needed)"
  - "Renderer drain+submit happens in app event loop, not inside scheduler RenderSubmit arm"
  - "drain_pending_render_deltas_into_renderer made pub for app.rs to call directly"

patterns-established:
  - "Dependency injection: all subsystems are passed as explicit &mut references"
  - "Tests construct local StreamingState::new() and MeshingState::default() — no global state dependency"

requirements-completed: [REND-06]

duration: 6min
completed: 2026-03-25
---

# Phase 4 Plan 01: Infrastructure Fixes + DI Refactor Summary

**Eliminated all OnceLock<Mutex<>> global singletons and replaced with App struct dependency injection; added env_logger and submit_frame error propagation**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-25T05:21:36Z
- **Completed:** 2026-03-25T11:03:29Z
- **Tasks:** 3
- **Files modified:** 11

## Accomplishments
- Deleted `src/renderer/globals.rs` and all OnceLock-based Renderer global
- Removed static STREAMING/MESHING OnceLock from scheduler; all state passed as &mut refs
- Created `App` struct in `src/app.rs` owning all three subsystems directly
- Added `env_logger::init()` to main() for visible RUST_LOG output
- submit_frame errors caught with `log::error!` in event loop, not silently discarded
- Updated all existing tests (Phase 1/2/3) to construct local state — no global dependency

## Task Commits

Each task was committed atomically:

1. **Task 1: Create App struct and migrate Renderer ownership from globals** - `00c565b` (feat)
2. **Task 2: Migrate StreamingState and MeshingState out of scheduler globals** - `c197737` (feat)
3. **Task 3: Add env_logger, error propagation, and full verification** - `29c8300` (feat)

## Files Created/Modified
- `src/app.rs` - App struct owning Renderer + StreamingState + MeshingState, driving event loop
- `src/renderer/globals.rs` - DELETED (OnceLock<Mutex<Renderer>> singleton)
- `src/renderer/mod.rs` - Removed `pub mod globals` and re-exports
- `src/runtime/scheduler.rs` - Pure logic with explicit &mut parameters, no global state
- `src/runtime/mod.rs` - Export StreamingState from scheduler
- `src/main.rs` - env_logger::init() before app::run()
- `tests/phase4_rendering.rs` - 9 tests verifying OnceLock elimination and DI
- `tests/phase1_stage_order.rs` - Updated to construct local state
- `tests/phase1_observability.rs` - Updated to construct local state
- `tests/phase1_events.rs` - Updated to construct local state
- `tests/phase2_streaming.rs` - Updated to construct local state

## Decisions Made
- Used `Option<&mut Renderer>` in run_frame signature to allow tests to pass None (avoids requiring Vulkan context in test)
- Renderer drain+submit moved to app event loop rather than scheduler RenderSubmit arm — cleaner ownership, no double-locking
- drain_pending_render_deltas_into_renderer made pub so app.rs can call it directly

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] run_frame needs Option<&mut Renderer> for test compatibility**
- **Found during:** Task 2 (migrating to &mut Renderer parameter)
- **Issue:** Existing tests (phase1_stage_order, phase1_observability, phase1_events, phase2_streaming) call run_frame but cannot construct a real Renderer (requires Vulkan context)
- **Fix:** Changed renderer parameter to `Option<&mut Renderer>`. Tests pass None; real app passes Some.
- **Files modified:** src/runtime/scheduler.rs, all test files
- **Verification:** All 50+ tests pass
- **Committed in:** c197737 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary for test compatibility. The Option wrapping is minimal overhead and will be refined in later plans when tests may construct lightweight renderer stubs.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Clean ownership model established — App struct owns all subsystems
- Ready for Plan 04-02 (real camera system + push constants)
- All subsequent Phase 4 plans can safely take &mut Renderer from App

---
*Phase: 04-rendering-foundation-overhaul*
*Completed: 2026-03-25*
