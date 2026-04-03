//! Screen-Space Ambient Occlusion (SSAO) implementation (LGHT-03).
//!
//! Supports three algorithms: GTAO (default), HBAO+, and classic SSAO.
//! Uses compute shaders for AO calculation and bilateral blur.
//! AO result is exposed to fragment shaders via bindless binding 17, while the
//! compute path uses dedicated SSAO descriptor sets.

use anyhow::{Context, Result, anyhow};
use ash::vk;
use gpu_allocator::vulkan::Allocation;

use super::Renderer;
use super::bindless::BINDING_SSAO_TEXTURE;
use super::spirv::create_shader_module;

/// SSAO algorithm selection.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SsaoAlgorithm {
    Gtao = 0,
    HbaoPlus = 1,
    ClassicSsao = 2,
}

impl SsaoAlgorithm {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gtao => "GTAO",
            Self::HbaoPlus => "HBAO+",
            Self::ClassicSsao => "Classic SSAO",
        }
    }
}

/// SSAO configuration (egui-adjustable).
#[derive(Clone, Debug)]
pub struct SsaoConfig {
    pub algorithm: SsaoAlgorithm,
    /// World-space AO radius (default 0.5).
    pub radius: f32,
    /// AO strength multiplier (default 1.0).
    pub intensity: f32,
    /// Directions for GTAO/HBAO (default 8), samples for classic (default 32).
    pub sample_count: u32,
    /// Compute at half resolution for performance.
    pub half_resolution: bool,
    /// Whether SSAO is enabled.
    pub enabled: bool,
    /// Debug: show raw AO buffer as greyscale output.
    pub debug_view: bool,
}

impl Default for SsaoConfig {
    fn default() -> Self {
        Self {
            algorithm: SsaoAlgorithm::Gtao,
            radius: 0.5,
            intensity: 1.0,
            sample_count: 8,
            half_resolution: false,
            enabled: true,
            debug_view: false,
        }
    }
}

/// Push constants for the SSAO compute shader (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SsaoPushConstants {
    pub algorithm: u32,
    pub radius: f32,
    pub intensity: f32,
    pub sample_count: u32,
    pub screen_size: [f32; 2],
    pub near_plane: f32,
    pub far_plane: f32,
}

/// Push constants for the SSAO blur shader (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlurPushConstants {
    pub direction: [f32; 2],
    pub texel_size: [f32; 2],
    pub pass_index: u32,
    pub _pad0: f32,
    pub _pad1: f32,
    pub _pad2: f32,
}

/// Owns SSAO compute pipeline, blur pipeline, AO images, and samplers.
pub struct SsaoPass {
    /// AO result texture (R8_UNORM) — binding 17.
    pub ao_image: vk::Image,
    pub ao_view: vk::ImageView,
    pub ao_allocation: Option<Allocation>,
    /// Blur intermediate texture (R8_UNORM) — binding 24.
    pub blur_image: vk::Image,
    pub blur_view: vk::ImageView,
    pub blur_allocation: Option<Allocation>,
    /// Sampler for AO result (linear filter for smooth sampling in fragment shader).
    pub sampler: vk::Sampler,
    /// SSAO compute pipeline.
    pub compute_pipeline: vk::Pipeline,
    pub compute_layout: vk::PipelineLayout,
    /// Bilateral blur compute pipeline.
    pub blur_pipeline: vk::Pipeline,
    pub blur_layout: vk::PipelineLayout,
    /// Dedicated descriptor infrastructure for SSAO compute/blur passes.
    pub compute_descriptor_set_layout: vk::DescriptorSetLayout,
    pub blur_descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_pool: vk::DescriptorPool,
    pub compute_descriptor_set: vk::DescriptorSet,
    pub blur_h_descriptor_set: vk::DescriptorSet,
    pub blur_v_descriptor_set: vk::DescriptorSet,
    /// AO image dimensions.
    pub width: u32,
    pub height: u32,
}

