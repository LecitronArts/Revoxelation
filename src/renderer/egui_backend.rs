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
    Renderer, StagingBuffer, create_allocated_buffer, create_allocated_image,
    destroy_allocated_buffer, destroy_allocated_image,
};

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

pub struct EguiAshBackend {
    pub(crate) pipeline: vk::Pipeline,
    pub(crate) pipeline_layout: vk::PipelineLayout,
    pub(crate) descriptor_pool: vk::DescriptorPool,
    pub(crate) descriptor_set_layout: vk::DescriptorSetLayout,
    pub(crate) font_image: vk::Image,
    pub(crate) font_image_view: vk::ImageView,
    pub(crate) font_allocation: Option<Allocation>,
    pub(crate) font_sampler: vk::Sampler,
    pub(crate) descriptor_set: vk::DescriptorSet,
    font_extent: Option<vk::Extent3D>,
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

        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)];
        let descriptor_pool = unsafe {
            device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .pool_sizes(&pool_sizes)
                        .max_sets(1),
                    None,
                )
                .context("failed to create egui descriptor pool")?
        };

        let descriptor_set_layouts = [descriptor_set_layout];
        let descriptor_set = unsafe {
            device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(descriptor_pool)
                        .set_layouts(&descriptor_set_layouts),
                )
                .context("failed to allocate egui descriptor set")?
                .into_iter()
                .next()
                .context("egui descriptor allocation returned no descriptor sets")?
        };

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
            descriptor_set,
            font_extent: None,
        })
    }

    pub fn paint(
        &mut self,
        renderer: &mut Renderer,
        cmd: vk::CommandBuffer,
        textures_delta: TexturesDelta,
        clipped_primitives: Vec<ClippedPrimitive>,
        screen_size_points: [f32; 2],
    ) -> Result<()> {
        let _ = (cmd, screen_size_points);

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

        for clipped in clipped_primitives {
            let _clip_rect = clipped.clip_rect;
            if let Primitive::Mesh(mesh) = clipped.primitive {
                self.upload_mesh_scratch(renderer, &mesh)?;
            }
        }

        Ok(())
    }

    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
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

    fn upload_font_delta(&mut self, renderer: &mut Renderer, delta: egui::epaint::ImageDelta) -> Result<()> {
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
        if self.font_image_view == vk::ImageView::null() || self.font_sampler == vk::Sampler::null() {
            return Ok(());
        }

        let image_infos = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .image_view(self.font_image_view)
            .sampler(self.font_sampler)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(self.descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_infos)];

        unsafe {
            renderer
                .device_ctx
                .device
                .update_descriptor_sets(&writes, &[]);
        }

        Ok(())
    }

    fn upload_mesh_scratch(&mut self, renderer: &mut Renderer, mesh: &epaint::Mesh) -> Result<()> {
        if mesh.is_empty() {
            return Ok(());
        }

        let vertices: Vec<GuiVertex> = mesh.vertices.iter().copied().map(Into::into).collect();
        let vertex_bytes = bytemuck::cast_slice(&vertices);
        let index_bytes = bytemuck::cast_slice(mesh.indices.as_slice());

        if !vertex_bytes.is_empty() {
            let mut staging = StagingBuffer::new(renderer, vertex_bytes.len() as u64)?;
            staging.write(vertex_bytes);
            let (dst, allocation) = create_allocated_buffer(
                renderer,
                vertex_bytes.len() as u64,
                vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER,
                gpu_allocator::MemoryLocation::GpuOnly,
                AllocationScheme::GpuAllocatorManaged,
                "egui-scratch-vertex",
            )?;
            staging.copy_to(renderer, dst, vertex_bytes.len() as u64)?;
            staging.destroy(renderer)?;
            destroy_allocated_buffer(renderer, dst, allocation)?;
        }

        if !index_bytes.is_empty() {
            let mut staging = StagingBuffer::new(renderer, index_bytes.len() as u64)?;
            staging.write(index_bytes);
            let (dst, allocation) = create_allocated_buffer(
                renderer,
                index_bytes.len() as u64,
                vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::INDEX_BUFFER,
                gpu_allocator::MemoryLocation::GpuOnly,
                AllocationScheme::GpuAllocatorManaged,
                "egui-scratch-index",
            )?;
            staging.copy_to(renderer, dst, index_bytes.len() as u64)?;
            staging.destroy(renderer)?;
            destroy_allocated_buffer(renderer, dst, allocation)?;
        }

        Ok(())
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
