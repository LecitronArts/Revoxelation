use crate::renderer::RendererSettings;
use crate::renderer::protocol::{
    encode_history_flags, history_read_slot_from_frame, history_write_slot_from_frame,
    svgf_atrous_source_slot, svgf_resolve_source_slot,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PassMetadata {
    pub run_trace: bool,
    pub run_reistir: bool,
    pub run_svgf: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePlan {
    pub frame_index: u32,
    pub resolution: [u32; 2],
    pub history_read_slot: u32,
    pub history_write_slot: u32,
    pub history_flags: u32,
    pub svgf_passes: usize,
    pub svgf_resolve_source_slot: u32,
    pub passes: PassMetadata,
}

impl FramePlan {
    pub const fn svgf_atrous_source_slot(self, pass_index: u32) -> u32 {
        svgf_atrous_source_slot(self.history_write_slot, pass_index)
    }
}

pub fn build_frame_plan(
    frame_index: u32,
    width: u32,
    height: u32,
    settings: &RendererSettings,
    max_svgf_passes: usize,
) -> FramePlan {
    let resolution = [width.max(1), height.max(1)];
    let history_read_slot = history_read_slot_from_frame(frame_index);
    let history_write_slot = history_write_slot_from_frame(frame_index);
    let history_flags = encode_history_flags(history_read_slot, history_write_slot);
    let svgf_passes = if settings.svgf_enabled {
        settings.svgf_passes.min(max_svgf_passes as u32) as usize
    } else {
        0
    };
    let svgf_resolve_source_slot = svgf_resolve_source_slot(history_write_slot, svgf_passes as u32);
    let passes = PassMetadata {
        run_trace: true,
        run_reistir: true,
        run_svgf: settings.svgf_enabled,
    };

    FramePlan {
        frame_index,
        resolution,
        history_read_slot,
        history_write_slot,
        history_flags,
        svgf_passes,
        svgf_resolve_source_slot,
        passes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_SVGF_PASSES: usize = 5;

    #[test]
    fn frame_plan_even_frame_uses_expected_history_slots() {
        let settings = RendererSettings::default();
        let plan = build_frame_plan(0, 1920, 1080, &settings, MAX_SVGF_PASSES);
        assert_eq!(plan.history_read_slot, 0);
        assert_eq!(plan.history_write_slot, 1);
    }

    #[test]
    fn frame_plan_odd_frame_uses_expected_history_slots() {
        let settings = RendererSettings::default();
        let plan = build_frame_plan(1, 1920, 1080, &settings, MAX_SVGF_PASSES);
        assert_eq!(plan.history_read_slot, 1);
        assert_eq!(plan.history_write_slot, 0);
    }

    #[test]
    fn frame_plan_history_flags_match_slot_encoding() {
        let settings = RendererSettings::default();
        let plan = build_frame_plan(2, 800, 600, &settings, MAX_SVGF_PASSES);
        assert_eq!(plan.history_flags, 2);
    }

    #[test]
    fn frame_plan_clamps_resolution_to_non_zero() {
        let settings = RendererSettings::default();
        let plan = build_frame_plan(3, 0, 0, &settings, MAX_SVGF_PASSES);
        assert_eq!(plan.resolution, [1, 1]);
    }

    #[test]
    fn frame_plan_disables_svgf_passes_when_svgf_is_disabled() {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = false;
        settings.svgf_passes = 4;
        let plan = build_frame_plan(4, 640, 480, &settings, MAX_SVGF_PASSES);
        assert!(!plan.passes.run_svgf);
        assert_eq!(plan.svgf_passes, 0);
        assert_eq!(plan.svgf_resolve_source_slot, plan.history_write_slot);
    }

    #[test]
    fn frame_plan_clamps_svgf_pass_count_to_maximum() {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = true;
        settings.svgf_passes = 64;
        let plan = build_frame_plan(5, 640, 480, &settings, MAX_SVGF_PASSES);
        assert!(plan.passes.run_svgf);
        assert_eq!(plan.svgf_passes, MAX_SVGF_PASSES);
    }

    #[test]
    fn frame_plan_exposes_atrous_source_slot_sequence() {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = true;
        settings.svgf_passes = 3;
        let plan = build_frame_plan(0, 1024, 768, &settings, MAX_SVGF_PASSES);
        assert_eq!(plan.svgf_atrous_source_slot(0), 1);
        assert_eq!(plan.svgf_atrous_source_slot(1), 0);
        assert_eq!(plan.svgf_atrous_source_slot(2), 1);
    }

    #[test]
    fn frame_plan_always_runs_trace_and_reistir() {
        let settings = RendererSettings::default();
        let plan = build_frame_plan(9, 1280, 720, &settings, MAX_SVGF_PASSES);
        assert!(plan.passes.run_trace);
        assert!(plan.passes.run_reistir);
    }
}
