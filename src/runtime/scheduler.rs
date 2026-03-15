use log::info;

use super::{
    events::{
        BlockEditCommand, BlockEditOperation, BlockPosition, ChunkCoordinate,
        ChunkLifecycleAction, ChunkLifecycleCommand, EventBus, EventBusSnapshot, PlayerAction,
        PlayerActionCommand, RuntimeCommand,
    },
    observability::RuntimeHudOverlay,
    stages::{Stage, STAGE_ORDER},
    trace::{TraceEntry, TransitionKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameExecution {
    pub frame_index: u64,
    pub executed_stages: Vec<Stage>,
    pub trace_entries: Vec<TraceEntry>,
    pub overlay: RuntimeHudOverlay,
    pub event_bus: EventBusSnapshot,
}

pub fn run_frame(frame_index: u64) -> FrameExecution {
    let mut executed_stages = Vec::with_capacity(STAGE_ORDER.len());
    let mut trace_entries = Vec::with_capacity(STAGE_ORDER.len() * 2);
    let mut event_bus = EventBus::new(frame_index);

    for (stage_index, stage) in STAGE_ORDER.into_iter().enumerate() {
        let begin_sequence = stage_index * 2;
        let begin = TraceEntry::new(frame_index, stage, TransitionKind::Begin, begin_sequence);
        info!(target: "runtime::trace", "{}", begin.to_structured_log());
        trace_entries.push(begin);

        match stage {
            Stage::Input => seed_input_commands(&mut event_bus),
            Stage::Simulation => event_bus.process_pending_commands(),
            Stage::RenderSubmit => {
                let _ = event_bus.consume_emitted();
            }
            Stage::WorldUpdate | Stage::MeshSync => {}
        }

        executed_stages.push(stage);

        let end_sequence = begin_sequence + 1;
        let end = TraceEntry::new(frame_index, stage, TransitionKind::End, end_sequence);
        info!(target: "runtime::trace", "{}", end.to_structured_log());
        trace_entries.push(end);
    }

    let overlay = RuntimeHudOverlay::from_trace_entries(&trace_entries);

    FrameExecution {
        frame_index,
        executed_stages,
        trace_entries,
        overlay,
        event_bus: event_bus.snapshot(),
    }
}

fn seed_input_commands(event_bus: &mut EventBus) {
    let _ = event_bus.publish_command(RuntimeCommand::PlayerAction(PlayerActionCommand {
        actor_entity_id: 1,
        action: PlayerAction::Jump,
    }));

    let _ = event_bus.publish_command(RuntimeCommand::ChunkLifecycle(ChunkLifecycleCommand {
        chunk: ChunkCoordinate { x: 0, y: 0, z: 0 },
        action: ChunkLifecycleAction::Activate,
    }));

    let _ = event_bus.publish_command(RuntimeCommand::BlockEdit(BlockEditCommand {
        actor_entity_id: 1,
        position: BlockPosition { x: 0, y: 64, z: 0 },
        edit: BlockEditOperation::Place {
            block_id: "stone".to_string(),
        },
    }));
}
