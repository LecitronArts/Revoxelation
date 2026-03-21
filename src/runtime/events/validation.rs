use super::command::{BlockEditOperation, ChunkLifecycleAction, PlayerAction, RuntimeCommand};
use super::event::RejectionReason;

pub fn validate(command: &RuntimeCommand) -> Result<(), RejectionReason> {
    match command {
        RuntimeCommand::PlayerAction(player_action) => {
            if player_action.actor_entity_id == 0 {
                return Err(RejectionReason::new(
                    "invalid_actor_id",
                    "player action commands require a non-zero actor entity id",
                ));
            }

            if let PlayerAction::StartMining { position } = player_action.action {
                if !(0..=511).contains(&position.y) {
                    return Err(RejectionReason::new(
                        "block_position_out_of_scope",
                        "player action target y must remain within [0, 511]",
                    ));
                }
            }

            Ok(())
        }
        RuntimeCommand::ChunkLifecycle(chunk_lifecycle) => {
            if !(-64..=64).contains(&chunk_lifecycle.chunk.y) {
                return Err(RejectionReason::new(
                    "chunk_y_out_of_scope",
                    "chunk lifecycle commands require chunk.y within [-64, 64]",
                ));
            }

            if matches!(chunk_lifecycle.action, ChunkLifecycleAction::Invalidate)
                && chunk_lifecycle.chunk.x == 0
                && chunk_lifecycle.chunk.z == 0
            {
                return Err(RejectionReason::new(
                    "invalid_root_chunk_invalidation",
                    "chunk invalidation at origin is reserved for scheduler-driven flow",
                ));
            }

            Ok(())
        }
        RuntimeCommand::BlockEdit(block_edit) => {
            if block_edit.actor_entity_id == 0 {
                return Err(RejectionReason::new(
                    "invalid_actor_id",
                    "block edit commands require a non-zero actor entity id",
                ));
            }

            if !(0..=511).contains(&block_edit.position.y) {
                return Err(RejectionReason::new(
                    "block_position_out_of_scope",
                    "block edit y coordinate must remain within [0, 511]",
                ));
            }

            if let BlockEditOperation::Place { block_id } = &block_edit.edit {
                if block_id.trim().is_empty() {
                    return Err(RejectionReason::new(
                        "empty_block_id",
                        "block edit place commands require a non-empty block id",
                    ));
                }
            }

            Ok(())
        }
    }
}
