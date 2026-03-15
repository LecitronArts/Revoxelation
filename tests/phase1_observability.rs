use revoxelation::runtime::{run_frame, Stage, TransitionKind, STAGE_ORDER};

#[test]
fn structured_logs_include_frame_stage_event() {
    let frame_index = 42;
    let execution = run_frame(frame_index);

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