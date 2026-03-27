use std::mem::size_of;

use anyhow::{Context, Result};
use ash::vk;
use gpu_allocator::{MemoryLocation, vulkan::{Allocation, AllocationScheme}};

use super::{Renderer, create_allocated_buffer, destroy_allocated_buffer, spirv::create_shader_module};
use super::camera::FrustumPlanes;

/// Workgroup size matching the compute shader's `local_size_x`.
const WORKGROUP_SIZE: u32 = 64;

/// Hi-Z config data uploaded to the GPU SSBO (binding 6).
///
/// Layout must match the GLSL `HiZConfigBuffer` struct in chunk_cull.comp.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HiZConfig {
    pub view_proj: [[f32; 4]; 4], // mat4
    pub hiz_size: [f32; 2],       // vec2 (hiz width, height)
    pub hiz_enabled: u32,         // 1 = enabled, 0 = disabled
    pub hiz_mip_count: u32,       // number of mip levels
}

pub struct ChunkCullPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    /// Small SSBO holding 6 frustum planes (96 bytes). CpuToGpu for easy per-frame update.
    pub frustum_planes_buffer: vk::Buffer,
    pub frustum_planes_allocation: Option<Allocation>,
    /// Mapped pointer to the frustum planes buffer for fast CPU writes.
    frustum_planes_mapped: *mut u8,
    /// Draw count buffer (4 bytes) — GpuOnly, reset via vkCmdFillBuffer each frame.
    pub draw_count_buffer: vk::Buffer,
    pub draw_count_allocation: Option<Allocation>,
    /// Hi-Z config SSBO (binding 6). CpuToGpu for per-frame update.
    pub hiz_config_buffer: vk::Buffer,
    pub hiz_config_allocation: Option<Allocation>,
    hiz_config_mapped: *mut u8,
}

// SAFETY: ChunkCullPipeline's `frustum_planes_mapped` and `hiz_config_mapped` (*mut u8) point
// into gpu-allocator CpuToGpu mapped memory. Send is safe because: (1) writes use
// copy_nonoverlapping to disjoint regions, (2) uploads happen before cmd submit (no concurrent GPU
// read), (3) pipeline is owned by Renderer and only accessed from the main render thread.
unsafe impl Send for ChunkCullPipeline {}

impl ChunkCullPipeline {
    /// Create the cull pipeline. Uses the shared bindless descriptor set layout
    /// from BindlessTable instead of creating its own descriptor infrastructure.
    pub fn new(renderer: &mut Renderer, bindless_layout: vk::DescriptorSetLayout) -> Result<Self> {
        let _device = &renderer.device_ctx.device;

        // Create frustum planes buffer (96 bytes, CpuToGpu for easy per-frame update)
        let (frustum_planes_buffer, frustum_planes_allocation) = create_allocated_buffer(
            renderer,
            size_of::<FrustumPlanes>() as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "cull-frustum-planes",
        )?;
        let frustum_planes_mapped = frustum_planes_allocation
            .mapped_ptr()
            .map(|p| p.as_ptr() as *mut u8)
            .unwrap_or(std::ptr::null_mut());

        // Create draw count buffer (4 bytes, GpuOnly with TRANSFER_DST for vkCmdFillBuffer reset)
        let (draw_count_buffer, draw_count_allocation) = create_allocated_buffer(
            renderer,
            size_of::<u32>() as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "cull-draw-count",
        )?;

        // Create Hi-Z config buffer (CpuToGpu for per-frame update)
        let (hiz_config_buffer, hiz_config_allocation) = create_allocated_buffer(
            renderer,
            size_of::<HiZConfig>() as u64,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "cull-hiz-config",
        )?;
        let hiz_config_mapped = hiz_config_allocation
            .mapped_ptr()
            .map(|p| p.as_ptr() as *mut u8)
            .unwrap_or(std::ptr::null_mut());

        // Push constant range for { active_draw_count: u32, capacity: u32 } = 8 bytes (D-08)
        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size((size_of::<u32>() * 2) as u32)];