impl SsaoPass {
    /// Create SSAO pass resources.
    pub fn new(
        renderer: &mut Renderer,
        width: u32,
        height: u32,
        config: &SsaoConfig,
    ) -> Result<Self> {
        let (ao_w, ao_h) = if config.half_resolution {
            (width / 2, height / 2)
        } else {
            (width, height)
        };
        let ao_w = ao_w.max(1);
        let ao_h = ao_h.max(1);

        let device = renderer.device_ctx.device.clone();

        // Create R8_UNORM AO result image.
        let (ao_image, ao_allocation) = create_r8_image(renderer, ao_w, ao_h, "ssao-ao-result")?;
        let ao_view = create_r8_view(&device, ao_image)?;

        // Create R8_UNORM blur intermediate image.
        let (blur_image, blur_allocation) =
            create_r8_image(renderer, ao_w, ao_h, "ssao-blur-intermediate")?;
        let blur_view = create_r8_view(&device, blur_image)?;

        // Sampler for reading AO in fragment shader (linear filter for smooth sampling).
        let sampler = unsafe {
            device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .context("failed to create SSAO sampler")?
        };

        // Transition both images to GENERAL for initial use.
        super::helpers::submit_one_shot_commands(renderer, |device, cmd| {
            let barriers = [
                image_layout_barrier(
                    ao_image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::GENERAL,
                ),
                image_layout_barrier(
                    blur_image,
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::GENERAL,
                ),
            ];
            unsafe {
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &barriers,
                );
            }
            Ok(())
        })?;

        let compute_descriptor_set_layout = create_ssao_descriptor_set_layout(&device)
            .context("failed to create SSAO compute descriptor set layout")?;
        let blur_descriptor_set_layout = create_ssao_descriptor_set_layout(&device)
            .context("failed to create SSAO blur descriptor set layout")?;
        let (descriptor_pool, compute_descriptor_set, blur_h_descriptor_set, blur_v_descriptor_set) =
            allocate_ssao_descriptor_sets(
                &device,
                compute_descriptor_set_layout,
                blur_descriptor_set_layout,
            )?;

        let (compute_pipeline, compute_layout) =
            create_ssao_compute_pipeline(renderer, compute_descriptor_set_layout)?;
        let (blur_pipeline, blur_layout) =
            create_ssao_blur_pipeline(renderer, blur_descriptor_set_layout)?;

        let pass = Self {
            ao_image,
            ao_view,
            ao_allocation: Some(ao_allocation),
            blur_image,
            blur_view,
            blur_allocation: Some(blur_allocation),
            sampler,
            compute_pipeline,
            compute_layout,
            blur_pipeline,
            blur_layout,
            compute_descriptor_set_layout,
            blur_descriptor_set_layout,
            descriptor_pool,
            compute_descriptor_set,
            blur_h_descriptor_set,
            blur_v_descriptor_set,
            width: ao_w,
            height: ao_h,
        };
        pass.refresh_descriptor_sets(renderer)?;

