use serde::{Deserialize, Serialize};

use super::sequence::SequenceMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    PlayerAction,
    ChunkLifecycle,
    BlockEdit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEnvelope {
    pub sequence: SequenceMetadata,
    pub command: RuntimeCommand,
}

impl CommandEnvelope {
    pub fn kind(&self) -> CommandKind {
        self.command.kind()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "family", content = "payload", rename_all = "snake_case")]
pub enum RuntimeCommand {
    PlayerAction(PlayerActionCommand),
    ChunkLifecycle(ChunkLifecycleCommand),
    BlockEdit(BlockEditCommand),
}

impl RuntimeCommand {
    pub fn kind(&self) -> CommandKind {
        match self {
            RuntimeCommand::PlayerAction(_) => CommandKind::PlayerAction,
            RuntimeCommand::ChunkLifecycle(_) => CommandKind::ChunkLifecycle,
            RuntimeCommand::BlockEdit(_) => CommandKind::BlockEdit,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerActionCommand {
    pub actor_entity_id: u64,
    pub action: PlayerAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "details", rename_all = "snake_case")]
pub enum PlayerAction {
    Jump,
    StartMining { position: BlockPosition },
    StopMining,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkLifecycleCommand {
    pub chunk: ChunkCoordinate,
    pub action: ChunkLifecycleAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkLifecycleAction {
    Activate,
    Deactivate,
    Invalidate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockEditCommand {
    pub actor_entity_id: u64,
    pub position: BlockPosition,
    pub edit: BlockEditOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "edit", content = "details", rename_all = "snake_case")]
pub enum BlockEditOperation {
    Place { block_id: String },
    Remove,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkCoordinate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockPosition {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}
