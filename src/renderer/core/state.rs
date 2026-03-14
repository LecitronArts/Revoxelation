use std::collections::VecDeque;

use winit::dpi::PhysicalSize;

use crate::renderer::lifecycle::executor::LifecycleExecutionAction;
use crate::renderer::lifecycle::plan::RendererLifecycleEvent;
use crate::renderer::passes::reistir::ReSTIRPass;
use crate::renderer::passes::svgf::SvgfPass;
use crate::renderer::passes::trace::TracePass;
use crate::renderer::protocol::CameraGpu;
use crate::renderer::resources::context::RendererResourceContext;
use crate::renderer::resources::restir_storage::FrameBridge;

use super::frame_plan::FramePlan;

pub(crate) const SVGF_MAX_ATROUS_PASSES: usize = 5;
pub(crate) const RENDERER_DIAG_EVENT_CAPACITY: usize = 128;
pub const DEBUG_OVERLAY_MODE_NONE: u32 = 0;
pub const DEBUG_OVERLAY_MODE_PROBE: u32 = 1;
pub const DEBUG_OVERLAY_MODE_MOTION: u32 = 2;
pub const DEBUG_OVERLAY_MODE_HISTORY_VALIDITY: u32 = 3;
pub const DEBUG_OVERLAY_MODE_HISTORY_WEIGHT: u32 = 4;
pub const DEBUG_OVERLAY_MODE_REJECT_REASON: u32 = 5;
pub const DEBUG_OVERLAY_MODE_CLAMP_DIFF: u32 = 6;
pub const DEBUG_OVERLAY_MODE_TEMPORAL_VARIANCE: u32 = 7;
pub const DEBUG_OVERLAY_MODE_MAX: u32 = DEBUG_OVERLAY_MODE_TEMPORAL_VARIANCE;

