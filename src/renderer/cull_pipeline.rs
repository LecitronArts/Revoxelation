use anyhow::{Context, Result};
use ash::vk;

use super::Renderer;

pub struct ChunkCullPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
}

impl ChunkCullPipeline {
    pub fn new(renderer: &Renderer) -> Result<Self> {
        let device = &renderer.device_ctx.device;
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
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
        })
    }

    pub fn dispatch(&self, renderer: &Renderer, cmd: vk::CommandBuffer, active_chunk_count: u32) {
        if active_chunk_count == 0 {
            return;
        }
        let group_count = active_chunk_count.div_ceil(64);
        unsafe {
            renderer.device_ctx.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.pipeline,
            );
            renderer
                .device_ctx
                .device
                .cmd_dispatch(cmd, group_count, 1, 1);
        }
    }

    pub fn destroy(self, renderer: &Renderer) {
        unsafe {
            renderer.device_ctx.device.destroy_pipeline(self.pipeline, None);
            renderer
                .device_ctx
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}

fn create_shader_module(device: &ash::Device, bytes: &[u8]) -> Result<vk::ShaderModule> {
    let code = bytemuck::cast_slice(bytes);
    unsafe {
        device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(code), None)
            .context("failed to create shader module")
    }
}
