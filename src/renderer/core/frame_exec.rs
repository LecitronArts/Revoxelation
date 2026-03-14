use bytemuck::bytes_of;
use egui_wgpu::ScreenDescriptor;

use crate::renderer::camera::PhysicalCamera;
use crate::renderer::core::frame_plan::{FramePlan, build_frame_plan};
use crate::renderer::passes::{
    DEFAULT_WORKGROUP_SIZE, DispatchGrid, PrepareContext as PassPrepareContext,
    RecordContext as PassRecordContext, RenderPass,
};
use crate::renderer::protocol::{CameraGpu, TracerUniform, encode_history_flags};
use crate::renderer::resources::surface::{svgf_atrous_step, svgf_uniform};

use super::state::{
    FrameContext, RenderDiagnosticsSummary, Renderer, RendererDiagEvent, RendererDiagEventSnapshot,
    RendererRuntimeContext, SVGF_MAX_ATROUS_PASSES,
};
use super::world_ops;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RenderExecutionAction {
    Trace,
    ReSTIR,
    SvgfInit,
    SvgfAtrous,
    SvgfResolve,
}

pub(super) type RenderExecutionSummary = RenderDiagnosticsSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RenderDiagnosticsSnapshot {
    pub summary: RenderExecutionSummary,
    pub trace_len: usize,
}

pub(super) fn summarize_render_execution(frame_plan: &FramePlan) -> RenderExecutionSummary {
    let run_svgf = frame_plan.passes.run_svgf;
    let svgf_passes = if run_svgf {
        frame_plan.svgf_passes.min(SVGF_MAX_ATROUS_PASSES)
    } else {
        0
    };

    RenderExecutionSummary {
        run_trace: frame_plan.passes.run_trace,
        run_reistir: frame_plan.passes.run_reistir,
        run_svgf,
        svgf_passes,
    }
}

pub(super) fn build_render_diagnostics(frame_plan: &FramePlan) -> RenderDiagnosticsSnapshot {
    let summary = summarize_render_execution(frame_plan);
    let trace_len = render_execution_trace(summary).len();
    RenderDiagnosticsSnapshot { summary, trace_len }
}

pub(super) fn render_execution_trace(
    summary: RenderExecutionSummary,
) -> Vec<RenderExecutionAction> {
    let mut actions = Vec::new();
    if summary.run_trace {
        actions.push(RenderExecutionAction::Trace);
    }
    if summary.run_reistir {
        actions.push(RenderExecutionAction::ReSTIR);
    }
    if summary.run_svgf {
        actions.push(RenderExecutionAction::SvgfInit);
        for _ in 0..summary.svgf_passes {
            actions.push(RenderExecutionAction::SvgfAtrous);
        }
        actions.push(RenderExecutionAction::SvgfResolve);
    }
    actions
}

pub(super) const fn debug_overlay_flag(settings: &crate::renderer::RendererSettings) -> u32 {
    if settings.debug_overlay {
        settings.debug_overlay_mode
    } else {
        0
    }
}

