use bytemuck::bytes_of;
use wgpu::util::DeviceExt;

use crate::renderer::light_sampler::INVALID_EMITTER_INDEX;
use crate::renderer::protocol::{
    CameraGpu, ChunkMapEntryGpu, ChunkMetaGpu, EmissiveVoxelGpu, TracerUniform,
};
use crate::renderer::resources::context::{
    RendererResourceContext, SurfaceHistoryResources, WorldUploadResources,
};
use crate::renderer::resources::surface::{
    RebuiltSurfaceResources, build_surface_resource_state,
    rebuild_surface_resources as rebuild_surface_gpu_resources,
};
use crate::renderer::world::upload::{UploadedWorldResources, create_importance_map_texture};

use super::super::state::{RendererSettings, RendererUniformContext};

pub(super) struct InitialResourceSetup {
    pub settings: RendererSettings,
    pub resources: RendererResourceContext,
    pub uniforms: RendererUniformContext,
}

pub(super) fn setup_initial_resources(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    config: &wgpu::SurfaceConfiguration,
    max_storage_binding_size: u64,
    output_format: wgpu::TextureFormat,
) -> InitialResourceSetup {
    let settings = RendererSettings::default();
    let surface_state = build_surface_resource_state(
        config.width,
        config.height,
        max_storage_binding_size,
        &settings,
    );
    let RebuiltSurfaceResources {
        output_texture,
        output_view,
        accumulation_buffer,
        restir_storage,
        svgf_ping_buffer,
        svgf_pong_buffer,
        svgf_debug_buffer,
        svgf_init_uniform_buffer,
        svgf_resolve_uniform_buffer,
        svgf_atrous_uniform_buffers,
    } = rebuild_surface_gpu_resources(device, surface_state, output_format, &settings);

    let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("camera-uniform-buffer"),
        contents: bytes_of(&CameraGpu::default()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let previous_camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("previous-camera-uniform-buffer"),
        contents: bytes_of(&CameraGpu::default()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let tracer_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("tracer-uniform-buffer"),
        contents: bytes_of(&TracerUniform::default()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let voxel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("voxel-buffer"),
        contents: bytes_of(&0_u32),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let chunk_meta_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chunk-meta-buffer"),
        contents: bytes_of(&ChunkMetaGpu::empty()),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let chunk_map_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chunk-map-buffer"),
        contents: bytes_of(&ChunkMapEntryGpu::empty()),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let emissive_voxel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("emissive-voxel-buffer"),
        contents: bytes_of(&EmissiveVoxelGpu::empty()),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let emissive_cdf_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("emissive-cdf-buffer"),
        contents: bytes_of(&1.0f32),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let emissive_remap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("emissive-remap-buffer"),
        contents: bytes_of(&INVALID_EMITTER_INDEX),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    let (importance_map_texture, importance_map_view) =
        create_importance_map_texture(device, queue, [1, 1, 1], &[0.0]);

    let world_resources = WorldUploadResources::from(UploadedWorldResources {
        voxel_buffer,
        chunk_meta_buffer,
        chunk_map_buffer,
        emissive_voxel_buffer,
        emissive_cdf_buffer,
        emissive_remap_buffer,
        importance_map_texture,
        importance_map_view,
    });
    let surface_resources = SurfaceHistoryResources {
        output_texture,
        output_view,
        accumulation_buffer,
        restir_storage,
        svgf_ping_buffer,
        svgf_pong_buffer,
        svgf_debug_buffer,
        svgf_init_uniform_buffer,
        svgf_resolve_uniform_buffer,
        svgf_atrous_uniform_buffers,
    };

    let resources = RendererResourceContext::new(world_resources, surface_resources);
    let uniforms = RendererUniformContext {
        camera_buffer,
        previous_camera_buffer,
        tracer_uniform_buffer,
    };

    InitialResourceSetup {
        settings,
        resources,
        uniforms,
    }
}
