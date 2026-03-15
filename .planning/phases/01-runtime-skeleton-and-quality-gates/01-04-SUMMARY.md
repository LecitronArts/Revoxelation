---
phase: 01-runtime-skeleton-and-quality-gates
plan: 04
subsystem: runtime
tags: [rust, ecs, serde, events, scheduler]
requires:
  - phase: 01-runtime-skeleton-and-quality-gates/01-01
    provides: deterministic stage order and frame execution scaffold
  - phase: 01-runtime-skeleton-and-quality-gates/01-03
    provides: boundary-safe runtime domain contracts
provides:
  - Serializable command intent models for player actions, chunk lifecycle, and block edits
  - Serializable event fact models with command acceptance and rejection outcomes
  - Deterministic monotonic sequence metadata and one-frame event bus stage integration
affects: [phase-02-world-streaming, phase-07-network-interfaces, observability]
tech-stack:
  added: [serde, serde_json]
  patterns: [intent-vs-fact event contracts, deterministic sequence envelopes, explicit validation rejection reasons]
key-files:
  created: [src/runtime/events/command.rs, src/runtime/events/event.rs, src/runtime/events/sequence.rs, src/runtime/events/validation.rs, src/runtime/events/bus.rs]
  modified: [Cargo.toml, src/runtime/mod.rs, src/runtime/scheduler.rs, src/runtime/events/mod.rs, tests/phase1_events.rs]
key-decisions:
  - "CommandOutcome events are emitted for every command to keep acceptance and rejection paths observable."
  - "Monotonic event sequence numbers are frame-indexed with a fixed stride to preserve deterministic replay ordering."
  - "Scheduler integrates event bus flow only at stage boundaries (Input publish, Simulation process, RenderSubmit consume)."
patterns-established:
  - "Command/Event separation: commands express intent, events express runtime facts."
  - "Validation-first processing: every command is validated before any domain event emission."
requirements-completed: [ECS-03]
duration: 4min
completed: 2026-03-15
---

# Phase 01 Plan 04: Runtime Event Contracts Summary

**Serializable runtime command/event contracts with deterministic sequencing, explicit rejection outcomes, and one-frame scheduler bus integration for ECS-03.**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-15T05:58:26Z
- **Completed:** 2026-03-15T06:02:51Z
- **Tasks:** 3
- **Files modified:** 10

## Accomplishments
- Implemented serde-backed command and event envelopes across player action, chunk lifecycle, and block edit families.
- Added deterministic validation behavior with explicit `RejectionReason` payloads and outcome events for observability.
- Integrated event bus publish/process/consume flow into scheduler stage boundaries with monotonic one-frame integration coverage.

## Task Commits

Each task was committed atomically:

1. **Task 1: Define serializable command/event schemas and deterministic sequence metadata** - `6158157` (feat)
2. **Task 2: Implement event validation and explicit rejection reasons** - `60bbff1` (feat)
3. **Task 3: Integrate one-frame emit/consume flow at scheduler stage boundaries** - `66e7df5` (feat)

**Plan metadata:** pending final docs commit

## Files Created/Modified
- `src/runtime/events/command.rs` - Serializable command intent models for three ECS-03 families.
- `src/runtime/events/event.rs` - Serializable event fact models plus command outcome payloads.
- `src/runtime/events/sequence.rs` - Frame-indexed monotonic sequence metadata utilities.
- `src/runtime/events/validation.rs` - Deterministic command validation rules and explicit rejection reasons.
- `src/runtime/events/bus.rs` - Event bus command processing, event emission, and consume snapshot API.
- `src/runtime/scheduler.rs` - Stage-boundary event bus wiring in `run_frame`.
- `tests/phase1_events.rs` - serde roundtrip, rejection-path, and one-frame monotonic selector tests.

## Decisions Made
- Chose fixed event ordering semantics where each command emits `CommandOutcome` before any accepted domain event.
- Used frame-based monotonic sequence stride (`FRAME_SEQUENCE_STRIDE`) to keep ordering deterministic and replay-oriented.
- Preserved wave0 selector coverage by keeping a `wave0_` bootstrap selector while replacing event fixture assertions with concrete model tests.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added serde dependencies required for contract serialization**
- **Found during:** Task 1 (schema implementation)
- **Issue:** Project had no serde crates; command/event contracts could not derive Serialize/Deserialize.
- **Fix:** Added `serde` dependency and `serde_json` dev-dependency in `Cargo.toml`.
- **Files modified:** Cargo.toml
- **Verification:** `cargo test --quiet event_serde_roundtrip_models -- --nocapture`
- **Committed in:** 6158157

**2. [Rule 3 - Blocking] Exported runtime events module for integration-test access**
- **Found during:** Task 1 (test implementation)
- **Issue:** `tests/phase1_events.rs` could not import new event modules without a public `runtime::events` module export.
- **Fix:** Added `pub mod events;` in `src/runtime/mod.rs`.
- **Files modified:** src/runtime/mod.rs
- **Verification:** `cargo test --quiet event_serde_roundtrip_models -- --nocapture`
- **Committed in:** 6158157

---

**Total deviations:** 2 auto-fixed (2 Rule 3 blocking)
**Impact on plan:** Both deviations were required to compile and verify ECS-03 behavior; no scope creep.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Runtime now provides deterministic, serializable event contracts suitable for world-streaming and replay-sensitive phases.
- No blockers found for advancing to Plan 01-05.

---
*Phase: 01-runtime-skeleton-and-quality-gates*
*Completed: 2026-03-15*

## Self-Check: PASSED
- Summary file exists and all task commit hashes were verified in git history.

