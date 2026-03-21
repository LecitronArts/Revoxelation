use crate::runtime::{Stage, TraceEntry, TransitionKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeOverlayStageProgress {
    pub current_stage: Option<Stage>,
    pub completed_stages: Vec<Stage>,
    pub last_frame_index: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeHudOverlay {
    pub stage_progress: RuntimeOverlayStageProgress,
}

impl RuntimeHudOverlay {
    pub fn from_trace_entries(trace_entries: &[TraceEntry]) -> Self {
        let mut current_stage = None;
        let mut completed_stages = Vec::new();
        let mut last_frame_index = None;

        for entry in trace_entries {
            last_frame_index = Some(entry.frame_index);
            match entry.transition_kind {
                TransitionKind::Begin => {
                    current_stage = Some(entry.stage);
                }
                TransitionKind::End => {
                    current_stage = Some(entry.stage);
                    completed_stages.push(entry.stage);
                }
            }
        }

        Self {
            stage_progress: RuntimeOverlayStageProgress {
                current_stage,
                completed_stages,
                last_frame_index,
            },
        }
    }

    pub fn overlay_text(&self) -> String {
        let current = self
            .stage_progress
            .current_stage
            .map(Stage::as_str)
            .unwrap_or("None");

        let completed = self
            .stage_progress
            .completed_stages
            .iter()
            .map(|stage| stage.as_str())
            .collect::<Vec<_>>()
            .join(" -> ");

        let frame = self
            .stage_progress
            .last_frame_index
            .map(|value| value.to_string())
            .unwrap_or_else(|| "None".to_string());

        format!(
            "overlay frame={} current_stage={} completed_stages={}",
            frame, current, completed
        )
    }
}
