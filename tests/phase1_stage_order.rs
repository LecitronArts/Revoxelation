use revoxelation::runtime::{run_frame, Stage, STAGE_ORDER};

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

    let frame = run_frame(7);
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