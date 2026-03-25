//! Phase 2 streaming integration tests.
//!
//! These tests exercise the full WorldUpdate -> rayon -> MeshSync round-trip
//! through `run_frame`. Tests construct local state directly (no OnceLock globals).

use revoxelation::meshing::MeshingState;
use revoxelation::runtime::scheduler::{StreamingState, run_frame};

// ---------------------------------------------------------------------------
// full_round_trip
// ---------------------------------------------------------------------------

/// Two consecutive frames complete without panic; both return all 5 stages.
#[test]
fn full_round_trip() {
    let mut streaming = StreamingState::new();
    let mut meshing = MeshingState::default();

    let r0 = run_frame(&mut streaming, &mut meshing, None, 2000);
    assert_eq!(
        r0.executed_stages.len(),
        5,
        "frame 2000 must execute all 5 stages"
    );

    let r1 = run_frame(&mut streaming, &mut meshing, None, 2001);
    assert_eq!(
        r1.executed_stages.len(),
        5,
        "frame 2001 must execute all 5 stages"
    );
}

// ---------------------------------------------------------------------------
// cancel_in_flight_no_panic
// ---------------------------------------------------------------------------

/// Running additional frames after the streaming pipeline has been primed
/// must not panic, regardless of cancel-flag state.
#[test]
fn cancel_in_flight_no_panic() {
    let mut streaming = StreamingState::new();
    let mut meshing = MeshingState::default();

    // Frame 2002: WorldUpdate enqueues work.
    let r0 = run_frame(&mut streaming, &mut meshing, None, 2002);
    assert_eq!(r0.executed_stages.len(), 5);

    // Frame 2003: MeshSync drains whatever rayon has produced.
    let r1 = run_frame(&mut streaming, &mut meshing, None, 2003);
    assert_eq!(r1.executed_stages.len(), 5);
}
