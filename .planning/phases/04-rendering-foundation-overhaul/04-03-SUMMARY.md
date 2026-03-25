---
phase: 04-rendering-foundation-overhaul
plan: 03
subsystem: renderer
tags: [vulkan, swapchain, resize, ash, vk]

requires:
  - phase: 04-rendering-foundation-overhaul/04-02
    provides: "Dynamic viewport/scissor pipelines that survive swapchain recreation"
provides:
  - "recreate_swapchain_context() with old_swapchain driver optimization"
  - "FrameOutcome enum for submit_frame signaling NeedsRecreate"
  - "WindowEvent::Resized handling with needs_resize flag"
  - "Minimization (0x0 extent) graceful skip"
affects: [04-06, 04-07, 05-01]

tech-stack:
  added: []
  patterns: ["FrameOutcome return enum for swapchain lifecycle signaling"]

key-files:
  created: []
  modified:
    - "src/renderer/submit.rs"
    - "src/renderer/mod.rs"
    - "src/app.rs"
    - "tests/phase25_vulkan.rs"

key-decisions:
  - "D-05: ERROR_OUT_OF_DATE_KHR from acquire_next_image skips frame and returns NeedsRecreate"
  - "D-06: SUBOPTIMAL/OUT_OF_DATE from queue_present triggers recreate after present completes"
  - "D-07: Window extent 0x0 (minimized) skips rendering entirely"
  - "D-08: needs_resize flag + window_extent stored in App; recreation happens before next acquire"

patterns-established:
  - "FrameOutcome enum: submit_frame returns Submitted or NeedsRecreate instead of plain Result<()>"
  - "Resize-before-render: swapchain recreation happens at start of RedrawRequested, not during Resized event"

requirements-completed: [REND-02]

duration: 10min
completed: 2026-03-25
---

# Phase 4 Plan 03: Swapchain Lifecycle Management Summary

**Robust swapchain recreation on resize/OUT_OF_DATE/SUBOPTIMAL with graceful minimization skip via FrameOutcome enum**

## Performance

- **Duration:** 10 min
- **Started:** 2026-03-25T12:42:46Z
- **Completed:** 2026-03-25T12:52:26Z
- **Tasks:** 2 (Task 1 was already done from prior commit a5dd39f)
- **Files modified:** 4

## Accomplishments
- submit_frame now returns `FrameOutcome` enum (Submitted | NeedsRecreate) instead of `Result<()>`
- acquire_next_image catches `ERROR_OUT_OF_DATE_KHR` and skips the frame with NeedsRecreate signal
- queue_present catches `SUBOPTIMAL` (true) and `ERROR_OUT_OF_DATE_KHR` after present completes
- App handles `WindowEvent::Resized` by storing new extent and flagging `needs_resize`
- Before rendering: skip if extent is 0x0 (minimized); recreate swapchain if flagged
- All 6 rend_02 tests pass; all 37 phase4_rendering tests pass; full test suite passes

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement swapchain recreation function** - `a5dd39f` (feat) — already existed from prior work
2. **Task 2: Handle resize events and OUT_OF_DATE/SUBOPTIMAL in submit_frame** - `3b6cc83` (feat)

## Files Created/Modified
- `src/renderer/submit.rs` - Added FrameOutcome enum, OUT_OF_DATE/SUBOPTIMAL handling in acquire and present
- `src/renderer/mod.rs` - Re-exported FrameOutcome
- `src/app.rs` - Added needs_resize/window_extent fields, Resized handler, minimization skip, FrameOutcome match
- `tests/phase25_vulkan.rs` - Updated submit_frame type signature test to match new FrameOutcome return

## Decisions Made
- FrameOutcome enum chosen over Result<NeedsResize> error variant — cleaner separation of "swapchain stale" (not an error) from real errors
- Resize-before-render pattern: swapchain recreation at start of RedrawRequested, not during Resized event handler — avoids recreating multiple times during drag-resize

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed phase25_vulkan test type mismatch**
- **Found during:** Task 2 (submit_frame return type change)
- **Issue:** Existing test `submit_frame_fn_exists` expected `Result<()>` but new signature returns `Result<FrameOutcome>`
- **Fix:** Updated type annotation in test to match new `FrameOutcome` return
- **Files modified:** tests/phase25_vulkan.rs
- **Verification:** cargo test passes
- **Committed in:** 3b6cc83 (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary fix for existing test compatibility. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- REND-02 satisfied: swapchain lifecycle is fully managed
- Ready for Plan 04-06 (Hi-Z occlusion culling) or Plan 04-07 (pipeline cache + performance counters)
- recreate_swapchain_context is available for any future resize/VR paths

---
*Phase: 04-rendering-foundation-overhaul*
*Completed: 2026-03-25*
