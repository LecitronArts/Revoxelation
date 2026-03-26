use anyhow::{Context, Result};
use ash::vk;

use super::{Renderer, chunk_pool::ChunkPool, spirv::create_shader_module};
use super::camera::CameraUniforms;

pub struct ChunkMeshPipeline {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
}

impl ChunkMeshPipeline {
    /// Create the mesh pipeline. Uses the shared bindless descriptor set layout
    /// from BindlessTable instead of creating its own descriptor infrastructure.
    pub fn new(renderer: &Renderer, bindless_layout: vk::DescriptorSetLayout) -> Result<Self> {
        let device = &renderer.device_ctx.device;

        // Push constant range for CameraUniforms (80 bytes, VERTEX stage) — D-06
        let push_constant_ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX,
            offset: 0,
            size: std::mem::size_of::<CameraUniforms>() as u32, // 80 bytes
        }];

        // Pipeline layout uses the shared bindless set 0 layout — D-07
        let set_layouts = [bindless_layout];
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&set_layouts)
                        .push_constant_ranges(&push_constant_ranges),
                    None,
                )
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
        // Dynamic viewport and scissor — not baked into the pipeline.
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(false)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let color_blend = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(&color_blend_attachment);
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS);
        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_state)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .color_blend_state(&color_blend)
            .depth_stencil_state(&depth_stencil)
            .dynamic_state(&dynamic_state_info)
            .layout(pipeline_layout)
            .render_pass(renderer.swapchain_ctx.render_pass)
            .subpass(0)];
        let cache_handle = renderer
            .pipeline_cache
            .as_ref()
            .expect("pipeline cache must be initialized before mesh pipeline")
            .handle();
        let pipeline = unsafe {
            device
                .create_graphics_pipelines(cache_handle, &pipeline_info, None)
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

    /// Draw chunks using the shared bindless descriptor set.
    ///
    /// Uses `vkCmdDrawIndexedIndirectCount` (Vulkan 1.2 core) so the GPU cull shader
    /// determines the actual draw count. `max_draw_count` is the pool capacity and
    /// `draw_count_buffer` holds the GPU-written visible-chunk count (D-06, D-09).
    pub fn draw(
        &self,
        renderer: &Renderer,
        chunk_pool: &ChunkPool,
        cmd: vk::CommandBuffer,
        max_draw_count: u32,
        draw_count_buffer: vk::Buffer,
        camera_uniforms: &CameraUniforms,
        bindless_set: vk::DescriptorSet,
    ) {
        let extent = renderer.swapchain_ctx.extent;
        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: extent.width as f32,
            height: extent.height as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent,
        };
        let vertex_buffers = [chunk_pool.vertex_buffer()];
        let vertex_offsets = [0];
        unsafe {
            renderer.device_ctx.device.cmd_bind_pipeline(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline,
            );
            renderer.device_ctx.device.cmd_set_viewport(cmd, 0, &[viewport]);
            renderer.device_ctx.device.cmd_set_scissor(cmd, 0, &[scissor]);
            renderer.device_ctx.device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(camera_uniforms),
            );
            // Bind the shared bindless descriptor set 0 — D-08
            renderer.device_ctx.device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[bindless_set],
                &[],
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
            renderer.device_ctx.device.cmd_draw_indexed_indirect_count(
                cmd,
                chunk_pool.scene_buffer(),
                chunk_pool.dense_indirect_region_offset(),
                draw_count_buffer,
                0,
                max_draw_count,
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