#[derive(Debug, Clone, Copy)]
pub struct RendererSettings {
    pub max_bounces: u32,
    pub sun_intensity: f32,
    pub exposure: f32,
    pub environment_intensity: f32,
    pub sun_yaw_degrees: f32,
    pub sun_pitch_degrees: f32,
    pub max_history: f32,
    pub dda_max_steps: u32,
    pub rr_start_bounce: u32,
    pub rr_min_survival: f32,
    pub rr_max_survival: f32,
    pub restir_di_enabled: bool,
    pub restir_gi_enabled: bool,
    pub restir_spatial_radius: u32,
    pub restir_temporal_boost: f32,
    pub restir_gi_directional_gate: f32,
    pub restir_gi_reuse_m_cap: u32,
    pub restir_gi_reuse_weight_cap: f32,
    pub restir_gi_jacobian_min: f32,
    pub restir_gi_jacobian_max: f32,
    pub debug_overlay: bool,
    pub debug_overlay_mode: u32,
    pub svgf_enabled: bool,
    pub svgf_passes: u32,
    pub svgf_step_scale: u32,
    pub svgf_normal_phi: f32,
    pub svgf_depth_phi: f32,
    pub svgf_luma_phi: f32,
    pub svgf_clamp_sigma: f32,
    pub svgf_invalid_variance_boost: f32,
    pub svgf_center_weight: f32,
    pub svgf_history_normal_reject_cos: f32,
    pub svgf_history_depth_reject_scale: f32,
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            max_bounces: 3,
            sun_intensity: 8.0,
            exposure: 1.2,
            environment_intensity: 1.0,
            sun_yaw_degrees: 40.0,
            sun_pitch_degrees: 55.0,
            max_history: 24.0,
            dda_max_steps: 512,
            rr_start_bounce: 3,
            rr_min_survival: 0.1,
            rr_max_survival: 0.95,
            restir_di_enabled: true,
            restir_gi_enabled: true,
            restir_spatial_radius: 1,
            restir_temporal_boost: 1.0,
            restir_gi_directional_gate: 0.2,
            restir_gi_reuse_m_cap: 8,
            restir_gi_reuse_weight_cap: 24.0,
            restir_gi_jacobian_min: 0.25,
            restir_gi_jacobian_max: 3.0,
            debug_overlay: true,
            debug_overlay_mode: DEBUG_OVERLAY_MODE_PROBE,
            svgf_enabled: true,
            svgf_passes: 5,
            svgf_step_scale: 1,
            svgf_normal_phi: 1.5,
            svgf_depth_phi: 96.0,
            svgf_luma_phi: 2.0,
            svgf_clamp_sigma: 2.25,
            svgf_invalid_variance_boost: 3.5,
            svgf_center_weight: 4.0,
            svgf_history_normal_reject_cos: 0.85,
            svgf_history_depth_reject_scale: 0.10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RendererStats {
    pub frame_index: u32,
    pub chunk_count: u32,
    pub emissive_count: u32,
    pub chunk_map_avg_probe: f32,
    pub chunk_map_load_factor: f32,
    pub chunk_map_dropped_entries: u32,
    pub world_sync_reject_count: u32,
    pub last_world_sync_reject_reason: String,
    pub resolution: [u32; 2],
    pub camera_in_motion: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderDiagnosticsSummary {
    pub run_trace: bool,
    pub run_reistir: bool,
    pub run_svgf: bool,
    pub svgf_passes: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RendererDiagnostics {
    pub last_lifecycle_event: Option<RendererLifecycleEvent>,
    pub last_lifecycle_trace: Vec<LifecycleExecutionAction>,
    pub last_render_summary: RenderDiagnosticsSummary,
    pub last_render_trace_len: usize,
    pub world_sync_reject_count: u32,
    pub last_world_sync_reject_reason: String,
    pub chunk_map_dropped_entries: u32,
    pub resource_version_signature: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererDiagEventSnapshot {
    pub frame_index: u32,
    pub lifecycle_trace: Option<Vec<LifecycleExecutionAction>>,
    pub render_summary: Option<RenderDiagnosticsSummary>,
    pub render_trace_len: usize,
    pub resource_version_signature: u64,
    pub world_sync_reject_count: u32,
    pub last_world_sync_reject_reason: String,
    pub chunk_map_dropped_entries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RendererDiagEvent {
    SyncRejected(RendererDiagEventSnapshot),
    SyncSucceeded(RendererDiagEventSnapshot),
    Resize(RendererDiagEventSnapshot),
    Reconfigure(RendererDiagEventSnapshot),
    Render(RendererDiagEventSnapshot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererDiagEventKind {
    SyncRejected,
    SyncSucceeded,
    Resize,
    Reconfigure,
    Render,
}

impl RendererDiagEvent {
    #[allow(dead_code)]
    pub fn snapshot(&self) -> &RendererDiagEventSnapshot {
        match self {
            Self::SyncRejected(snapshot)
            | Self::SyncSucceeded(snapshot)
            | Self::Resize(snapshot)
            | Self::Reconfigure(snapshot)
            | Self::Render(snapshot) => snapshot,
        }
    }

    pub const fn kind(&self) -> RendererDiagEventKind {
        match self {
            Self::SyncRejected(_) => RendererDiagEventKind::SyncRejected,
            Self::SyncSucceeded(_) => RendererDiagEventKind::SyncSucceeded,
            Self::Resize(_) => RendererDiagEventKind::Resize,
            Self::Reconfigure(_) => RendererDiagEventKind::Reconfigure,
            Self::Render(_) => RendererDiagEventKind::Render,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RendererDiagEventRing {
    capacity: usize,
    entries: VecDeque<RendererDiagEvent>,
}

impl Default for RendererDiagEventRing {
    fn default() -> Self {
        Self::with_capacity(RENDERER_DIAG_EVENT_CAPACITY)
    }
}

impl RendererDiagEventRing {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    pub(crate) fn push(&mut self, event: RendererDiagEvent) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(event);
    }

    #[allow(dead_code)]
    pub(crate) fn recent(&self, limit: usize) -> Vec<RendererDiagEvent> {
        if limit == 0 {
            return Vec::new();
        }
        let skip = self.entries.len().saturating_sub(limit);
        self.entries.iter().skip(skip).cloned().collect()
    }

    pub(crate) fn recent_by_kind(
        &self,
        limit: usize,
        kind: RendererDiagEventKind,
    ) -> Vec<RendererDiagEvent> {
        if limit == 0 {
            return Vec::new();
        }
        let filtered = self
            .entries
            .iter()
            .filter(|event| event.kind() == kind)
            .cloned()
            .collect::<Vec<_>>();
        let skip = filtered.len().saturating_sub(limit);
        filtered.into_iter().skip(skip).collect()
    }

    pub(crate) fn since_frame(&self, frame_index: u32, limit: usize) -> Vec<RendererDiagEvent> {
        if limit == 0 {
            return Vec::new();
        }
        let filtered = self
            .entries
            .iter()
            .filter(|event| event.snapshot().frame_index >= frame_index)
            .cloned()
            .collect::<Vec<_>>();
        let skip = filtered.len().saturating_sub(limit);
        filtered.into_iter().skip(skip).collect()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RendererDiagnosticsState {
    pub last_lifecycle_event: Option<RendererLifecycleEvent>,
    pub last_lifecycle_trace: Vec<LifecycleExecutionAction>,
    pub last_render_summary: RenderDiagnosticsSummary,
    pub last_render_trace_len: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameContext {
    pub frame_index: u32,
    pub resolution: [u32; 2],
    pub history_read_slot: u32,
    pub history_write_slot: u32,
}

impl FrameContext {
    pub(crate) fn from_plan(plan: &FramePlan) -> Self {
        Self {
            frame_index: plan.frame_index,
            resolution: plan.resolution,
            history_read_slot: plan.history_read_slot,
            history_write_slot: plan.history_write_slot,
        }
    }
}

pub(crate) struct RendererGpuContext {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub max_storage_binding_size: u64,
    pub config: wgpu::SurfaceConfiguration,
    pub size: PhysicalSize<u32>,
    pub output_format: wgpu::TextureFormat,
}

pub(crate) struct RendererPipelineContext {
    pub trace_pipeline: wgpu::ComputePipeline,
    pub reistir_pipeline: wgpu::ComputePipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub bind_group: wgpu::BindGroup,
    pub svgf_init_pipeline: wgpu::ComputePipeline,
    pub svgf_atrous_pipeline: wgpu::ComputePipeline,
    pub svgf_resolve_pipeline: wgpu::ComputePipeline,
    pub svgf_bind_group_layout: wgpu::BindGroupLayout,
    pub svgf_init_bind_group: wgpu::BindGroup,
    pub svgf_atrous_bind_groups: Vec<wgpu::BindGroup>,
    pub svgf_resolve_bind_group: wgpu::BindGroup,
    pub egui_renderer: egui_wgpu::Renderer,
    pub trace_pass: TracePass,
    pub reistir_pass: ReSTIRPass,
    pub svgf_pass: SvgfPass,
}

pub(crate) struct RendererUniformContext {
    pub camera_buffer: wgpu::Buffer,
    pub previous_camera_buffer: wgpu::Buffer,
    pub tracer_uniform_buffer: wgpu::Buffer,
}

pub(crate) struct RendererRuntimeContext {
    pub importance_map_dims: [u32; 3],
    pub chunk_count: u32,
    pub chunk_map_size: u32,
    pub chunk_map_mask: u32,
    pub chunk_map_max_probe: u32,
    pub chunk_map_avg_probe: f32,
    pub chunk_map_max_probe_observed: u32,
    pub chunk_map_load_factor: f32,
    pub chunk_map_dropped_entries: u32,
    pub emissive_count: u32,
    pub emissive_cdf_count: u32,
    pub emissive_remap_count: u32,
    pub emissive_signatures: Vec<u32>,
    pub world_min: [i32; 3],
    pub world_max: [i32; 3],
    pub settings: RendererSettings,
    pub world_sync_reject_count: u32,
    pub last_world_sync_reject_reason: String,
    pub frame_bridge: FrameBridge,
    pub motion_frames_remaining: u32,
    pub camera_in_motion: bool,
    pub last_camera_gpu: CameraGpu,
    pub diagnostics: RendererDiagnosticsState,
    pub diag_events: RendererDiagEventRing,
}

pub struct Renderer {
    pub(crate) gpu: RendererGpuContext,
    pub(crate) pipelines: RendererPipelineContext,
    pub(crate) uniforms: RendererUniformContext,
    pub(crate) resources: RendererResourceContext,
    pub(crate) runtime: RendererRuntimeContext,
}

#[cfg(test)]
mod tests {
    use crate::renderer::lifecycle::executor::LifecycleExecutionAction;

    use super::{
        RenderDiagnosticsSummary, RendererDiagEvent, RendererDiagEventKind, RendererDiagEventRing,
        RendererDiagEventSnapshot,
    };

    fn snapshot(frame_index: u32) -> RendererDiagEventSnapshot {
        RendererDiagEventSnapshot {
            frame_index,
            lifecycle_trace: None,
            render_summary: None,
            render_trace_len: frame_index as usize,
            resource_version_signature: 100 + frame_index as u64,
            world_sync_reject_count: frame_index,
            last_world_sync_reject_reason: format!("reason-{frame_index}"),
            chunk_map_dropped_entries: frame_index.saturating_add(1),
        }
    }

    fn event_of_kind(kind: RendererDiagEventKind, frame_index: u32) -> RendererDiagEvent {
        let snapshot = snapshot(frame_index);
        match kind {
            RendererDiagEventKind::SyncRejected => RendererDiagEvent::SyncRejected(snapshot),
            RendererDiagEventKind::SyncSucceeded => RendererDiagEvent::SyncSucceeded(snapshot),
            RendererDiagEventKind::Resize => RendererDiagEvent::Resize(snapshot),
            RendererDiagEventKind::Reconfigure => RendererDiagEvent::Reconfigure(snapshot),
            RendererDiagEventKind::Render => RendererDiagEvent::Render(snapshot),
        }
    }

    #[test]
    fn diag_ring_preserves_order_under_capacity() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(1)));
        ring.push(RendererDiagEvent::SyncSucceeded(snapshot(2)));
        ring.push(RendererDiagEvent::Resize(snapshot(3)));
        let frames = ring
            .recent(10)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![1, 2, 3]);
    }

    #[test]
    fn diag_ring_drops_oldest_on_capacity_overflow() {
        let mut ring = RendererDiagEventRing::with_capacity(2);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(1)));
        ring.push(RendererDiagEvent::SyncSucceeded(snapshot(2)));
        ring.push(RendererDiagEvent::Resize(snapshot(3)));
        let frames = ring
            .recent(10)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![2, 3]);
    }

    #[test]
    fn diag_ring_keeps_latest_entries_after_multiple_overflows() {
        let mut ring = RendererDiagEventRing::with_capacity(3);
        for frame in 0..8 {
            ring.push(RendererDiagEvent::Render(snapshot(frame)));
        }
        let frames = ring
            .recent(10)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![5, 6, 7]);
    }

    #[test]
    fn diag_ring_with_zero_capacity_falls_back_to_one_slot() {
        let mut ring = RendererDiagEventRing::with_capacity(0);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(10)));
        ring.push(RendererDiagEvent::SyncSucceeded(snapshot(11)));
        let frames = ring
            .recent(10)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![11]);
    }

    #[test]
    fn diag_ring_recent_returns_oldest_to_newest() {
        let mut ring = RendererDiagEventRing::with_capacity(5);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(1)));
        ring.push(RendererDiagEvent::Resize(snapshot(2)));
        ring.push(RendererDiagEvent::Reconfigure(snapshot(3)));
        ring.push(RendererDiagEvent::Render(snapshot(4)));
        let frames = ring
            .recent(3)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![2, 3, 4]);
    }

    #[test]
    fn diag_ring_capacity_truncation_keeps_tail_order() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        for frame in 1..=6 {
            ring.push(RendererDiagEvent::Render(snapshot(frame)));
        }
        let frames = ring
            .recent(4)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![3, 4, 5, 6]);
    }

    #[test]
    fn recent_limit_zero_returns_empty() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(1)));
        assert!(ring.recent(0).is_empty());
    }

    #[test]
    fn recent_limit_one_returns_latest_event_only() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(1)));
        ring.push(RendererDiagEvent::SyncSucceeded(snapshot(2)));
        let frames = ring
            .recent(1)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![2]);
    }

    #[test]
    fn recent_limit_equal_to_len_returns_all_events() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(1)));
        ring.push(RendererDiagEvent::SyncSucceeded(snapshot(2)));
        ring.push(RendererDiagEvent::Resize(snapshot(3)));
        let frames = ring
            .recent(3)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![1, 2, 3]);
    }

    #[test]
    fn recent_limit_larger_than_len_returns_all_events() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        ring.push(RendererDiagEvent::SyncRejected(snapshot(1)));
        ring.push(RendererDiagEvent::SyncSucceeded(snapshot(2)));
        let frames = ring
            .recent(99)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![1, 2]);
    }

    #[test]
    fn sync_succeeded_event_has_lifecycle_trace_and_no_render_summary() {
        let mut snapshot = snapshot(7);
        snapshot.lifecycle_trace = Some(vec![
            LifecycleExecutionAction::RebuildBindGroups,
            LifecycleExecutionAction::ResetAccumulation,
        ]);
        let event = RendererDiagEvent::SyncSucceeded(snapshot);
        let data = event.snapshot();
        assert!(data.lifecycle_trace.is_some());
        assert!(data.render_summary.is_none());
    }

    #[test]
    fn sync_rejected_event_preserves_rejection_snapshot_fields() {
        let snapshot = snapshot(8);
        let event = RendererDiagEvent::SyncRejected(snapshot);
        let data = event.snapshot();
        assert_eq!(data.world_sync_reject_count, 8);
        assert_eq!(data.last_world_sync_reject_reason, "reason-8");
        assert_eq!(data.chunk_map_dropped_entries, 9);
    }

    #[test]
    fn render_event_has_render_summary_and_no_lifecycle_trace() {
        let mut snapshot = snapshot(9);
        snapshot.lifecycle_trace = None;
        snapshot.render_summary = Some(RenderDiagnosticsSummary {
            run_trace: true,
            run_reistir: true,
            run_svgf: true,
            svgf_passes: 3,
        });
        let event = RendererDiagEvent::Render(snapshot);
        let data = event.snapshot();
        assert!(data.lifecycle_trace.is_none());
        assert_eq!(
            data.render_summary,
            Some(RenderDiagnosticsSummary {
                run_trace: true,
                run_reistir: true,
                run_svgf: true,
                svgf_passes: 3,
            })
        );
    }

    #[test]
    fn resize_event_keeps_resource_signature_and_trace_len() {
        let mut snapshot = snapshot(10);
        snapshot.lifecycle_trace = Some(vec![
            LifecycleExecutionAction::ConfigureSurface,
            LifecycleExecutionAction::RebuildSurfaceResources,
            LifecycleExecutionAction::RebuildBindGroups,
            LifecycleExecutionAction::ResetAccumulation,
        ]);
        snapshot.render_trace_len = 6;
        snapshot.resource_version_signature = 0xAA55;
        let event = RendererDiagEvent::Resize(snapshot);
        let data = event.snapshot();
        assert_eq!(data.render_trace_len, 6);
        assert_eq!(data.resource_version_signature, 0xAA55);
        assert_eq!(
            data.lifecycle_trace.as_ref().map(|trace| trace.len()),
            Some(4)
        );
    }

    #[test]
    fn recent_by_type_limit_zero_returns_empty() {
        let mut ring = RendererDiagEventRing::with_capacity(5);
        ring.push(event_of_kind(RendererDiagEventKind::Render, 1));
        assert!(
            ring.recent_by_kind(0, RendererDiagEventKind::Render)
                .is_empty()
        );
    }

    #[test]
    fn recent_by_type_returns_only_matching_kind() {
        let mut ring = RendererDiagEventRing::with_capacity(6);
        ring.push(event_of_kind(RendererDiagEventKind::Render, 1));
        ring.push(event_of_kind(RendererDiagEventKind::Resize, 2));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 3));
        ring.push(event_of_kind(RendererDiagEventKind::SyncSucceeded, 4));
        let frames = ring
            .recent_by_kind(10, RendererDiagEventKind::Render)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![1, 3]);
    }

    #[test]
    fn recent_by_type_applies_limit_to_matching_events_only() {
        let mut ring = RendererDiagEventRing::with_capacity(10);
        ring.push(event_of_kind(RendererDiagEventKind::Render, 1));
        ring.push(event_of_kind(RendererDiagEventKind::Resize, 2));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 3));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 4));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 5));
        let frames = ring
            .recent_by_kind(2, RendererDiagEventKind::Render)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![4, 5]);
    }

    #[test]
    fn recent_by_type_preserves_old_to_new_order() {
        let mut ring = RendererDiagEventRing::with_capacity(10);
        ring.push(event_of_kind(RendererDiagEventKind::SyncRejected, 1));
        ring.push(event_of_kind(RendererDiagEventKind::SyncRejected, 3));
        ring.push(event_of_kind(RendererDiagEventKind::SyncRejected, 5));
        let frames = ring
            .recent_by_kind(10, RendererDiagEventKind::SyncRejected)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![1, 3, 5]);
    }

    #[test]
    fn since_frame_limit_zero_returns_empty() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        ring.push(event_of_kind(RendererDiagEventKind::Render, 5));
        assert!(ring.since_frame(5, 0).is_empty());
    }

    #[test]
    fn since_frame_is_inclusive_of_target_frame() {
        let mut ring = RendererDiagEventRing::with_capacity(6);
        ring.push(event_of_kind(RendererDiagEventKind::Render, 3));
        ring.push(event_of_kind(RendererDiagEventKind::Resize, 4));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 5));
        let frames = ring
            .since_frame(4, 10)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![4, 5]);
    }

    #[test]
    fn since_frame_returns_empty_when_frame_is_newer_than_all_events() {
        let mut ring = RendererDiagEventRing::with_capacity(4);
        ring.push(event_of_kind(RendererDiagEventKind::Render, 1));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 2));
        assert!(ring.since_frame(10, 3).is_empty());
    }

    #[test]
    fn since_frame_applies_tail_limit_while_preserving_order() {
        let mut ring = RendererDiagEventRing::with_capacity(10);
        ring.push(event_of_kind(RendererDiagEventKind::Render, 1));
        ring.push(event_of_kind(RendererDiagEventKind::Resize, 2));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 3));
        ring.push(event_of_kind(RendererDiagEventKind::Reconfigure, 4));
        ring.push(event_of_kind(RendererDiagEventKind::Render, 5));
        let frames = ring
            .since_frame(2, 2)
            .into_iter()
            .map(|event| event.snapshot().frame_index)
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![4, 5]);
    }
}