pub(super) fn render_frame(
    renderer: &mut Renderer,
    camera: &PhysicalCamera,
    paint_jobs: &[egui::ClippedPrimitive],
    textures_delta: &egui::TexturesDelta,
    pixels_per_point: f32,
) -> Result<(), wgpu::SurfaceError> {
    if renderer.gpu.size.width == 0 || renderer.gpu.size.height == 0 {
        return Ok(());
    }

    let frame_plan = build_frame_plan(
        renderer.runtime.frame_bridge.frame_index(),
        renderer.gpu.config.width,
        renderer.gpu.config.height,
        &renderer.runtime.settings,
        SVGF_MAX_ATROUS_PASSES,
    );
    let frame_context = FrameContext::from_plan(&frame_plan);
    let diagnostics = build_render_diagnostics(&frame_plan);
    let execution = diagnostics.summary;
    debug_assert!(
        diagnostics.trace_len >= 2,
        "render trace should include trace + reistir",
    );

    let camera_gpu = camera.to_gpu(
        frame_context.resolution[0],
        frame_context.resolution[1],
        frame_context.frame_index,
    );
    let camera_in_motion = update_motion_state(&mut renderer.runtime, &camera_gpu);
    renderer.runtime.camera_in_motion = camera_in_motion;
    if camera_changed(&renderer.runtime, &camera_gpu) {
        world_ops::reset_accumulation(renderer);
    }
    let previous_camera_gpu = if frame_context.frame_index == 0 {
        camera_gpu
    } else {
        renderer.runtime.last_camera_gpu
    };
    let sun_dir = sun_direction(
        renderer.runtime.settings.sun_yaw_degrees,
        renderer.runtime.settings.sun_pitch_degrees,
    );

    let tracer_uniform = TracerUniform {
        resolution_frame_chunks: [
            frame_context.resolution[0],
            frame_context.resolution[1],
            frame_context.frame_index,
            renderer.runtime.chunk_count.max(1),
        ],
        chunk_map_info: [
            renderer.runtime.chunk_map_size,
            renderer.runtime.chunk_map_mask,
            renderer.runtime.chunk_map_max_probe,
            renderer.runtime.chunk_map_max_probe_observed,
        ],
        emissive_info: [
            renderer.runtime.emissive_count,
            renderer.runtime.emissive_cdf_count,
            renderer.runtime.emissive_remap_count,
            0,
        ],
        importance_info: [
            renderer.runtime.importance_map_dims[0].max(1),
            renderer.runtime.importance_map_dims[1].max(1),
            renderer.runtime.importance_map_dims[2].max(1),
            0,
        ],
        debug_map_stats: [
            renderer.runtime.chunk_map_avg_probe,
            renderer.runtime.chunk_map_max_probe_observed as f32,
            renderer.runtime.chunk_map_load_factor,
            if camera_in_motion { 1.0 } else { 0.0 },
        ],
        world_min: [
            renderer.runtime.world_min[0],
            renderer.runtime.world_min[1],
            renderer.runtime.world_min[2],
            0,
        ],
        world_max: [
            renderer.runtime.world_max[0],
            renderer.runtime.world_max[1],
            renderer.runtime.world_max[2],
            0,
        ],
        integrator: [
            renderer.runtime.settings.max_bounces as f32,
            renderer.runtime.settings.sun_intensity,
            renderer.runtime.settings.exposure,
            renderer.runtime.settings.environment_intensity,
        ],
        sun_dir: [sun_dir[0], sun_dir[1], sun_dir[2], 0.0],
        tuning_a: [
            renderer.runtime.settings.max_history,
            renderer.runtime.settings.rr_start_bounce as f32,
            renderer.runtime.settings.rr_min_survival,
            renderer.runtime.settings.rr_max_survival,
        ],
        tuning_b: [
            renderer.runtime.settings.restir_temporal_boost,
            renderer.runtime.settings.restir_spatial_radius as f32,
            renderer.runtime.settings.dda_max_steps as f32,
            renderer.runtime.settings.restir_gi_directional_gate,
        ],
        tuning_c: [
            renderer.runtime.settings.restir_gi_reuse_m_cap as f32,
            renderer.runtime.settings.restir_gi_reuse_weight_cap,
            renderer.runtime.settings.restir_gi_jacobian_min,
            renderer.runtime.settings.restir_gi_jacobian_max,
        ],
        flags: [
            if renderer.runtime.settings.restir_di_enabled {
                1
            } else {
                0
            },
            debug_overlay_flag(&renderer.runtime.settings),
            if renderer.runtime.settings.restir_gi_enabled {
                1
            } else {
                0
            },
            encode_history_flags(
                frame_context.history_read_slot,
                frame_context.history_write_slot,
            ),
        ],
    };

    renderer
        .gpu
        .queue
        .write_buffer(&renderer.uniforms.camera_buffer, 0, bytes_of(&camera_gpu));
    renderer.gpu.queue.write_buffer(
        &renderer.uniforms.previous_camera_buffer,
        0,
        bytes_of(&previous_camera_gpu),
    );
    renderer.gpu.queue.write_buffer(
        &renderer.uniforms.tracer_uniform_buffer,
        0,
        bytes_of(&tracer_uniform),
    );

    let svgf_init_uniform = svgf_uniform(
        renderer.gpu.config.width,
        renderer.gpu.config.height,
        1,
        frame_context.history_read_slot,
        &renderer.runtime.settings,
    );
    renderer.gpu.queue.write_buffer(
        &renderer.resources.surface.svgf_init_uniform_buffer,
        0,
        bytes_of(&svgf_init_uniform),
    );
    for (index, buffer) in renderer
        .resources
        .surface
        .svgf_atrous_uniform_buffers
        .iter()
        .enumerate()
    {
        let step = svgf_atrous_step(index as u32, renderer.runtime.settings.svgf_step_scale);
        let uniform = svgf_uniform(
            renderer.gpu.config.width,
            renderer.gpu.config.height,
            step,
            frame_plan.svgf_atrous_source_slot(index as u32),
            &renderer.runtime.settings,
        );
        renderer
            .gpu
            .queue
            .write_buffer(buffer, 0, bytes_of(&uniform));
    }
    let svgf_resolve_uniform = svgf_uniform(
        renderer.gpu.config.width,
        renderer.gpu.config.height,
        1,
        frame_plan.svgf_resolve_source_slot,
        &renderer.runtime.settings,
    );
    renderer.gpu.queue.write_buffer(
        &renderer.resources.surface.svgf_resolve_uniform_buffer,
        0,
        bytes_of(&svgf_resolve_uniform),
    );

    let frame = renderer.gpu.surface.get_current_texture()?;
    let frame_view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = renderer
        .gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("trace-command-encoder"),
        });
    let dispatch = DispatchGrid::new(
        frame_context.resolution[0],
        frame_context.resolution[1],
        DEFAULT_WORKGROUP_SIZE[0],
        DEFAULT_WORKGROUP_SIZE[1],
    );

    let prepare_context = PassPrepareContext {
        device: &renderer.gpu.device,
        queue: &renderer.gpu.queue,
        frame: &frame_context,
        dispatch,
        trace_bind_group_ready: true,
        reistir_bind_group_ready: true,
        svgf_init_bind_group_ready: true,
        svgf_resolve_bind_group_ready: true,
        svgf_atrous_bind_group_count: renderer.pipelines.svgf_atrous_bind_groups.len(),
        svgf_passes: execution.svgf_passes,
    };
    if execution.run_trace {
        renderer.pipelines.trace_pass.prepare(&prepare_context);
    }
    if execution.run_reistir {
        renderer.pipelines.reistir_pass.prepare(&prepare_context);
    }
    if execution.run_svgf {
        renderer.pipelines.svgf_pass.prepare(&prepare_context);
    }

    let mut record_context = PassRecordContext {
        encoder: &mut encoder,
        frame: &frame_context,
        dispatch,
        trace_pipeline: &renderer.pipelines.trace_pipeline,
        trace_bind_group: &renderer.pipelines.bind_group,
        reistir_pipeline: &renderer.pipelines.reistir_pipeline,
        reistir_bind_group: &renderer.pipelines.bind_group,
        svgf_init_pipeline: &renderer.pipelines.svgf_init_pipeline,
        svgf_init_bind_group: &renderer.pipelines.svgf_init_bind_group,
        svgf_atrous_pipeline: &renderer.pipelines.svgf_atrous_pipeline,
        svgf_atrous_bind_groups: &renderer.pipelines.svgf_atrous_bind_groups,
        svgf_resolve_pipeline: &renderer.pipelines.svgf_resolve_pipeline,
        svgf_resolve_bind_group: &renderer.pipelines.svgf_resolve_bind_group,
        svgf_passes: execution.svgf_passes,
    };
    if execution.run_trace {
        renderer.pipelines.trace_pass.record(&mut record_context);
    }
    if execution.run_reistir {
        renderer.pipelines.reistir_pass.record(&mut record_context);
    }
    if execution.run_svgf {
        renderer.pipelines.svgf_pass.record(&mut record_context);
    }

    encoder.copy_texture_to_texture(
        wgpu::ImageCopyTexture {
            texture: &renderer.resources.surface.output_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyTexture {
            texture: &frame.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: renderer.gpu.config.width,
            height: renderer.gpu.config.height,
            depth_or_array_layers: 1,
        },
    );

    for (id, image_delta) in &textures_delta.set {
        renderer.pipelines.egui_renderer.update_texture(
            &renderer.gpu.device,
            &renderer.gpu.queue,
            *id,
            image_delta,
        );
    }
    let screen_descriptor = ScreenDescriptor {
        size_in_pixels: [renderer.gpu.config.width, renderer.gpu.config.height],
        pixels_per_point,
    };
    renderer.pipelines.egui_renderer.update_buffers(
        &renderer.gpu.device,
        &renderer.gpu.queue,
        &mut encoder,
        paint_jobs,
        &screen_descriptor,
    );
    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui-render-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        renderer
            .pipelines
            .egui_renderer
            .render(&mut render_pass, paint_jobs, &screen_descriptor);
    }
    for id in &textures_delta.free {
        renderer.pipelines.egui_renderer.free_texture(id);
    }

    renderer.gpu.queue.submit(Some(encoder.finish()));
    frame.present();
    renderer.runtime.last_camera_gpu = camera_gpu;
    renderer.runtime.frame_bridge.advance();
    push_render_diag_event(renderer, frame_context.frame_index, diagnostics);
    Ok(())
}

