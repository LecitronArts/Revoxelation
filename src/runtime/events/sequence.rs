use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SequenceMetadata {
    pub frame_index: u64,
    pub sequence: u64,
}

impl SequenceMetadata {
    pub const fn new(frame_index: u64, sequence: u64) -> Self {
        Self {
            frame_index,
            sequence,
        }
    }
}

pub const FRAME_SEQUENCE_STRIDE: u64 = 1_000_000;

#[derive(Debug, Clone)]
pub struct SequenceClock {
    frame_index: u64,
    next_sequence: u64,
}

impl SequenceClock {
    pub fn new(frame_index: u64) -> Self {
        let base_sequence = frame_index.saturating_mul(FRAME_SEQUENCE_STRIDE);
        Self {
            frame_index,
            next_sequence: base_sequence,
        }
    }

    pub fn next(&mut self) -> SequenceMetadata {
        let metadata = SequenceMetadata::new(self.frame_index, self.next_sequence);
        self.next_sequence = self.next_sequence.saturating_add(1);
        metadata
    }
}

pub fn is_monotonic(entries: &[SequenceMetadata]) -> bool {
    entries.windows(2).all(|pair| {
        pair[0].sequence < pair[1].sequence && pair[0].frame_index <= pair[1].frame_index
    })
}
