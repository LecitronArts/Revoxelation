use anyhow::{Context, Result};
use ash::vk;
use egui::{
    ClippedPrimitive, TexturesDelta,
    epaint::{
        self, ImageData, Primitive, TextureId,
    },
    TextureFilter, TextureOptions, TextureWrapMode,
};
use gpu_allocator::vulkan::{Allocation, AllocationScheme};

use crate::renderer::{
    Renderer, create_allocated_buffer, create_allocated_image,
    destroy_allocated_buffer, destroy_allocated_image,
};
use crate::renderer::staging::StagingBuffer;
use crate::renderer::spirv::create_shader_module;

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct GuiVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
}

impl From<epaint::Vertex> for GuiVertex {
    fn from(vertex: epaint::Vertex) -> Self {
        Self {
            pos: [vertex.pos.x, vertex.pos.y],
            uv: [vertex.uv.x, vertex.uv.y],
            color: vertex.color.to_array(),
        }
    }
}

struct ScratchMeshBuffers {
    vertex_buffer: vk::Buffer,
    index_buffer: vk::Buffer,
    index_count: u32,
}

pub struct EguiAshBackend {
    pub(crate) pipeline: vk::Pipeline,
    pub(crate) pipeline_layout: vk::PipelineLayout,
    pub(crate) descriptor_pool: vk::DescriptorPool,
    pub(crate) descriptor_set_layout: vk::DescriptorSetLayout,
    pub(crate) font_image: vk::Image,
    pub(crate) font_image_view: vk::ImageView,
    pub(crate) font_allocation: Option<Allocation>,
    pub(crate) font_sampler: vk::Sampler,
    /// Per-frame descriptor sets (HIGH-01): 2 sets for double-buffered frames.
    /// Each frame binds and updates only its own set, preventing use-after-free
    /// when font texture is updated while the other frame's command buffer is in-flight.
    pub(crate) descriptor_sets: [vk::DescriptorSet; 2],
    font_extent: Option<vk::Extent3D>,
    /// Per-frame scratch buffer ring (2 = FRAMES_IN_FLIGHT).
    /// Buffers recorded into slot `current_frame` during paint() are only freed
    /// when that same slot is reused 2 frames later — after the corresponding
    /// frame fence has been waited on, guaranteeing the GPU is done with them.
    scratch_buffers: [Vec<(vk::Buffer, Allocation)>; 2],
}

impl EguiAshBackend {
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let device = &renderer.device_ctx.device;
        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let descriptor_set_layout = unsafe {
            device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .context("failed to create egui descriptor set layout")?
        };

