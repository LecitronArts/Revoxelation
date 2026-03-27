use revoxelation::meshing::MeshingState;
use revoxelation::runtime::{STAGE_ORDER, Stage, StreamingState, run_frame};

#[test]
fn stage_order_locked_to_input_sim_world_meshsync_render() {
    let expected = [
        Stage::Input,
        Stage::Simulation,
        Stage::WorldUpdate,
        Stage::MeshSync,
        Stage::RenderSubmit,
    ];

    assert_eq!(STAGE_ORDER, expected, "canonical stage order changed");

    let mut streaming = StreamingState::new();
    let mut meshing = MeshingState::default();
    let frame = run_frame(&mut streaming, &mut meshing, None, 7, [0.0, 0.0, 0.0], 720.0, std::f32::consts::FRAC_PI_3);
    assert_eq!(frame.frame_index, 7);
    assert_eq!(
        frame.executed_stages, expected,
        "scheduler must execute stages in canonical order"
    );

    for stage in expected {
        let seen = frame
            .executed_stages
            .iter()
            .filter(|candidate| **candidate == stage)
            .count();
        assert_eq!(seen, 1, "{stage:?} should execute exactly once");
    }
}
