use revoxelation::runtime::{
    events::{
        BlockEditCommand, BlockEditOperation, BlockPosition, ChunkCoordinate, ChunkLifecycleAction,
        ChunkLifecycleCommand, CommandEnvelope, CommandOutcome, CommandOutcomeEvent, EventBus,
        EventEnvelope, PlayerAction, PlayerActionCommand, RejectionReason, RuntimeCommand,
        RuntimeEvent, SequenceMetadata, is_monotonic,
    },
    run_frame,
};

#[test]
fn wave0_events_selector_bootstrap() {
    let selectors = [
        "event_serde_roundtrip_models",
        "invalid_command_rejected_with_reason",
        "one_frame_event_flow_is_monotonic",
    ];

    for selector in selectors {
        assert!(
            selector.contains('_'),
            "event selector should use underscore-delimited naming: {selector}",
        );
    }
}

#[test]
fn event_serde_roundtrip_models() {
    let command_models = vec![
        CommandEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_000),
            command: RuntimeCommand::PlayerAction(PlayerActionCommand {
                actor_entity_id: 7,
                action: PlayerAction::StartMining {
                    position: BlockPosition { x: 4, y: 65, z: -2 },
                },
            }),
        },
        CommandEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_001),
            command: RuntimeCommand::ChunkLifecycle(ChunkLifecycleCommand {
                chunk: ChunkCoordinate { x: 2, y: 0, z: -3 },
                action: ChunkLifecycleAction::Activate,
                lod_level: 0,
            }),
        },
        CommandEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_002),
            command: RuntimeCommand::BlockEdit(BlockEditCommand {
                actor_entity_id: 9,
                position: BlockPosition { x: 9, y: 70, z: 1 },
                edit: BlockEditOperation::Remove,
            }),
        },
    ];

    let encoded_commands = serde_json::to_string(&command_models)
        .expect("command models should serialize to JSON deterministically");
    let decoded_commands: Vec<CommandEnvelope> = serde_json::from_str(&encoded_commands)
        .expect("command models should deserialize from JSON");
    assert_eq!(decoded_commands, command_models);

    let event_models = vec![
        EventEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_003),
            event: RuntimeEvent::CommandOutcome(CommandOutcomeEvent {
                command_kind: command_models[0].kind(),
                outcome: CommandOutcome::Accepted,
            }),
        },
        EventEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_004),
            event: RuntimeEvent::CommandOutcome(CommandOutcomeEvent {
                command_kind: command_models[1].kind(),
                outcome: CommandOutcome::Rejected {
                    reason: RejectionReason::new(
                        "chunk_y_out_of_scope",
                        "chunk lifecycle commands require chunk.y within [-64, 64]",
                    ),
                },
            }),
        },
        EventEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_005),
            event: RuntimeEvent::PlayerActionApplied(PlayerActionCommand {
                actor_entity_id: 7,
                action: PlayerAction::Jump,
            }),
        },
        EventEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_006),
            event: RuntimeEvent::ChunkLifecycleApplied(ChunkLifecycleCommand {
                chunk: ChunkCoordinate { x: 2, y: 0, z: -3 },
                action: ChunkLifecycleAction::Deactivate,
                lod_level: 0,
            }),
        },
        EventEnvelope {
            sequence: SequenceMetadata::new(1, 1_000_007),
            event: RuntimeEvent::BlockEditApplied(BlockEditCommand {
                actor_entity_id: 9,
                position: BlockPosition { x: 9, y: 70, z: 1 },
                edit: BlockEditOperation::Place {
                    block_id: "granite".to_string(),
                },
            }),
        },
    ];

    let encoded_events =
        serde_json::to_string(&event_models).expect("event models should serialize to JSON");
    let decoded_events: Vec<EventEnvelope> =
        serde_json::from_str(&encoded_events).expect("event models should deserialize from JSON");
    assert_eq!(decoded_events, event_models);

    let sequence_order = decoded_events
        .iter()
        .map(|entry| entry.sequence)
        .collect::<Vec<_>>();
    assert!(
        is_monotonic(&sequence_order),
        "event sequence metadata should remain monotonic after serde roundtrip",
    );
}

#[test]
fn invalid_command_rejected_with_reason() {
    let mut event_bus = EventBus::new(2);
    let _ = event_bus.publish_command(RuntimeCommand::BlockEdit(BlockEditCommand {
        actor_entity_id: 5,
        position: BlockPosition { x: 0, y: 64, z: 0 },
        edit: BlockEditOperation::Place {
            block_id: "   ".to_string(),
        },
    }));

    event_bus.process_pending_commands();
    let snapshot = event_bus.snapshot();

    let outcomes = snapshot
        .emitted_events
        .iter()
        .filter_map(|entry| {
            if let RuntimeEvent::CommandOutcome(outcome) = &entry.event {
                Some(outcome)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    assert_eq!(outcomes.len(), 1, "invalid command should emit one outcome");

    let expected_rejection = CommandOutcome::Rejected {
        reason: RejectionReason::new(
            "empty_block_id",
            "block edit place commands require a non-empty block id",
        ),
    };
    assert_eq!(outcomes[0].outcome.clone(), expected_rejection);

    assert!(
        snapshot
            .emitted_events
            .iter()
            .all(|entry| !matches!(entry.event, RuntimeEvent::BlockEditApplied(_))),
        "invalid block edit command should never emit a block edit applied event",
    );
}

#[test]
fn one_frame_event_flow_is_monotonic() {
    let frame = run_frame(9);
    let event_bus = &frame.event_bus;

    assert_eq!(event_bus.frame_index, 9);
    assert_eq!(
        event_bus.emitted_events, event_bus.consumed_events,
        "render submit stage should consume all events emitted in the frame",
    );
    assert!(
        event_bus.emitted_is_monotonic(),
        "emitted events must preserve deterministic monotonic sequence ordering",
    );

    for sequence in event_bus.emitted_sequences() {
        assert_eq!(
            sequence.frame_index, 9,
            "single-frame event flow should retain frame metadata",
        );
    }
}
