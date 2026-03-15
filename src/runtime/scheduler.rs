use super::stages::{Stage, STAGE_ORDER};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameExecution {
    pub frame_index: u64,
    pub executed_stages: Vec<Stage>,
}

pub fn run_frame(frame_index: u64) -> FrameExecution {
    let mut executed_stages = Vec::with_capacity(STAGE_ORDER.len());

    for stage in STAGE_ORDER {
        executed_stages.push(stage);
    }

    FrameExecution {
        frame_index,
        executed_stages,
    }
}