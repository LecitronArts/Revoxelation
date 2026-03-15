use log::info;

use super::{
    stages::{Stage, STAGE_ORDER},
    trace::{TraceEntry, TransitionKind},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameExecution {
    pub frame_index: u64,
    pub executed_stages: Vec<Stage>,
    pub trace_entries: Vec<TraceEntry>,
}

pub fn run_frame(frame_index: u64) -> FrameExecution {
    let mut executed_stages = Vec::with_capacity(STAGE_ORDER.len());
    let mut trace_entries = Vec::with_capacity(STAGE_ORDER.len() * 2);

    for (stage_index, stage) in STAGE_ORDER.into_iter().enumerate() {
        let begin_sequence = stage_index * 2;
        let begin = TraceEntry::new(frame_index, stage, TransitionKind::Begin, begin_sequence);
        info!(target: "runtime::trace", "{}", begin.to_structured_log());
        trace_entries.push(begin);

        executed_stages.push(stage);

        let end_sequence = begin_sequence + 1;
        let end = TraceEntry::new(frame_index, stage, TransitionKind::End, end_sequence);
        info!(target: "runtime::trace", "{}", end.to_structured_log());
        trace_entries.push(end);
    }

    FrameExecution {
        frame_index,
        executed_stages,
        trace_entries,
    }
}