        Ok(pass)
    }

    /// Register AO textures with the bindless table.
    pub fn register_bindless(&self, renderer: &Renderer) {
        if let Some(bindless) = &renderer.bindless {
            // Binding 17: AO result as COMBINED_IMAGE_SAMPLER (for fragment shader read).
            bindless.register_image(
                &renderer.device_ctx.device,
                BINDING_SSAO_TEXTURE,
                self.ao_view,
                self.sampler,
                vk::ImageLayout::GENERAL,
            );
        }
    }

    pub fn refresh_descriptor_sets(&self, renderer: &Renderer) -> Result<()> {
        let hiz = renderer.hiz_pyramid.as_ref().ok_or_else(|| {
            anyhow!("Hi-Z pyramid must exist before SSAO descriptors can be refreshed")
        })?;
        let device = &renderer.device_ctx.device;

        write_ssao_descriptor_set(
            device,
            self.compute_descriptor_set,
            hiz.full_view,
            hiz.sampler,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            self.ao_view,
            vk::ImageLayout::GENERAL,
        );
        write_ssao_descriptor_set(
            device,
            self.blur_h_descriptor_set,
            self.ao_view,
            self.sampler,
            vk::ImageLayout::GENERAL,
            self.blur_view,
            vk::ImageLayout::GENERAL,
        );
        write_ssao_descriptor_set(
            device,
            self.blur_v_descriptor_set,
            self.blur_view,
            self.sampler,
            vk::ImageLayout::GENERAL,
            self.ao_view,
            vk::ImageLayout::GENERAL,
        );

        Ok(())
    }

    /// Record SSAO compute dispatch + bilateral blur into the command buffer.
    pub fn record_dispatch(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        config: &SsaoConfig,
    ) {
        if !config.enabled {
            return;
        }

        let group_x = self.width.div_ceil(8);
        let group_y = self.height.div_ceil(8);

        // ---- SSAO compute pass ----
        let ssao_pc = SsaoPushConstants {
            algorithm: config.algorithm as u32,
            radius: config.radius,
            intensity: config.intensity,
            sample_count: config.sample_count,
            screen_size: [self.width as f32, self.height as f32],
            near_plane: 0.1,
            far_plane: 2000.0,
        };

        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.compute_pipeline);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.compute_layout,
                0,
                &[self.compute_descriptor_set],
                &[],
            );
            device.cmd_push_constants(
                cmd,
                self.compute_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                bytemuck::bytes_of(&ssao_pc),
            );
            device.cmd_dispatch(cmd, group_x, group_y, 1);
        }

        // ---- Barrier: SSAO compute write → blur read ----
        let ao_barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(self.ao_image)
            .subresource_range(color_subresource_range());
        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[ao_barrier],
            );
        }

        let texel_size = [1.0 / self.width as f32, 1.0 / self.height as f32];

        // ---- Horizontal blur: read AO result → write blur intermediate ----
        let blur_h_pc = BlurPushConstants {
            direction: [1.0, 0.0],
            texel_size,
            pass_index: 0,
            _pad0: 0.0,
            _pad1: 0.0,
            _pad2: 0.0,
        };
        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.blur_pipeline);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.blur_layout,
                0,
                &[self.blur_h_descriptor_set],
                &[],
            );
            device.cmd_push_constants(
                cmd,
                self.blur_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                bytemuck::bytes_of(&blur_h_pc),
            );
            device.cmd_dispatch(cmd, group_x, group_y, 1);
        }

        // ---- Barrier: horizontal blur write → vertical blur read ----
        let blur_barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(self.blur_image)
            .subresource_range(color_subresource_range());
        let ao_write_barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_WRITE)
            .image(self.ao_image)
            .subresource_range(color_subresource_range());
        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[blur_barrier, ao_write_barrier],
            );
        }

        // ---- Vertical blur pass ----
        let blur_v_pc = BlurPushConstants {
            direction: [0.0, 1.0],
            texel_size,
            pass_index: 1,
            _pad0: 0.0,
            _pad1: 0.0,
            _pad2: 0.0,
        };
        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.blur_pipeline);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::COMPUTE,
                self.blur_layout,
                0,
                &[self.blur_v_descriptor_set],
                &[],
            );
            device.cmd_push_constants(
                cmd,
                self.blur_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                bytemuck::bytes_of(&blur_v_pc),
            );
            device.cmd_dispatch(cmd, group_x, group_y, 1);
        }

        // Final barrier: ao_image ready for fragment shader read.
        let final_barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(self.ao_image)
            .subresource_range(color_subresource_range());
        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[final_barrier],
            );
        }
    }

    /// Recreate AO images for a new swapchain size.
    pub fn recreate(
        self,
        renderer: &mut Renderer,
        new_width: u32,
        new_height: u32,
    ) -> Result<Self> {
        let config = renderer.ssao_config.clone();
        self.destroy(renderer)?;
        Self::new(renderer, new_width, new_height, &config)
    }

    /// Clean up all GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        let device = &renderer.device_ctx.device;
        unsafe {
            device.destroy_pipeline(self.compute_pipeline, None);
            device.destroy_pipeline_layout(self.compute_layout, None);
            device.destroy_pipeline(self.blur_pipeline, None);
            device.destroy_pipeline_layout(self.blur_layout, None);
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.compute_descriptor_set_layout, None);
            device.destroy_descriptor_set_layout(self.blur_descriptor_set_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_image_view(self.ao_view, None);
            device.destroy_image_view(self.blur_view, None);
        }
        if let Some(alloc) = self.ao_allocation.take() {
            unsafe {
                device.destroy_image(self.ao_image, None);
            }
            renderer
                .allocator
                .free(alloc)
                .map_err(|e| anyhow!("failed to free SSAO AO allocation: {e}"))?;
        }
        if let Some(alloc) = self.blur_allocation.take() {
            unsafe {
                device.destroy_image(self.blur_image, None);
            }
            renderer
                .allocator
                .free(alloc)
                .map_err(|e| anyhow!("failed to free SSAO blur allocation: {e}"))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

fn create_ssao_descriptor_set_layout(device: &ash::Device) -> Result<vk::DescriptorSetLayout> {
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_count(1)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
    ];

    unsafe {
        device
            .create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
            .context("failed to create SSAO descriptor set layout")
    }
}

