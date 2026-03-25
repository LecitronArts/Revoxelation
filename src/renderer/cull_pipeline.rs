use std::mem::size_of;

use anyhow::{Context, Result};
use ash::vk;
use gpu_allocator::{MemoryLocation, vulkan::{Allocation, AllocationScheme}};

use super::{Renderer, create_allocated_buffer, destroy_allocated_buffer, spirv::create_shader_module};
use super::camera::FrustumPlanes;

/// Workgroup size matching the compute shader's `local_size_x`.
const WORKGROUP_SIZE: u32 = 64;

pub struct ChunkCullPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
    /// Small SSBO holding 6 frustum planes (96 bytes). CpuToGpu for easy per-frame update.
    pub frustum_planes_buffer: vk::Buffer,
    pub frustum_planes_allocation: Option<Allocation>,
    /// Mapped pointer to the frustum planes buffer for fast CPU writes.
    frustum_planes_mapped: *mut u8,
    /// Draw count buffer (4 bytes) — GpuOnly, reset via vkCmdFillBuffer each frame.
    pub draw_count_buffer: vk::Buffer,
    pub draw_count_allocation: Option<Allocation>,
}

// Safety: raw pointer is only used for mapped writes from the main thread.
unsafe impl Send for ChunkCullPipeline {}

pub fn cull_descriptor_layout_bindings() -> [vk::DescriptorSetLayoutBinding<'static>; 6] {
    [
        // binding 0: chunk metadata (read)
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        // binding 1: indirect templates (read)
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        // binding 2: draw slots (read)
        vk::DescriptorSetLayoutBinding::default()
            .binding(2)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        // binding 3: output dense indirect (write)
        vk::DescriptorSetLayoutBinding::default()
            .binding(3)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        // binding 4: frustum planes (read) — 96 bytes
        vk::DescriptorSetLayoutBinding::default()
            .binding(4)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        // binding 5: draw count (read+write) — 4 bytes
        vk::DescriptorSetLayoutBinding::default()
            .binding(5)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
    ]
}

impl ChunkCullPipeline {
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let device = &renderer.device_ctx.device;
        let bindings = cull_descriptor_layout_bindings();
        let descriptor_set_layout = unsafe {
            device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .context("failed to create chunk cull descriptor set layout")?
        };
        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(bindings.len() as u32)];
        let descriptor_pool = unsafe {
            device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .pool_sizes(&pool_sizes)
                        .max_sets(1),
                    None,
                )
                .context("failed to create chunk cull descriptor pool")?
        };
        let descriptor_set_layouts = [descriptor_set_layout];
        let descriptor_set = unsafe {
            device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(descriptor_pool)
                        .set_layouts(&descriptor_set_layouts),
                )
                .context("failed to allocate chunk cull descriptor set")?
                .into_iter()
                .next()
                .context("chunk cull descriptor allocation returned no descriptor sets")?
        };

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

        let chunk_pool = renderer
            .chunk_pool
            .as_ref()
            .context("chunk cull pipeline requires a chunk pool before initialization")?;

        let metadata_buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(chunk_pool.metadata_buffer())
            .offset(0)
            .range(vk::WHOLE_SIZE)];
        let indirect_template_buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(chunk_pool.indirect_template_buffer())
            .offset(0)
            .range(vk::WHOLE_SIZE)];
        let draw_slot_buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(chunk_pool.draw_slot_buffer())
            .offset(0)
            .range(vk::WHOLE_SIZE)];
        let dense_indirect_buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(chunk_pool.dense_indirect_buffer())
            .offset(0)
            .range(vk::WHOLE_SIZE)];
        let frustum_planes_buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(frustum_planes_buffer)
            .offset(0)
            .range(size_of::<FrustumPlanes>() as u64)];
        let draw_count_buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(draw_count_buffer)
            .offset(0)
            .range(size_of::<u32>() as u64)];
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&metadata_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&indirect_template_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(2)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&draw_slot_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(3)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&dense_indirect_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(4)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&frustum_planes_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(5)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&draw_count_buffer_info),
        ];
        unsafe {
            renderer.device_ctx.device.update_descriptor_sets(&writes, &[]);
        }

        // Push constant range for active_draw_count (u32 = 4 bytes)
        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(size_of::<u32>() as u32)];

        let pipeline_layout = unsafe {
            renderer
                .device_ctx
                .device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&descriptor_set_layouts)
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
        let pipeline = unsafe {
            renderer
                .device_ctx
                .device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
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
            descriptor_pool,
            descriptor_set_layout,
            descriptor_set,
            frustum_planes_buffer,
            frustum_planes_allocation: Some(frustum_planes_allocation),
            frustum_planes_mapped,
            draw_count_buffer,
            draw_count_allocation: Some(draw_count_allocation),
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

    /// Return the draw count buffer handle (for barriers and IndirectCount draw).
    pub fn draw_count_buffer(&self) -> vk::Buffer {
        self.draw_count_buffer
    }

    pub fn dispatch(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        active_draw_count: u32,
        frustum_planes: &FrustumPlanes,
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
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &[self.descriptor_set],
                &[],
            );

            // Push constant: active_draw_count
            let pc_bytes = active_draw_count.to_ne_bytes();
            device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                &pc_bytes,
            );

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
            renderer
                .device_ctx
                .device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            renderer
                .device_ctx
                .device
                .destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        }
        if let Some(allocation) = self.frustum_planes_allocation.take() {
            let _ = destroy_allocated_buffer(renderer, self.frustum_planes_buffer, allocation);
        }
        if let Some(allocation) = self.draw_count_allocation.take() {
            let _ = destroy_allocated_buffer(renderer, self.draw_count_buffer, allocation);
        }
    }
}
