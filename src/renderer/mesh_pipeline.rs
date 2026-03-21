use anyhow::{Context, Result};
use ash::vk;

use super::{Renderer, chunk_pool::ChunkPool};

pub struct ChunkMeshPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
}

impl ChunkMeshPipeline {
    pub fn new(renderer: &Renderer) -> Result<Self> {
        let device = &renderer.device_ctx.device;
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
                .context("failed to create chunk mesh pipeline layout")?
        };

        let vert_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/chunk_mesh.vert.spv")),
        )?;
        let frag_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/chunk_mesh.frag.spv")),
        )?;

        let entry_name = c"main";
        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .module(vert_module)
                .name(entry_name)
                .stage(vk::ShaderStageFlags::VERTEX),
            vk::PipelineShaderStageCreateInfo::default()
                .module(frag_module)
                .name(entry_name)
                .stage(vk::ShaderStageFlags::FRAGMENT),
        ];
        let vertex_bindings = [vk::VertexInputBindingDescription {
            binding: 0,
            stride: 8,
            input_rate: vk::VertexInputRate::VERTEX,
        }];
        let vertex_attributes = [vk::VertexInputAttributeDescription {
            location: 0,
            binding: 0,
            format: vk::Format::R32G32_UINT,
            offset: 0,
        }];
        let vertex_input_state = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&vertex_bindings)
            .vertex_attribute_descriptions(&vertex_attributes);
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::Viewport::default()
            .x(0.0)
            .y(0.0)
            .width(renderer.swapchain_ctx.extent.width as f32)
            .height(renderer.swapchain_ctx.extent.height as f32)
            .min_depth(0.0)
            .max_depth(1.0);
        let scissor = vk::Rect2D::default().extent(renderer.swapchain_ctx.extent);
        let viewports = [viewport];
        let scissors = [scissor];
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewports(&viewports)
            .scissors(&scissors);
        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::BACK)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(false)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let color_blend = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(&color_blend_attachment);
        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_state)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .color_blend_state(&color_blend)
            .layout(pipeline_layout)
            .render_pass(renderer.swapchain_ctx.render_pass)
            .subpass(0)];
        let pipeline = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
                .map_err(|(_, err)| err)
                .context("failed to create chunk mesh graphics pipeline")?
                .into_iter()
                .next()
                .context("graphics pipeline creation returned no pipeline")?
        };

        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        Ok(Self {
            pipeline,
            pipeline_layout,
        })
    }

    pub fn draw(
        &self,
        renderer: &Renderer,
        chunk_pool: &ChunkPool,
        cmd: vk::CommandBuffer,
        draw_count: u32,
    ) {
        let vertex_buffers = [chunk_pool.vertex_buffer()];
        let vertex_offsets = [0];
        unsafe {
            renderer.device_ctx.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );
            renderer.device_ctx.device.cmd_bind_vertex_buffers(
                cmd,
                0,
                &vertex_buffers,
                &vertex_offsets,
            );
            renderer.device_ctx.device.cmd_bind_index_buffer(
                cmd,
                chunk_pool.index_buffer(),
                0,
                vk::IndexType::UINT32,
            );
            renderer.device_ctx.device.cmd_draw_indexed_indirect(
                cmd,
                chunk_pool.indirect_buffer(),
                0,
                draw_count,
                std::mem::size_of::<vk::DrawIndexedIndirectCommand>() as u32,
            );
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
