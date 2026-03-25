use revoxelation::meshing::MeshingState;
use revoxelation::runtime::{RuntimeHudOverlay, STAGE_ORDER, Stage, StreamingState, TransitionKind, run_frame};

#[test]
fn structured_logs_include_frame_stage_event() {
    let frame_index = 42;
    let mut streaming = StreamingState::new();
    let mut meshing = MeshingState::default();
    let execution = run_frame(&mut streaming, &mut meshing, None, frame_index);

    assert_eq!(execution.frame_index, frame_index);
    assert_eq!(execution.executed_stages, STAGE_ORDER);
    assert_eq!(
        execution.trace_entries.len(),
        STAGE_ORDER.len() * 2,
        "each stage must emit begin and end trace entries"
    );

    for (stage_index, stage) in STAGE_ORDER.into_iter().enumerate() {
        let begin_index = stage_index * 2;
        let begin = &execution.trace_entries[begin_index];
        assert_eq!(begin.frame_index, frame_index);
        assert_eq!(begin.stage, stage);
        assert_eq!(begin.transition_kind, TransitionKind::Begin);
        assert_eq!(begin.sequence_index, begin_index);

        let begin_log = begin.to_structured_log();
        assert!(begin_log.contains(&format!("frame_index={frame_index}")));
        assert!(begin_log.contains(&format!("stage={}", stage.as_str())));
        assert!(begin_log.contains("transition=begin"));
        assert!(begin_log.contains(&format!("sequence={begin_index}")));

        let end_index = begin_index + 1;
        let end = &execution.trace_entries[end_index];
        assert_eq!(end.frame_index, frame_index);
        assert_eq!(end.stage, stage);
        assert_eq!(end.transition_kind, TransitionKind::End);
        assert_eq!(end.sequence_index, end_index);

        let end_log = end.to_structured_log();
        assert!(end_log.contains(&format!("frame_index={frame_index}")));
        assert!(end_log.contains(&format!("stage={}", stage.as_str())));
        assert!(end_log.contains("transition=end"));
        assert!(end_log.contains(&format!("sequence={end_index}")));
    }

    let expected_stage_trace = STAGE_ORDER
        .into_iter()
        .flat_map(|stage| [stage, stage])
        .collect::<Vec<Stage>>();
    let observed_stage_trace = execution
        .trace_entries
        .iter()
        .map(|entry| entry.stage)
        .collect::<Vec<Stage>>();

    assert_eq!(
        observed_stage_trace, expected_stage_trace,
        "trace ordering must match deterministic stage traversal"
    );
}

#[test]
fn hud_overlay_exposes_stage_progress() {
    let frame_index = 17;
    let mut streaming = StreamingState::new();
    let mut meshing = MeshingState::default();
    let execution = run_frame(&mut streaming, &mut meshing, None, frame_index);
    let overlay = &execution.overlay;

    assert_eq!(overlay.stage_progress.last_frame_index, Some(frame_index));
    assert_eq!(
        overlay.stage_progress.current_stage,
        Some(Stage::RenderSubmit),
        "one-frame execution should end on RenderSubmit"
    );
    assert_eq!(
        overlay.stage_progress.completed_stages, STAGE_ORDER,
        "overlay should report completed stages in deterministic order"
    );

    let derived_overlay = RuntimeHudOverlay::from_trace_entries(&execution.trace_entries);
    assert_eq!(
        overlay, &derived_overlay,
        "overlay must be derived directly from runtime trace entries"
    );

    let overlay_text = overlay.overlay_text();
    assert!(overlay_text.contains("overlay"));
    assert!(overlay_text.contains("current_stage=RenderSubmit"));
    assert!(overlay_text.contains("frame=17"));
    for stage in STAGE_ORDER {
        assert!(
            overlay_text.contains(stage.as_str()),
            "overlay text should include completed stage {}",
            stage.as_str()
        );
    }
}
