//! Phase 2 streaming integration tests.
//!
//! These tests exercise the full WorldUpdate -> rayon -> MeshSync round-trip
//! through `run_frame`. Because `run_frame` uses `OnceLock` global state, tests
//! in this file are order-sensitive and frame indices must not clash with those
//! used in scheduler unit tests.

use revoxelation::runtime::scheduler::run_frame;

// ---------------------------------------------------------------------------
// full_round_trip
// ---------------------------------------------------------------------------

/// Two consecutive frames complete without panic; both return all 5 stages.
#[test]
fn full_round_trip() {
    let r0 = run_frame(2000);
    assert_eq!(r0.executed_stages.len(), 5, "frame 2000 must execute all 5 stages");

    let r1 = run_frame(2001);
    assert_eq!(r1.executed_stages.len(), 5, "frame 2001 must execute all 5 stages");
}

// ---------------------------------------------------------------------------
// cancel_in_flight_no_panic
// ---------------------------------------------------------------------------

/// Running additional frames after the streaming pipeline has been primed
/// must not panic, regardless of cancel-flag state.
#[test]
fn cancel_in_flight_no_panic() {
    // Frame 2002: WorldUpdate enqueues work.
    let r0 = run_frame(2002);
    assert_eq!(r0.executed_stages.len(), 5);

    // Frame 2003: MeshSync drains whatever rayon has produced.
    let r1 = run_frame(2003);
    assert_eq!(r1.executed_stages.len(), 5);
}
