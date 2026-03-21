use serde::{Deserialize, Serialize};

use super::command::{BlockEditCommand, ChunkLifecycleCommand, CommandKind, PlayerActionCommand};
use super::sequence::SequenceMetadata;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub sequence: SequenceMetadata,
    pub event: RuntimeEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "family", content = "payload", rename_all = "snake_case")]
pub enum RuntimeEvent {
    CommandOutcome(CommandOutcomeEvent),
    PlayerActionApplied(PlayerActionCommand),
    ChunkLifecycleApplied(ChunkLifecycleCommand),
    BlockEditApplied(BlockEditCommand),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcomeEvent {
    pub command_kind: CommandKind,
    pub outcome: CommandOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "details", rename_all = "snake_case")]
pub enum CommandOutcome {
    Accepted,
    Rejected { reason: RejectionReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectionReason {
    pub code: String,
    pub message: String,
}

impl RejectionReason {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