        // Per-frame descriptor sets: 2 sets for double-buffered frames (HIGH-01).
        // This ensures the other frame's command buffer never references a descriptor
        // set that was modified by a font texture update on the current frame.
        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(2)];
        let descriptor_pool = unsafe {
            device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .pool_sizes(&pool_sizes)
                        .max_sets(2),
                    None,
                )
                .context("failed to create egui descriptor pool")?
        };

        let descriptor_set_layouts = [descriptor_set_layout];
        let alloc_layouts = [descriptor_set_layout, descriptor_set_layout];
        let allocated_sets = unsafe {
            device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(descriptor_pool)
                        .set_layouts(&alloc_layouts),
                )
                .context("failed to allocate egui descriptor sets")?
        };
        let descriptor_sets: [vk::DescriptorSet; 2] = [allocated_sets[0], allocated_sets[1]];

        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX)
            .offset(0)
            .size(8)];
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&descriptor_set_layouts)
                        .push_constant_ranges(&push_constant_ranges),
                    None,
                )
                .context("failed to create egui pipeline layout")?
        };

        Ok(Self {
            pipeline: vk::Pipeline::null(),
            pipeline_layout,
            descriptor_pool,
            descriptor_set_layout,
            font_image: vk::Image::null(),
            font_image_view: vk::ImageView::null(),
            font_allocation: None,
            font_sampler: vk::Sampler::null(),
            descriptor_sets,
            font_extent: None,
            scratch_buffers: [Vec::new(), Vec::new()],
        })
    }

    /// Lazily create the graphics pipeline on first paint with a valid font texture.
    fn ensure_pipeline(&mut self, renderer: &Renderer) -> Result<()> {
        if self.pipeline != vk::Pipeline::null() {
            return Ok(());
        }

        let device = &renderer.device_ctx.device;

        let vert_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/egui.vert.spv")),
        )?;
        let frag_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/egui.frag.spv")),
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

        // GuiVertex: pos(f32x2) + uv(f32x2) + color(u8x4) = 20 bytes
        let vertex_bindings = [vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<GuiVertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }];
        let vertex_attributes = [
            vk::VertexInputAttributeDescription {
                location: 0,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                location: 1,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 8,
            },
            vk::VertexInputAttributeDescription {
                location: 2,
                binding: 0,
                format: vk::Format::R8G8B8A8_UNORM,
                offset: 16,
            },
        ];

        let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&vertex_bindings)
            .vertex_attribute_descriptions(&vertex_attributes);

        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE);

        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);

        // Pre-multiplied alpha blending.
        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::ONE)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD)
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let color_blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_blend_attachment);

        // egui draws on top of everything — no depth testing.
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(false)
            .depth_write_enable(false);

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .color_blend_state(&color_blend)
            .depth_stencil_state(&depth_stencil)
            .dynamic_state(&dynamic_state)
            .layout(self.pipeline_layout)
            .render_pass(renderer.swapchain_ctx.render_pass)
            .subpass(0)];

        let cache_handle = renderer
            .pipeline_cache
            .as_ref()
            .map(|c| c.handle())
            .unwrap_or(vk::PipelineCache::null());

        self.pipeline = unsafe {
            device
                .create_graphics_pipelines(cache_handle, &pipeline_info, None)
                .map_err(|(_, err)| err)
                .context("failed to create egui graphics pipeline")?
                .into_iter()
                .next()
                .context("egui pipeline creation returned empty")?
        };

        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        Ok(())
    }

    /// Free scratch buffers for the given frame slot.
    ///
    /// Called at the start of each frame after the fence for `current_frame` has
    /// been waited on. Since we just reused this slot, the buffers it contains
    /// are from 2 frames ago and the GPU is guaranteed to be done with them.
    fn free_stale_scratch(&mut self, renderer: &mut Renderer, current_frame: usize) -> Result<()> {
        for (buffer, allocation) in self.scratch_buffers[current_frame].drain(..) {
            destroy_allocated_buffer(renderer, buffer, allocation)?;
        }
        Ok(())
    }

    pub fn paint(
        &mut self,
        renderer: &mut Renderer,
        cmd: vk::CommandBuffer,
        current_frame: usize,
        textures_delta: TexturesDelta,
        clipped_primitives: Vec<ClippedPrimitive>,
        screen_size_points: [f32; 2],
    ) -> Result<()> {
        // 0. Free scratch buffers for THIS frame slot (safe — fence was waited on).
        self.free_stale_scratch(renderer, current_frame)?;

        // 1. Process texture updates.
        for (texture_id, delta) in textures_delta.set {
            if matches!(texture_id, TextureId::Managed(0)) {
                self.upload_font_delta(renderer, delta)?;
            }
        }

        for texture_id in textures_delta.free {
            if matches!(texture_id, TextureId::Managed(0)) {
                self.destroy_font_texture(renderer)?;
            }
        }

        // 2. Nothing to draw if no primitives or no font texture ready.
        if clipped_primitives.is_empty() || self.font_image == vk::Image::null() {
            return Ok(());
        }

        // 3. Ensure pipeline is created.
        self.ensure_pipeline(renderer)?;

        let device = renderer.device_ctx.device.clone();
        let extent = renderer.swapchain_ctx.extent;

        unsafe {
            // 4. Bind pipeline.
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);

            // 5. Set viewport.
            let viewport = vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: extent.width as f32,
                height: extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            };
            device.cmd_set_viewport(cmd, 0, &[viewport]);

            // 6. Push screen size constants.
            device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(&screen_size_points),
            );

            // 7. Bind current frame's font descriptor set (HIGH-01).
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[self.descriptor_sets[current_frame]],
                &[],
            );
        }

        // 8. Draw each clipped primitive.
        for clipped in clipped_primitives {
            let clip_rect = clipped.clip_rect;

            if let Primitive::Mesh(mesh) = clipped.primitive {
                let Some(scratch) = self.upload_mesh_scratch(renderer, current_frame, &mesh)? else {
                    continue;
                };

                // Convert egui clip rect to Vulkan scissor (pixel coordinates).
                let clip_min_x = (clip_rect.min.x.round() as i32).max(0);
                let clip_min_y = (clip_rect.min.y.round() as i32).max(0);
                let clip_max_x = (clip_rect.max.x.round() as u32).min(extent.width);
                let clip_max_y = (clip_rect.max.y.round() as u32).min(extent.height);

                if clip_max_x <= clip_min_x as u32 || clip_max_y <= clip_min_y as u32 {
                    continue;
                }

                let scissor = vk::Rect2D {
                    offset: vk::Offset2D {
                        x: clip_min_x,
                        y: clip_min_y,
                    },
                    extent: vk::Extent2D {
                        width: clip_max_x - clip_min_x as u32,
                        height: clip_max_y - clip_min_y as u32,
                    },
                };

                unsafe {
                    device.cmd_set_scissor(cmd, 0, &[scissor]);
                    device.cmd_bind_vertex_buffers(cmd, 0, &[scratch.vertex_buffer], &[0]);
                    device.cmd_bind_index_buffer(
                        cmd,
                        scratch.index_buffer,
                        0,
                        vk::IndexType::UINT32,
                    );
                    device.cmd_draw_indexed(cmd, scratch.index_count, 1, 0, 0, 0);
                }
            }
        }

        Ok(())
    }

    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        // Free any remaining scratch buffers from ALL frame slots.
        for slot in 0..self.scratch_buffers.len() {
            for (buffer, allocation) in self.scratch_buffers[slot].drain(..) {
                destroy_allocated_buffer(renderer, buffer, allocation)?;
            }
        }
        self.destroy_font_texture(renderer)?;

        let device = &renderer.device_ctx.device;
        unsafe {
            if self.pipeline != vk::Pipeline::null() {
                device.destroy_pipeline(self.pipeline, None);
            }
            if self.pipeline_layout != vk::PipelineLayout::null() {
                device.destroy_pipeline_layout(self.pipeline_layout, None);
            }
            if self.descriptor_pool != vk::DescriptorPool::null() {
                device.destroy_descriptor_pool(self.descriptor_pool, None);
            }
            if self.descriptor_set_layout != vk::DescriptorSetLayout::null() {
                device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            }
        }

        Ok(())
    }

    fn upload_font_delta(
        &mut self,
        renderer: &mut Renderer,
        delta: egui::epaint::ImageDelta,
    ) -> Result<()> {
        let size = delta.image.size();
        let extent = vk::Extent3D {
            width: size[0] as u32,
            height: size[1] as u32,
            depth: 1,
        };
        let whole_update = delta.pos.is_none();
        let mut newly_created = false;

        if whole_update
            && self.font_extent != Some(extent)
            && self.font_image != vk::Image::null()
        {
            self.destroy_font_texture(renderer)?;
        }

        if self.font_image == vk::Image::null() {
            let (font_image, font_allocation) = create_allocated_image(
                renderer,
                extent,
                vk::Format::R8G8B8A8_SRGB,
                vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
                AllocationScheme::DedicatedImage(vk::Image::null()),
                "egui-font-image",
            )?;

            let device = &renderer.device_ctx.device;
            let font_image_view = unsafe {
                device
                    .create_image_view(
                        &vk::ImageViewCreateInfo::default()
                            .image(font_image)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(vk::Format::R8G8B8A8_SRGB)
                            .subresource_range(
                                vk::ImageSubresourceRange::default()
                                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                                    .base_mip_level(0)
                                    .level_count(1)
                                    .base_array_layer(0)
                                    .layer_count(1),
                            ),
                        None,
                    )
                    .context("failed to create egui font image view")?
            };

            self.font_image = font_image;
            self.font_image_view = font_image_view;
            self.font_allocation = Some(font_allocation);
            self.font_extent = Some(extent);
            newly_created = true;
        }

        self.recreate_sampler(renderer, delta.options)?;

        let rgba_bytes = image_to_rgba_bytes(&delta.image);
        let mut staging = StagingBuffer::new(renderer, rgba_bytes.len() as u64)?;
        staging.write(&rgba_bytes);
        staging.copy_to_image(
            renderer,
            self.font_image,
            extent,
            delta.pos.unwrap_or([0, 0]).map(|value| value as u32),
            if newly_created {
                vk::ImageLayout::UNDEFINED
            } else {
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
            },
        )?;
        staging.destroy(renderer)?;

        self.write_font_descriptor(renderer)?;
        Ok(())
    }

    fn recreate_sampler(&mut self, renderer: &Renderer, options: TextureOptions) -> Result<()> {
        let device = &renderer.device_ctx.device;
        unsafe {
            if self.font_sampler != vk::Sampler::null() {
                device.destroy_sampler(self.font_sampler, None);
            }

            self.font_sampler = device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(texture_filter_to_vk(options.magnification))
                        .min_filter(texture_filter_to_vk(options.minification))
                        .address_mode_u(texture_wrap_to_vk(options.wrap_mode))
                        .address_mode_v(texture_wrap_to_vk(options.wrap_mode))
                        .address_mode_w(texture_wrap_to_vk(options.wrap_mode))
                        .anisotropy_enable(false)
                        .max_anisotropy(1.0)
                        .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                        .min_lod(0.0)
                        .max_lod(0.0),
                    None,
                )
                .context("failed to create egui font sampler")?;
        }

        Ok(())
    }

    fn write_font_descriptor(&self, renderer: &Renderer) -> Result<()> {
        if self.font_image_view == vk::ImageView::null()
            || self.font_sampler == vk::Sampler::null()
        {
            return Ok(());
        }

        let image_infos = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(self.font_image_view)
            .sampler(self.font_sampler)];
        // Update both per-frame descriptor sets (HIGH-01).
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(self.descriptor_sets[0])
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image_infos),
            vk::WriteDescriptorSet::default()
                .dst_set(self.descriptor_sets[1])
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&image_infos),
        ];

        unsafe {
            renderer
                .device_ctx
                .device
                .update_descriptor_sets(&writes, &[]);
        }

        Ok(())
    }

    fn upload_mesh_scratch(
        &mut self,
        renderer: &mut Renderer,
        current_frame: usize,
        mesh: &epaint::Mesh,
    ) -> Result<Option<ScratchMeshBuffers>> {
        if mesh.is_empty() {
            return Ok(None);
        }

        let vertices: Vec<GuiVertex> = mesh.vertices.iter().copied().map(Into::into).collect();
        let vertex_bytes = bytemuck::cast_slice(&vertices);
        let index_bytes = bytemuck::cast_slice(mesh.indices.as_slice());

        // Use CpuToGpu host-visible memory — egui meshes are small (<100KB).
        // Upload via staging buffer for correctness (guaranteed GPU-side visibility).
        let (vb, vb_alloc) = create_allocated_buffer(
            renderer,
            vertex_bytes.len() as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER,
            gpu_allocator::MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "egui-scratch-vertex",
        )?;
        {
            let mut staging = StagingBuffer::new(renderer, vertex_bytes.len() as u64)?;
            staging.write(vertex_bytes);
            staging.copy_to(renderer, vb, vertex_bytes.len() as u64)?;
            staging.destroy(renderer)?;
        }

        let (ib, ib_alloc) = create_allocated_buffer(
            renderer,
            index_bytes.len() as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::INDEX_BUFFER,
            gpu_allocator::MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "egui-scratch-index",
        )?;
        {
            let mut staging = StagingBuffer::new(renderer, index_bytes.len() as u64)?;
            staging.write(index_bytes);
            staging.copy_to(renderer, ib, index_bytes.len() as u64)?;
            staging.destroy(renderer)?;
        }

        let index_count = mesh.indices.len() as u32;

        // Register for deferred cleanup — freed when this frame slot is reused.
        self.scratch_buffers[current_frame].push((vb, vb_alloc));
        self.scratch_buffers[current_frame].push((ib, ib_alloc));

        Ok(Some(ScratchMeshBuffers {
            vertex_buffer: vb,
            index_buffer: ib,
            index_count,
        }))
    }

    fn destroy_font_texture(&mut self, renderer: &mut Renderer) -> Result<()> {
        let device = &renderer.device_ctx.device;

        unsafe {
            if self.font_sampler != vk::Sampler::null() {
                device.destroy_sampler(self.font_sampler, None);
                self.font_sampler = vk::Sampler::null();
            }
            if self.font_image_view != vk::ImageView::null() {
                device.destroy_image_view(self.font_image_view, None);
                self.font_image_view = vk::ImageView::null();
            }
        }

        if self.font_image != vk::Image::null() {
            if let Some(allocation) = self.font_allocation.take() {
                destroy_allocated_image(renderer, self.font_image, allocation)?;
            }
            self.font_image = vk::Image::null();
        }

        self.font_extent = None;
        Ok(())
    }
}

fn image_to_rgba_bytes(image: &ImageData) -> Vec<u8> {
    match image {
        ImageData::Color(image) => image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect(),
        ImageData::Font(font) => font
            .srgba_pixels(None)
            .flat_map(|pixel| pixel.to_array())
            .collect(),
    }
}

fn texture_filter_to_vk(filter: TextureFilter) -> vk::Filter {
    match filter {
        TextureFilter::Nearest => vk::Filter::NEAREST,
        TextureFilter::Linear => vk::Filter::LINEAR,
    }
}

fn texture_wrap_to_vk(wrap: TextureWrapMode) -> vk::SamplerAddressMode {
    match wrap {
        TextureWrapMode::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
        TextureWrapMode::Repeat => vk::SamplerAddressMode::REPEAT,
        TextureWrapMode::MirroredRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
    }
}
