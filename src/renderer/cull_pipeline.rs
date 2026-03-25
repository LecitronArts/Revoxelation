use anyhow::{Context, Result};
use ash::vk;

use super::{Renderer, spirv::create_shader_module};

pub struct ChunkCullPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
}

pub fn cull_descriptor_layout_bindings() -> [vk::DescriptorSetLayoutBinding<'static>; 4] {
    [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        vk::DescriptorSetLayoutBinding::default()
            .binding(2)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        vk::DescriptorSetLayoutBinding::default()
            .binding(3)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
    ]
}

impl ChunkCullPipeline {
    pub fn new(renderer: &Renderer) -> Result<Self> {
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
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(bindings[0].binding)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&metadata_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(bindings[1].binding)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&indirect_template_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(bindings[2].binding)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&draw_slot_buffer_info),
            vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(bindings[3].binding)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&dense_indirect_buffer_info),
        ];
        unsafe {
            device.update_descriptor_sets(&writes, &[]);
        }
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().set_layouts(&descriptor_set_layouts),
                    None,
                )
                .context("failed to create chunk cull pipeline layout")?
        };
        let shader_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/chunk_cull.comp.spv")),
        )?;
        let entry_name = c"main";
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .module(shader_module)
            .name(entry_name)
            .stage(vk::ShaderStageFlags::COMPUTE);
        let pipeline = unsafe {
            device
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
            device.destroy_shader_module(shader_module, None);
        }

        Ok(Self {
            pipeline,
            pipeline_layout,
            descriptor_pool,
            descriptor_set_layout,
            descriptor_set,
        })
    }

    pub fn dispatch(&self, renderer: &Renderer, cmd: vk::CommandBuffer, active_draw_count: u32) {
        if active_draw_count == 0 {
            return;
        }
        unsafe {
            renderer.device_ctx.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline,
            );
            renderer.device_ctx.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline_layout,
                0,
                &[self.descriptor_set],
                &[],
            );
            renderer
                .device_ctx
                .device
                .cmd_dispatch(cmd, active_draw_count, 1, 1);
        }
    }

    pub fn destroy(self, renderer: &Renderer) {
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
    }
}