fn sun_direction(yaw_degrees: f32, pitch_degrees: f32) -> [f32; 3] {
    let yaw = yaw_degrees.to_radians();
    let pitch = pitch_degrees.to_radians();
    let dir = glam::Vec3::new(
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    )
    .normalize_or_zero();
    [dir.x, dir.y, dir.z]
}

fn camera_changed(runtime: &RendererRuntimeContext, current: &CameraGpu) -> bool {
    let current_pos = glam::Vec3::from_array([
        current.position_lens[0],
        current.position_lens[1],
        current.position_lens[2],
    ]);
    let previous_pos = glam::Vec3::from_array([
        runtime.last_camera_gpu.position_lens[0],
        runtime.last_camera_gpu.position_lens[1],
        runtime.last_camera_gpu.position_lens[2],
    ]);
    let position_delta = current_pos.distance(previous_pos);

    let current_forward = glam::Vec3::from_array([
        current.forward_fov[0],
        current.forward_fov[1],
        current.forward_fov[2],
    ])
    .normalize_or_zero();
    let previous_forward = glam::Vec3::from_array([
        runtime.last_camera_gpu.forward_fov[0],
        runtime.last_camera_gpu.forward_fov[1],
        runtime.last_camera_gpu.forward_fov[2],
    ])
    .normalize_or_zero();
    let view_delta = current_forward.dot(previous_forward);

    position_delta > 3.0
        || view_delta < 0.8
        || (current.forward_fov[3] - runtime.last_camera_gpu.forward_fov[3]).abs() > 0.05
        || (current.position_lens[3] - runtime.last_camera_gpu.position_lens[3]).abs() > 1.0e-4
        || (current.up_focus[3] - runtime.last_camera_gpu.up_focus[3]).abs() > 1.0e-3
        || (current.clip_depth[0] - runtime.last_camera_gpu.clip_depth[0]).abs() > 1.0e-5
        || (current.clip_depth[1] - runtime.last_camera_gpu.clip_depth[1]).abs() > 1.0e-3
        || (current.clip_depth[2] - runtime.last_camera_gpu.clip_depth[2]).abs() > 1.0e-4
        || current.resolution_frame[0] != runtime.last_camera_gpu.resolution_frame[0]
        || current.resolution_frame[1] != runtime.last_camera_gpu.resolution_frame[1]
}

