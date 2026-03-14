mod compute_pipelines;
mod device_setup;
mod pipeline_layouts;
mod pipeline_setup;
mod resource_setup;
mod shader_modules;

use std::sync::Arc;

use anyhow::Result;

use crate::renderer::passes::reistir::ReSTIRPass;
use crate::renderer::passes::svgf::SvgfPass;
use crate::renderer::passes::trace::TracePass;
use crate::renderer::resources::bind_groups::{RebuiltBindGroups, rebuild_bind_groups};
use crate::renderer::resources::restir_storage::FrameBridge;
use crate::world::VoxelWorld;

use super::state::{
    Renderer, RendererDiagEventRing, RendererDiagnosticsState, RendererGpuContext,
    RendererPipelineContext, RendererRuntimeContext,
};

use device_setup::setup_device;
use pipeline_setup::setup_pipelines;
use resource_setup::setup_initial_resources;

pub(super) async fn bootstrap_renderer(
    window: Arc<winit::window::Window>,
    world: Arc<VoxelWorld>,
) -> Result<Renderer> {
    let device_setup = setup_device(window).await?;
    let resources_setup = setup_initial_resources(
        &device_setup.device,
        &device_setup.queue,
        &device_setup.config,
        device_setup.max_storage_binding_size,
        device_setup.output_format,
    );

    let pipeline_setup = setup_pipelines(&device_setup.device, device_setup.output_format);

    let RebuiltBindGroups {
        trace_bind_group: bind_group,
        svgf_init_bind_group,
        svgf_atrous_bind_groups,
        svgf_resolve_bind_group,
    } = rebuild_bind_groups(resources_setup.resources.bind_group_input(
        &device_setup.device,
        &pipeline_setup.bind_group_layout,
        &pipeline_setup.svgf_bind_group_layout,
        &resources_setup.uniforms.camera_buffer,
        &resources_setup.uniforms.previous_camera_buffer,
        &resources_setup.uniforms.tracer_uniform_buffer,
    ));

    let egui_renderer =
        egui_wgpu::Renderer::new(&device_setup.device, device_setup.config.format, None, 1);

    let mut renderer = Renderer {
        gpu: RendererGpuContext {
            surface: device_setup.surface,
            device: device_setup.device,
            queue: device_setup.queue,
            max_storage_binding_size: device_setup.max_storage_binding_size,
            config: device_setup.config,
            size: device_setup.size,
            output_format: device_setup.output_format,
        },
        pipelines: RendererPipelineContext {
            trace_pipeline: pipeline_setup.trace_pipeline,
            reistir_pipeline: pipeline_setup.reistir_pipeline,
            bind_group_layout: pipeline_setup.bind_group_layout,
            bind_group,
            svgf_init_pipeline: pipeline_setup.svgf_init_pipeline,
            svgf_atrous_pipeline: pipeline_setup.svgf_atrous_pipeline,
            svgf_resolve_pipeline: pipeline_setup.svgf_resolve_pipeline,
            svgf_diag_reduce_pipeline: pipeline_setup.svgf_diag_reduce_pipeline,
            svgf_bind_group_layout: pipeline_setup.svgf_bind_group_layout,
            svgf_init_bind_group,
            svgf_atrous_bind_groups,
            svgf_resolve_bind_group,
            egui_renderer,
            trace_pass: TracePass::default(),
            reistir_pass: ReSTIRPass::default(),
            svgf_pass: SvgfPass::default(),
        },
        uniforms: resources_setup.uniforms,
        resources: resources_setup.resources,
        runtime: RendererRuntimeContext {
            importance_map_dims: [1, 1, 1],
            chunk_count: 1,
            chunk_map_size: 1,
            chunk_map_mask: 0,
            chunk_map_max_probe: 1,
            chunk_map_avg_probe: 0.0,
            chunk_map_max_probe_observed: 0,
            chunk_map_load_factor: 0.0,
            chunk_map_dropped_entries: 0,
            emissive_count: 0,
            emissive_cdf_count: 1,
            emissive_remap_count: 1,
            emissive_signatures: vec![0],
            world_min: [-64, -64, -64],
            world_max: [64, 64, 64],
            settings: resources_setup.settings,
            world_sync_reject_count: 0,
            last_world_sync_reject_reason: String::new(),
            frame_bridge: FrameBridge::default(),
            motion_frames_remaining: 0,
            camera_in_motion: false,
            last_camera_gpu: crate::renderer::protocol::CameraGpu::default(),
            last_svgf_diagnostics: None,
            svgf_diag_readback_slots: std::array::from_fn(|_| Default::default()),
            svgf_diag_next_copy_slot: 0,
            svgf_diag_surface_generation: 0,
            diagnostics: RendererDiagnosticsState::default(),
            diag_events: RendererDiagEventRing::default(),
        },
    };

    super::world_ops::sync_world(&mut renderer, &world);
    Ok(renderer)
}