fn allocate_ssao_descriptor_sets(
    device: &ash::Device,
    compute_layout: vk::DescriptorSetLayout,
    blur_layout: vk::DescriptorSetLayout,
) -> Result<(
    vk::DescriptorPool,
    vk::DescriptorSet,
    vk::DescriptorSet,
    vk::DescriptorSet,
)> {
    let pool_sizes = [
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(3),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(3),
    ];

    let descriptor_pool = unsafe {
        device
            .create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .pool_sizes(&pool_sizes)
                    .max_sets(3),
                None,
            )
            .context("failed to create SSAO descriptor pool")?
    };

    let compute_descriptor_set = unsafe {
        device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(descriptor_pool)
                    .set_layouts(&[compute_layout]),
            )
            .context("failed to allocate SSAO compute descriptor set")?
            .into_iter()
            .next()
            .context("SSAO compute descriptor set allocation returned empty")?
    };
    let blur_sets = unsafe {
        device
            .allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(descriptor_pool)
                    .set_layouts(&[blur_layout, blur_layout]),
            )
            .context("failed to allocate SSAO blur descriptor sets")?
    };
    let blur_h_descriptor_set = *blur_sets
        .first()
        .context("SSAO blur descriptor allocation returned no horizontal set")?;
    let blur_v_descriptor_set = *blur_sets
        .get(1)
        .context("SSAO blur descriptor allocation returned no vertical set")?;

    Ok((
        descriptor_pool,
        compute_descriptor_set,
        blur_h_descriptor_set,
        blur_v_descriptor_set,
    ))
}

fn write_ssao_descriptor_set(
    device: &ash::Device,
    descriptor_set: vk::DescriptorSet,
    input_view: vk::ImageView,
    sampler: vk::Sampler,
    input_layout: vk::ImageLayout,
    output_view: vk::ImageView,
    output_layout: vk::ImageLayout,
) {
    let input_info = [vk::DescriptorImageInfo::default()
        .image_view(input_view)
        .sampler(sampler)
        .image_layout(input_layout)];
    let output_info = [vk::DescriptorImageInfo::default()
        .image_view(output_view)
        .image_layout(output_layout)];
    let writes = [
        vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&input_info),
        vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .image_info(&output_info),
    ];
    unsafe {
        device.update_descriptor_sets(&writes, &[]);
    }
}

fn create_r8_image(
    renderer: &mut Renderer,
    width: u32,
    height: u32,
    name: &'static str,
) -> Result<(vk::Image, Allocation)> {
    super::helpers::create_allocated_image(
        renderer,
        vk::Extent3D {
            width,
            height,
            depth: 1,
        },
        vk::Format::R8_UNORM,
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC
            | vk::ImageUsageFlags::TRANSFER_DST,
        gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
        name,
    )
}