        // Pipeline layout uses the shared bindless set 0 layout — D-07
        let set_layouts = [bindless_layout];
        let pipeline_layout = unsafe {
            renderer
                .device_ctx
                .device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&set_layouts)
                        .push_constant_ranges(&push_constant_ranges),
                    None,
                )
                .context("failed to create chunk cull pipeline layout")?
        };
        let shader_module = create_shader_module(
            &renderer.device_ctx.device,
            include_bytes!(concat!(env!("OUT_DIR"), "/chunk_cull.comp.spv")),
        )?;
        let entry_name = c"main";
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .module(shader_module)
            .name(entry_name)
            .stage(vk::ShaderStageFlags::COMPUTE);
        let cache_handle = renderer
            .pipeline_cache
            .as_ref()
            .expect("pipeline cache must be initialized before cull pipeline")
            .handle();
        let pipeline = unsafe {
            renderer
                .device_ctx
                .device
                .create_compute_pipelines(
                    cache_handle,
                    &[vk::ComputePipelineCreateInfo::default()
                        .stage(stage)
                        .layout(pipeline_layout)],
                    None,
                )
                .map_err(|(_, err)| err)
                .context("failed to create chunk cull compute pipeline")?
                .into_iter()
                .next()
                .context("compute pipeline creation returned no pipeline")?
        };

        unsafe {
            renderer.device_ctx.device.destroy_shader_module(shader_module, None);
        }

        Ok(Self {
            pipeline,
            pipeline_layout,
            frustum_planes_buffer,
            frustum_planes_allocation: Some(frustum_planes_allocation),
            frustum_planes_mapped,
            draw_count_buffer,
            draw_count_allocation: Some(draw_count_allocation),
            hiz_config_buffer,
            hiz_config_allocation: Some(hiz_config_allocation),
            hiz_config_mapped,
        })
    }

    /// Upload frustum planes to the GPU SSBO (direct mapped write, CpuToGpu buffer).
    pub fn upload_frustum_planes(&self, planes: &FrustumPlanes) {
        if self.frustum_planes_mapped.is_null() {
            return;
        }
        let src = planes as *const FrustumPlanes as *const u8;
        unsafe {
            std::ptr::copy_nonoverlapping(src, self.frustum_planes_mapped, size_of::<FrustumPlanes>());
        }
    }

    /// Upload Hi-Z config to the GPU SSBO (direct mapped write, CpuToGpu buffer).
    pub fn upload_hiz_config(&self, config: &HiZConfig) {
        if self.hiz_config_mapped.is_null() {
            return;
        }
        let src = config as *const HiZConfig as *const u8;
        unsafe {
            std::ptr::copy_nonoverlapping(src, self.hiz_config_mapped, size_of::<HiZConfig>());
        }
    }

    /// Return the draw count buffer handle (for barriers and IndirectCount draw).
    pub fn draw_count_buffer(&self) -> vk::Buffer {
        self.draw_count_buffer
    }

    /// Dispatch the cull compute shader. Uses the shared bindless descriptor set.
    pub fn dispatch(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        active_draw_count: u32,
        capacity: u32,
        frustum_planes: &FrustumPlanes,
        bindless_set: vk::DescriptorSet,
    ) {
        if active_draw_count == 0 {
            return;
        }

        // Upload frustum planes to the SSBO.
        self.upload_frustum_planes(frustum_planes);

        // Reset draw count buffer to 0 via vkCmdFillBuffer.
        unsafe {
            device.cmd_fill_buffer(
                cmd,
                self.draw_count_buffer,
                0,
                size_of::<u32>() as u64,
                0,
            );

            // Barrier: TRANSFER_WRITE → SHADER_READ|SHADER_WRITE for draw count buffer.
            let fill_barrier = vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE)
                .buffer(self.draw_count_buffer)
                .offset(0)
                .size(size_of::<u32>() as u64);
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[fill_barrier],
                &[],
            );

            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline);
            // Bind the shared bindless descriptor set 0 — D-08
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &[bindless_set],
                &[],
            );

            // Push constants: { active_draw_count, capacity } (D-08)
            let pc_data: [u32; 2] = [active_draw_count, capacity];
            let pc_bytes = bytemuck::cast_slice(&pc_data);
            device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                pc_bytes,            );

            // Dispatch ceil(active_draw_count / 64) workgroups.
            let group_count = active_draw_count.div_ceil(WORKGROUP_SIZE);
            device.cmd_dispatch(cmd, group_count, 1, 1);
        }
    }

    pub fn destroy(mut self, renderer: &mut Renderer) {
        unsafe {
            renderer.device_ctx.device.destroy_pipeline(self.pipeline, None);
            renderer
                .device_ctx
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }
        if let Some(allocation) = self.frustum_planes_allocation.take() {
            let _ = destroy_allocated_buffer(renderer, self.frustum_planes_buffer, allocation);
        }
        if let Some(allocation) = self.draw_count_allocation.take() {
            let _ = destroy_allocated_buffer(renderer, self.draw_count_buffer, allocation);
        }
        if let Some(allocation) = self.hiz_config_allocation.take() {
            let _ = destroy_allocated_buffer(renderer, self.hiz_config_buffer, allocation);
        }
    }
}