fn update_motion_state(runtime: &mut RendererRuntimeContext, current: &CameraGpu) -> bool {
    let current_pos = glam::Vec3::from_array([
        current.position_lens[0],
        current.position_lens[1],
        current.position_lens[2],
    ]);
    let previous_pos = glam::Vec3::from_array([
        runtime.last_camera_gpu.position_lens[0],
        runtime.last_camera_gpu.position_lens[1],
        runtime.last_camera_gpu.position_lens[2],
    ]);
    let position_delta = current_pos.distance(previous_pos);

    let current_forward = glam::Vec3::from_array([
        current.forward_fov[0],
        current.forward_fov[1],
        current.forward_fov[2],
    ])
    .normalize_or_zero();
    let previous_forward = glam::Vec3::from_array([
        runtime.last_camera_gpu.forward_fov[0],
        runtime.last_camera_gpu.forward_fov[1],
        runtime.last_camera_gpu.forward_fov[2],
    ])
    .normalize_or_zero();
    let forward_dot = current_forward.dot(previous_forward);

    let moving = position_delta > 0.01
        || forward_dot < 0.9995
        || (current.forward_fov[3] - runtime.last_camera_gpu.forward_fov[3]).abs() > 0.0005;

    if moving {
        runtime.motion_frames_remaining = 3;
    } else if runtime.motion_frames_remaining > 0 {
        runtime.motion_frames_remaining -= 1;
    }

    runtime.motion_frames_remaining > 0
}

