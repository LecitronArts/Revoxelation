use std::sync::Arc;

use anyhow::Result;
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::renderer::camera::PhysicalCamera;
use crate::world::VoxelWorld;

use super::{bootstrap, frame_exec, world_ops};

pub use super::state::{
    FrameContext, Renderer, RendererDiagEvent, RendererDiagEventKind, RendererDiagnostics,
    RendererSettings, RendererStats,
};

pub(crate) fn sanitize_renderer_settings(settings: RendererSettings) -> RendererSettings {
    let mut settings = settings;
    settings.max_bounces = settings.max_bounces.clamp(1, 16);
    settings.sun_intensity = settings.sun_intensity.max(0.0);
    settings.exposure = settings.exposure.clamp(0.05, 16.0);
    settings.environment_intensity = settings.environment_intensity.max(0.0);
    settings.max_history = settings.max_history.clamp(1.0, 256.0);
    settings.dda_max_steps = settings.dda_max_steps.clamp(32, 2048);
    settings.rr_start_bounce = settings.rr_start_bounce.clamp(1, 12);
    settings.rr_min_survival = settings.rr_min_survival.clamp(0.01, 0.99);
    settings.rr_max_survival = settings
        .rr_max_survival
        .clamp(settings.rr_min_survival, 0.995);
    settings.restir_spatial_radius = settings.restir_spatial_radius.clamp(0, 2);
    settings.restir_temporal_boost = settings.restir_temporal_boost.clamp(0.0, 8.0);
    settings.restir_gi_directional_gate = settings.restir_gi_directional_gate.clamp(-0.25, 0.99);
    settings.restir_gi_reuse_m_cap = settings.restir_gi_reuse_m_cap.clamp(1, 32);
    settings.restir_gi_reuse_weight_cap = settings.restir_gi_reuse_weight_cap.clamp(1.0, 128.0);
    settings.restir_gi_jacobian_min = settings.restir_gi_jacobian_min.clamp(0.01, 1.0);
    settings.restir_gi_jacobian_max = settings
        .restir_gi_jacobian_max
        .clamp(settings.restir_gi_jacobian_min, 16.0);
    settings.debug_overlay_mode = settings
        .debug_overlay_mode
        .clamp(0, super::state::DEBUG_OVERLAY_MODE_MAX);
    settings.svgf_passes = settings
        .svgf_passes
        .clamp(0, super::state::SVGF_MAX_ATROUS_PASSES as u32);
    settings.svgf_step_scale = settings.svgf_step_scale.clamp(1, 4);
    settings.svgf_normal_phi = settings.svgf_normal_phi.clamp(0.05, 16.0);
    settings.svgf_depth_phi = settings.svgf_depth_phi.clamp(1.0, 256.0);
    settings.svgf_luma_phi = settings.svgf_luma_phi.clamp(0.1, 12.0);
    settings.svgf_clamp_sigma = settings.svgf_clamp_sigma.clamp(0.0, 8.0);
    settings.svgf_invalid_variance_boost = settings.svgf_invalid_variance_boost.clamp(1.0, 16.0);
    settings.svgf_center_weight = settings.svgf_center_weight.clamp(0.5, 12.0);
    settings.svgf_history_normal_reject_cos =
        settings.svgf_history_normal_reject_cos.clamp(0.5, 0.999);
    settings.svgf_history_depth_reject_scale =
        settings.svgf_history_depth_reject_scale.clamp(0.01, 0.5);
    settings
}

impl Renderer {
    pub async fn new(window: Arc<Window>, world: Arc<VoxelWorld>) -> Result<Self> {
        bootstrap::bootstrap_renderer(window, world).await
    }

    pub fn sync_world(&mut self, world: &VoxelWorld) {
        world_ops::sync_world(self, world);
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        world_ops::resize(self, new_size);
    }

    pub fn reconfigure(&mut self) {
        world_ops::reconfigure(self);
    }

