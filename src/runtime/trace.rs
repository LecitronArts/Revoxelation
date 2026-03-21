use super::stages::Stage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Begin,
    End,
}

impl TransitionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            TransitionKind::Begin => "begin",
            TransitionKind::End => "end",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry {
    pub frame_index: u64,
    pub stage: Stage,
    pub transition_kind: TransitionKind,
    pub sequence_index: usize,
}

impl TraceEntry {
    pub const fn new(
        frame_index: u64,
        stage: Stage,
        transition_kind: TransitionKind,
        sequence_index: usize,
    ) -> Self {
        Self {
            frame_index,
            stage,
            transition_kind,
            sequence_index,
        }
    }

    pub fn to_structured_log(&self) -> String {
        format!(
            "frame_index={} stage={} transition={} sequence={}",
            self.frame_index,
            self.stage.as_str(),
            self.transition_kind.as_str(),
            self.sequence_index
        )
    }
}
