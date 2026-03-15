use revoxelation::runtime::events::{
    is_monotonic, BlockEditCommand, BlockEditOperation, BlockPosition, ChunkCoordinate,
    ChunkLifecycleAction, ChunkLifecycleCommand, CommandEnvelope, CommandOutcome,
    CommandOutcomeEvent, EventEnvelope, PlayerAction, PlayerActionCommand, RejectionReason,
    RuntimeCommand, RuntimeEvent, SequenceMetadata,
};

#[test]
fn wave0_events_selector_bootstrap() {
    let selectors = ["event_serde_roundtrip_models"];

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