    pub fn render(
        &mut self,
        camera: &PhysicalCamera,
        paint_jobs: &[egui::ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
        pixels_per_point: f32,
    ) -> Result<(), wgpu::SurfaceError> {
        frame_exec::render_frame(self, camera, paint_jobs, textures_delta, pixels_per_point)
    }

    pub fn settings(&self) -> RendererSettings {
        self.runtime.settings
    }

    pub fn force_reset_history(&mut self) {
        world_ops::reset_accumulation(self);
    }

    pub fn update_settings(&mut self, settings: RendererSettings) {
        let settings = sanitize_renderer_settings(settings);

        let changed = self.runtime.settings.max_bounces != settings.max_bounces
            || (self.runtime.settings.sun_intensity - settings.sun_intensity).abs() > 1.0e-5
            || (self.runtime.settings.environment_intensity - settings.environment_intensity).abs()
                > 1.0e-5
            || (self.runtime.settings.sun_yaw_degrees - settings.sun_yaw_degrees).abs() > 1.0e-5
            || (self.runtime.settings.sun_pitch_degrees - settings.sun_pitch_degrees).abs()
                > 1.0e-5
            || (self.runtime.settings.max_history - settings.max_history).abs() > 1.0e-5
            || self.runtime.settings.dda_max_steps != settings.dda_max_steps
            || self.runtime.settings.rr_start_bounce != settings.rr_start_bounce
            || (self.runtime.settings.rr_min_survival - settings.rr_min_survival).abs() > 1.0e-5
            || (self.runtime.settings.rr_max_survival - settings.rr_max_survival).abs() > 1.0e-5
            || self.runtime.settings.restir_di_enabled != settings.restir_di_enabled
            || self.runtime.settings.restir_gi_enabled != settings.restir_gi_enabled
            || self.runtime.settings.restir_spatial_radius != settings.restir_spatial_radius
            || (self.runtime.settings.restir_temporal_boost - settings.restir_temporal_boost).abs()
                > 1.0e-5
            || (self.runtime.settings.restir_gi_directional_gate
                - settings.restir_gi_directional_gate)
                .abs()
                > 1.0e-5
            || self.runtime.settings.restir_gi_reuse_m_cap != settings.restir_gi_reuse_m_cap
            || (self.runtime.settings.restir_gi_reuse_weight_cap
                - settings.restir_gi_reuse_weight_cap)
                .abs()
                > 1.0e-5
            || (self.runtime.settings.restir_gi_jacobian_min - settings.restir_gi_jacobian_min)
                .abs()
                > 1.0e-5
            || (self.runtime.settings.restir_gi_jacobian_max - settings.restir_gi_jacobian_max)
                .abs()
                > 1.0e-5
            || self.runtime.settings.debug_overlay_mode != settings.debug_overlay_mode
            || self.runtime.settings.svgf_enabled != settings.svgf_enabled
            || self.runtime.settings.svgf_passes != settings.svgf_passes
            || self.runtime.settings.svgf_step_scale != settings.svgf_step_scale
            || (self.runtime.settings.svgf_normal_phi - settings.svgf_normal_phi).abs() > 1.0e-5
            || (self.runtime.settings.svgf_depth_phi - settings.svgf_depth_phi).abs() > 1.0e-5
            || (self.runtime.settings.svgf_luma_phi - settings.svgf_luma_phi).abs() > 1.0e-5
            || (self.runtime.settings.svgf_clamp_sigma - settings.svgf_clamp_sigma).abs() > 1.0e-5
            || (self.runtime.settings.svgf_invalid_variance_boost
                - settings.svgf_invalid_variance_boost)
                .abs()
                > 1.0e-5
            || (self.runtime.settings.svgf_center_weight - settings.svgf_center_weight).abs()
                > 1.0e-5
            || (self.runtime.settings.svgf_history_normal_reject_cos
                - settings.svgf_history_normal_reject_cos)
                .abs()
                > 1.0e-5
            || (self.runtime.settings.svgf_history_depth_reject_scale
                - settings.svgf_history_depth_reject_scale)
                .abs()
                > 1.0e-5;
        self.runtime.settings = settings;
        if changed {
            world_ops::reset_accumulation(self);
        }
    }

    pub fn stats(&self) -> RendererStats {
        RendererStats {
            frame_index: self.runtime.frame_bridge.frame_index(),
            chunk_count: self.runtime.chunk_count,
            emissive_count: self.runtime.emissive_count,
            chunk_map_avg_probe: self.runtime.chunk_map_avg_probe,
            chunk_map_load_factor: self.runtime.chunk_map_load_factor,
            chunk_map_dropped_entries: self.runtime.chunk_map_dropped_entries,
            world_sync_reject_count: self.runtime.world_sync_reject_count,
            last_world_sync_reject_reason: self.runtime.last_world_sync_reject_reason.clone(),
            resolution: [self.gpu.config.width, self.gpu.config.height],
            camera_in_motion: self.runtime.camera_in_motion,
        }
    }

    #[allow(dead_code)]
    pub fn diagnostics(&self) -> RendererDiagnostics {
        RendererDiagnostics {
            last_lifecycle_event: self.runtime.diagnostics.last_lifecycle_event,
            last_lifecycle_trace: self.runtime.diagnostics.last_lifecycle_trace.clone(),
            last_render_summary: self.runtime.diagnostics.last_render_summary,
            last_render_trace_len: self.runtime.diagnostics.last_render_trace_len,
            world_sync_reject_count: self.runtime.world_sync_reject_count,
            last_world_sync_reject_reason: self.runtime.last_world_sync_reject_reason.clone(),
            chunk_map_dropped_entries: self.runtime.chunk_map_dropped_entries,
            resource_version_signature: self.resources.versions().dependency_signature(),
        }
    }

    #[allow(dead_code)]
    pub fn diagnostics_recent(&self, limit: usize) -> Vec<RendererDiagEvent> {
        self.runtime.diag_events.recent(limit)
    }

    #[allow(dead_code)]
    pub fn diagnostics_recent_by_type(
        &self,
        limit: usize,
        event_kind: RendererDiagEventKind,
    ) -> Vec<RendererDiagEvent> {
        self.runtime.diag_events.recent_by_kind(limit, event_kind)
    }

    #[allow(dead_code)]
    pub fn diagnostics_since_frame(
        &self,
        frame_index: u32,
        limit: usize,
    ) -> Vec<RendererDiagEvent> {
        self.runtime.diag_events.since_frame(frame_index, limit)
    }
}

#[cfg(test)]
mod tests {
    use crate::renderer::core::state::DEBUG_OVERLAY_MODE_MAX;

    use super::sanitize_renderer_settings;
    use crate::renderer::RendererSettings;

    #[test]
    fn overlay_mode_zero_survives_sanitization() {
        let mut settings = RendererSettings::default();
        settings.debug_overlay_mode = 0;
        let sanitized = sanitize_renderer_settings(settings);
        assert_eq!(sanitized.debug_overlay_mode, 0);
    }

    #[test]
    fn overlay_mode_above_max_is_clamped() {
        let mut settings = RendererSettings::default();
        settings.debug_overlay_mode = DEBUG_OVERLAY_MODE_MAX + 77;
        let sanitized = sanitize_renderer_settings(settings);
        assert_eq!(sanitized.debug_overlay_mode, DEBUG_OVERLAY_MODE_MAX);
    }

    #[test]
    fn svgf_history_thresholds_are_clamped_into_expected_range() {
        let mut settings = RendererSettings::default();
        settings.svgf_history_normal_reject_cos = 0.1;
        settings.svgf_history_depth_reject_scale = 2.0;
        let sanitized = sanitize_renderer_settings(settings);
        assert_eq!(sanitized.svgf_history_normal_reject_cos, 0.5);
        assert_eq!(sanitized.svgf_history_depth_reject_scale, 0.5);
    }
}