fn push_render_diag_event(
    renderer: &mut Renderer,
    frame_index: u32,
    diagnostics: RenderDiagnosticsSnapshot,
) {
    renderer.runtime.diagnostics.last_render_summary = diagnostics.summary;
    renderer.runtime.diagnostics.last_render_trace_len = diagnostics.trace_len;
    renderer
        .runtime
        .diag_events
        .push(RendererDiagEvent::Render(RendererDiagEventSnapshot {
            frame_index,
            lifecycle_trace: None,
            render_summary: Some(diagnostics.summary),
            render_trace_len: diagnostics.trace_len,
            resource_version_signature: renderer.resources.versions().dependency_signature(),
            world_sync_reject_count: renderer.runtime.world_sync_reject_count,
            last_world_sync_reject_reason: renderer.runtime.last_world_sync_reject_reason.clone(),
            chunk_map_dropped_entries: renderer.runtime.chunk_map_dropped_entries,
        }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::RendererSettings;
    use crate::renderer::core::frame_plan::build_frame_plan;

    fn build_summary(svgf_enabled: bool, svgf_passes: u32) -> RenderExecutionSummary {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = svgf_enabled;
        settings.svgf_passes = svgf_passes;
        let plan = build_frame_plan(0, 1280, 720, &settings, SVGF_MAX_ATROUS_PASSES);
        summarize_render_execution(&plan)
    }

    #[test]
    fn summary_disables_svgf_when_setting_is_off() {
        let summary = build_summary(false, 5);
        assert!(!summary.run_svgf);
        assert_eq!(summary.svgf_passes, 0);
    }

    #[test]
    fn summary_clamps_svgf_passes_to_maximum() {
        let summary = build_summary(true, 64);
        assert!(summary.run_svgf);
        assert_eq!(summary.svgf_passes, SVGF_MAX_ATROUS_PASSES);
    }

    #[test]
    fn trace_without_svgf_contains_trace_then_reistir() {
        let summary = build_summary(false, 3);
        let trace = render_execution_trace(summary);
        assert_eq!(
            trace,
            vec![RenderExecutionAction::Trace, RenderExecutionAction::ReSTIR]
        );
    }

    #[test]
    fn trace_with_zero_svgf_passes_keeps_init_and_resolve_order() {
        let summary = build_summary(true, 0);
        let trace = render_execution_trace(summary);
        assert_eq!(
            trace,
            vec![
                RenderExecutionAction::Trace,
                RenderExecutionAction::ReSTIR,
                RenderExecutionAction::SvgfInit,
                RenderExecutionAction::SvgfResolve,
            ]
        );
    }

    #[test]
    fn trace_with_three_svgf_passes_has_expected_order() {
        let summary = build_summary(true, 3);
        let trace = render_execution_trace(summary);
        assert_eq!(
            trace,
            vec![
                RenderExecutionAction::Trace,
                RenderExecutionAction::ReSTIR,
                RenderExecutionAction::SvgfInit,
                RenderExecutionAction::SvgfAtrous,
                RenderExecutionAction::SvgfAtrous,
                RenderExecutionAction::SvgfAtrous,
                RenderExecutionAction::SvgfResolve,
            ]
        );
    }

    #[test]
    fn trace_length_matches_svgf_pass_count() {
        let summary = build_summary(true, 4);
        let trace = render_execution_trace(summary);
        assert_eq!(trace.len(), 2 + 1 + summary.svgf_passes + 1);
    }

    #[test]
    fn summary_always_runs_trace_and_reistir() {
        let summary = build_summary(true, 2);
        assert!(summary.run_trace);
        assert!(summary.run_reistir);
    }

    #[test]
    fn diagnostics_without_svgf_tracks_summary_and_trace_len() {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = false;
        settings.svgf_passes = 4;
        let plan = build_frame_plan(0, 1280, 720, &settings, SVGF_MAX_ATROUS_PASSES);
        let diagnostics = build_render_diagnostics(&plan);
        assert_eq!(
            diagnostics.summary,
            RenderExecutionSummary {
                run_trace: true,
                run_reistir: true,
                run_svgf: false,
                svgf_passes: 0,
            }
        );
        assert_eq!(diagnostics.trace_len, 2);
    }

    #[test]
    fn diagnostics_with_svgf_tracks_summary_and_trace_len() {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = true;
        settings.svgf_passes = 2;
        let plan = build_frame_plan(1, 800, 600, &settings, SVGF_MAX_ATROUS_PASSES);
        let diagnostics = build_render_diagnostics(&plan);
        assert_eq!(
            diagnostics.summary,
            RenderExecutionSummary {
                run_trace: true,
                run_reistir: true,
                run_svgf: true,
                svgf_passes: 2,
            }
        );
        assert_eq!(diagnostics.trace_len, 6);
    }

    #[test]
    fn diagnostics_trace_len_respects_svgf_clamp() {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = true;
        settings.svgf_passes = 64;
        let plan = build_frame_plan(2, 640, 480, &settings, SVGF_MAX_ATROUS_PASSES);
        let diagnostics = build_render_diagnostics(&plan);
        assert_eq!(diagnostics.summary.svgf_passes, SVGF_MAX_ATROUS_PASSES);
        assert_eq!(diagnostics.trace_len, 2 + 1 + SVGF_MAX_ATROUS_PASSES + 1);
    }

    #[test]
    fn diagnostics_trace_len_matches_render_trace_output() {
        let summary = build_summary(true, 3);
        let trace = render_execution_trace(summary);
        let plan = build_frame_plan(
            3,
            1920,
            1080,
            &RendererSettings {
                svgf_enabled: true,
                svgf_passes: 3,
                ..RendererSettings::default()
            },
            SVGF_MAX_ATROUS_PASSES,
        );
        let diagnostics = build_render_diagnostics(&plan);
        assert_eq!(trace.len(), diagnostics.trace_len);
    }

    #[test]
    fn debug_overlay_flag_is_zero_when_overlay_disabled() {
        let mut settings = RendererSettings::default();
        settings.debug_overlay = false;
        settings.debug_overlay_mode = 7;
        assert_eq!(debug_overlay_flag(&settings), 0);
    }

    #[test]
    fn debug_overlay_flag_preserves_disabled_mode_zero_when_enabled() {
        let mut settings = RendererSettings::default();
        settings.debug_overlay = true;
        settings.debug_overlay_mode = 0;
        assert_eq!(debug_overlay_flag(&settings), 0);
    }

    #[test]
    fn schedule_guard_svgf_disabled_stays_trace_then_reistir() {
        let summary = build_summary(false, 5);
        assert_eq!(
            render_execution_trace(summary),
            vec![RenderExecutionAction::Trace, RenderExecutionAction::ReSTIR]
        );
    }

    #[test]
    fn schedule_guard_svgf_zero_pass_has_only_init_and_resolve() {
        let summary = build_summary(true, 0);
        let trace = render_execution_trace(summary);
        assert_eq!(
            trace,
            vec![
                RenderExecutionAction::Trace,
                RenderExecutionAction::ReSTIR,
                RenderExecutionAction::SvgfInit,
                RenderExecutionAction::SvgfResolve,
            ]
        );
    }

    #[test]
    fn schedule_guard_svgf_multi_pass_counts_atrous_instances() {
        let summary = build_summary(true, 4);
        let atrous_count = render_execution_trace(summary)
            .into_iter()
            .filter(|action| *action == RenderExecutionAction::SvgfAtrous)
            .count();
        assert_eq!(atrous_count, 4);
    }
}