fn create_r8_view(device: &ash::Device, image: vk::Image) -> Result<vk::ImageView> {
    unsafe {
        device
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(vk::Format::R8_UNORM)
                    .subresource_range(color_subresource_range()),
                None,
            )
            .context("failed to create SSAO R8 image view")
    }
}

fn color_subresource_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .base_mip_level(0)
        .level_count(1)
        .base_array_layer(0)
        .layer_count(1)
}

fn image_layout_barrier(
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) -> vk::ImageMemoryBarrier<'static> {
    vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::SHADER_READ)
        .image(image)
        .subresource_range(color_subresource_range())
}

fn create_ssao_compute_pipeline(
    renderer: &Renderer,
    compute_descriptor_set_layout: vk::DescriptorSetLayout,
) -> Result<(vk::Pipeline, vk::PipelineLayout)> {
    let device = &renderer.device_ctx.device;

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<SsaoPushConstants>() as u32,
    }];
    let set_layouts = [compute_descriptor_set_layout];
    let pipeline_layout = unsafe {
        device
            .create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )
            .context("failed to create SSAO compute pipeline layout")?
    };

    let shader_module = create_shader_module(
        device,
        include_bytes!(concat!(env!("OUT_DIR"), "/ssao_compute.comp.spv")),
    )?;
    let entry_name = c"main";
    let stage = vk::PipelineShaderStageCreateInfo::default()
        .module(shader_module)
        .name(entry_name)
        .stage(vk::ShaderStageFlags::COMPUTE);

    let cache_handle = renderer
        .pipeline_cache
        .as_ref()
        .map(|c| c.handle())
        .unwrap_or(vk::PipelineCache::null());

    let pipeline = unsafe {
        device
            .create_compute_pipelines(
                cache_handle,
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(stage)
                    .layout(pipeline_layout)],
                None,
            )
            .map_err(|(_, err)| err)
            .context("failed to create SSAO compute pipeline")?
            .into_iter()
            .next()
            .context("SSAO compute pipeline creation returned empty")?
    };
    unsafe {
        device.destroy_shader_module(shader_module, None);
    }

    Ok((pipeline, pipeline_layout))
}

fn create_ssao_blur_pipeline(
    renderer: &Renderer,
    blur_descriptor_set_layout: vk::DescriptorSetLayout,
) -> Result<(vk::Pipeline, vk::PipelineLayout)> {
    let device = &renderer.device_ctx.device;

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::COMPUTE,
        offset: 0,
        size: std::mem::size_of::<BlurPushConstants>() as u32,
    }];
    let set_layouts = [blur_descriptor_set_layout];
    let pipeline_layout = unsafe {
        device
            .create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )
            .context("failed to create SSAO blur pipeline layout")?
    };

    let shader_module = create_shader_module(
        device,
        include_bytes!(concat!(env!("OUT_DIR"), "/ssao_blur.comp.spv")),
    )?;
    let entry_name = c"main";
    let stage = vk::PipelineShaderStageCreateInfo::default()
        .module(shader_module)
        .name(entry_name)
        .stage(vk::ShaderStageFlags::COMPUTE);

    let cache_handle = renderer
        .pipeline_cache
        .as_ref()
        .map(|c| c.handle())
        .unwrap_or(vk::PipelineCache::null());

    let pipeline = unsafe {
        device
            .create_compute_pipelines(
                cache_handle,
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(stage)
                    .layout(pipeline_layout)],
                None,
            )
            .map_err(|(_, err)| err)
            .context("failed to create SSAO blur pipeline")?
            .into_iter()
            .next()
            .context("SSAO blur pipeline creation returned empty")?
    };
    unsafe {
        device.destroy_shader_module(shader_module, None);
    }

    Ok((pipeline, pipeline_layout))
}
