use anyhow::{Context, Result, anyhow};
use ash::vk;
use gpu_allocator::{
    MemoryLocation,
    vulkan::{Allocation, AllocationCreateDesc, AllocationScheme},
};

use super::spirv::create_shader_module;
use super::Renderer;

/// Compute the number of mip levels needed for a Hi-Z pyramid of the given resolution.
///
/// `ceil(log2(max(width, height))) + 1` — includes the full-resolution mip 0.
pub fn hiz_mip_count(width: u32, height: u32) -> u32 {
    let max_dim = width.max(height);
    if max_dim <= 1 {
        return 1;
    }
    // floor(log2(max_dim)) + 1
    (u32::BITS - max_dim.leading_zeros())
}

/// Hi-Z depth pyramid — R32_SFLOAT image with a full mip chain for GPU occlusion culling.
///
/// Generated each frame from the previous frame's depth buffer using a compute shader
/// that performs 2×2 max downsampling per mip level.
pub struct HiZPyramid {
    /// The Hi-Z image (R32_SFLOAT, full mip chain).
    pub image: vk::Image,
    pub allocation: Option<Allocation>,
    /// One image view per mip level for compute shader writes.
    pub mip_views: Vec<vk::ImageView>,
    /// Full-pyramid image view for sampling in the cull shader.
    pub full_view: vk::ImageView,
    /// Sampler with NEAREST filtering for Hi-Z reads.
    pub sampler: vk::Sampler,
    /// Compute pipeline for Hi-Z generation.
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    /// One descriptor pool per mip transition (src→dst pair).
    /// We allocate descriptor sets on the fly per mip level.
    pub descriptor_pool: vk::DescriptorPool,
    /// Descriptor sets: one per mip-level transition. Index i reads mip i and writes mip i+1.
    pub descriptor_sets: Vec<vk::DescriptorSet>,
    /// A separate sampler + view for reading the depth buffer as mip 0 source.
    pub depth_src_view: vk::ImageView,
    pub depth_sampler: vk::Sampler,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
}

impl HiZPyramid {
    pub fn new(renderer: &mut Renderer, width: u32, height: u32) -> Result<Self> {
        let device = &renderer.device_ctx.device;
        let mip_count = hiz_mip_count(width, height);

        // Create Hi-Z image with full mip chain.
        let image = unsafe {
            device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::R32_SFLOAT)
                        .extent(vk::Extent3D {
                            width,
                            height,
                            depth: 1,
                        })
                        .mip_levels(mip_count)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(
                            vk::ImageUsageFlags::STORAGE
                                | vk::ImageUsageFlags::SAMPLED
                                | vk::ImageUsageFlags::TRANSFER_DST,
                        )
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .context("failed to create Hi-Z image")?
        };

        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation = renderer
            .allocator
            .allocate(&AllocationCreateDesc {
                name: "hiz-pyramid",
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::GpuAllocatorManaged,
            })
            .map_err(|e| anyhow!("failed to allocate Hi-Z image memory: {e}"))?;
        unsafe {
            device
                .bind_image_memory(image, allocation.memory(), allocation.offset())
                .context("failed to bind Hi-Z image memory")?;
        }

        // Create per-mip image views for compute write.
        let mut mip_views = Vec::with_capacity(mip_count as usize);
        for mip in 0..mip_count {
            let view = unsafe {
                device
                    .create_image_view(
                        &vk::ImageViewCreateInfo::default()
                            .image(image)
                            .view_type(vk::ImageViewType::TYPE_2D)
                            .format(vk::Format::R32_SFLOAT)
                            .subresource_range(
                                vk::ImageSubresourceRange::default()
                                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                                    .base_mip_level(mip)
                                    .level_count(1)
                                    .layer_count(1),
                            ),
                        None,
                    )
                    .context("failed to create Hi-Z mip image view")?
            };
            mip_views.push(view);
        }

        // Full-pyramid view for cull shader sampling.
        let full_view = unsafe {
            device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(vk::Format::R32_SFLOAT)
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(mip_count)
                                .layer_count(1),
                        ),
                    None,
                )
                .context("failed to create Hi-Z full image view")?
        };

        // Sampler for Hi-Z reads (nearest filtering).
        let sampler = unsafe {
            device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::NEAREST)
                        .min_filter(vk::Filter::NEAREST)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .min_lod(0.0)
                        .max_lod(mip_count as f32),
                    None,
                )
                .context("failed to create Hi-Z sampler")?
        };

        // Depth source sampler (nearest, for reading depth buffer as mip 0 source).
        let depth_sampler = unsafe {
            device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::NEAREST)
                        .min_filter(vk::Filter::NEAREST)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE),
                    None,
                )
                .context("failed to create depth sampler for Hi-Z")?
        };

        // Depth source image view (depth aspect for sampling the depth buffer).
        let depth_src_view = unsafe {
            device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(renderer.swapchain_ctx.depth_image)
                        .view_type(vk::ImageViewType::TYPE_2D)
                        .format(vk::Format::D32_SFLOAT)
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(vk::ImageAspectFlags::DEPTH)
                                .level_count(1)
                                .layer_count(1),
                        ),
                    None,
                )
                .context("failed to create depth source view for Hi-Z")?
        };

        // Descriptor set layout: binding 0 = combined image sampler (src), binding 1 = storage image (dst).
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
        let descriptor_set_layout = unsafe {
            device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                    None,
                )
                .context("failed to create Hi-Z descriptor set layout")?
        };

        // We need mip_count descriptor sets (mip_count-1 Hi-Z mip passes + 1 for depth→mip0).
        // Actually: we need mip_count sets total. Set 0: depth→mip0, Set i: mip(i-1)→mip(i) for i=1..mip_count-1.
        let set_count = mip_count.max(1);
        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(set_count),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(set_count),
        ];
        let descriptor_pool = unsafe {
            device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .pool_sizes(&pool_sizes)
                        .max_sets(set_count),
                    None,
                )
                .context("failed to create Hi-Z descriptor pool")?
        };

        let layouts: Vec<vk::DescriptorSetLayout> =
            (0..set_count).map(|_| descriptor_set_layout).collect();
        let descriptor_sets = unsafe {
            device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(descriptor_pool)
                        .set_layouts(&layouts),
                )
                .context("failed to allocate Hi-Z descriptor sets")?
        };

        // Write descriptor sets:
        // Set 0: src = depth buffer, dst = hiz mip 0
        // Set i (1..mip_count-1): src = hiz mip (i-1), dst = hiz mip i
        for i in 0..set_count as usize {
            let (src_view, src_sampler_handle) = if i == 0 {
                (depth_src_view, depth_sampler)
            } else {
                (mip_views[i - 1], sampler)
            };
            let src_info = [vk::DescriptorImageInfo::default()
                .image_view(src_view)
                .sampler(src_sampler_handle)
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
            let dst_info = [vk::DescriptorImageInfo::default()
                .image_view(mip_views[i])
                .image_layout(vk::ImageLayout::GENERAL)];
            let writes = [
                vk::WriteDescriptorSet::default()
                    .dst_set(descriptor_sets[i])
                    .dst_binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .image_info(&src_info),
                vk::WriteDescriptorSet::default()
                    .dst_set(descriptor_sets[i])
                    .dst_binding(1)
                    .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                    .image_info(&dst_info),
            ];
            unsafe {
                device.update_descriptor_sets(&writes, &[]);
            }
        }

        // Push constant: ivec2 dst_size (8 bytes).
        let push_constant_ranges = [vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
            .offset(0)
            .size(8)]; // ivec2 = 2 * i32 = 8 bytes

        let set_layouts = [descriptor_set_layout];
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&set_layouts)
                        .push_constant_ranges(&push_constant_ranges),
                    None,
                )
                .context("failed to create Hi-Z pipeline layout")?
        };

        let shader_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/hiz_generate.comp.spv")),
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
                .context("failed to create Hi-Z compute pipeline")?
                .into_iter()
                .next()
                .context("Hi-Z pipeline creation returned no pipeline")?
        };
        unsafe {
            device.destroy_shader_module(shader_module, None);
        }

        Ok(Self {
            image,
            allocation: Some(allocation),
            mip_views,
            full_view,
            sampler,
            pipeline,
            pipeline_layout,
            descriptor_set_layout,
            descriptor_pool,
            descriptor_sets,
            depth_src_view,
            depth_sampler,
            width,
            height,
            mip_count,
        })
    }

    /// Dispatch the Hi-Z generation compute shader for all mip levels.
    ///
    /// `cmd` must be a recording command buffer.
    /// The depth image must already be in `SHADER_READ_ONLY_OPTIMAL` layout.
    /// After this call, the Hi-Z image is in `SHADER_READ_ONLY_OPTIMAL` for cull shader sampling.
    pub fn generate(&self, device: &ash::Device, cmd: vk::CommandBuffer) {
        if self.mip_count == 0 {
            return;
        }

        // Transition entire Hi-Z image to GENERAL for compute writes.
        let init_barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::SHADER_WRITE)
            .image(self.image)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(self.mip_count)
                    .layer_count(1),
            );
        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[init_barrier],
            );

            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::COMPUTE, self.pipeline);
        }

        let mut mip_width = self.width;
        let mut mip_height = self.height;

        for mip in 0..self.mip_count as usize {
            // Destination mip dimensions.
            let dst_w = (mip_width).max(1);
            let dst_h = (mip_height).max(1);

            // If this is not the first pass, transition the source mip (mip-1 of hiz) to SHADER_READ.
            if mip > 0 {
                let src_barrier = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::GENERAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ)
                    .image(self.image)
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .base_mip_level((mip - 1) as u32)
                            .level_count(1)
                            .layer_count(1),
                    );
                unsafe {
                    device.cmd_pipeline_barrier(
                        cmd,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[],
                        &[src_barrier],
                    );
                }
            }

            unsafe {
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::COMPUTE,
                    self.pipeline_layout,
                    0,
                    &[self.descriptor_sets[mip]],
                    &[],
                );

                let pc_data = [dst_w as i32, dst_h as i32];
                let pc_bytes: &[u8] = bytemuck::cast_slice(&pc_data);
                device.cmd_push_constants(
                    cmd,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    pc_bytes,
                );

                let group_x = dst_w.div_ceil(8);
                let group_y = dst_h.div_ceil(8);
                device.cmd_dispatch(cmd, group_x, group_y, 1);
            }

            // Next mip is half the size.
            mip_width = (mip_width / 2).max(1);
            mip_height = (mip_height / 2).max(1);
        }

        // Transition the last written mip (and any still-GENERAL mips) to SHADER_READ_ONLY_OPTIMAL.
        let final_barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(self.image)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(if self.mip_count > 1 {
                        (self.mip_count - 1)
                    } else {
                        0
                    })
                    .level_count(1)
                    .layer_count(1),
            );
        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[final_barrier],
            );
        }
    }

    /// Recreate the Hi-Z pyramid for a new swapchain size.
    pub fn recreate(self, renderer: &mut Renderer, new_width: u32, new_height: u32) -> Result<Self> {
        self.destroy(renderer);
        Self::new(renderer, new_width, new_height)
    }

    pub fn destroy(mut self, renderer: &mut Renderer) {
        let device = &renderer.device_ctx.device;
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_sampler(self.depth_sampler, None);
            device.destroy_image_view(self.depth_src_view, None);
            device.destroy_image_view(self.full_view, None);
            for view in self.mip_views.drain(..) {
                device.destroy_image_view(view, None);
            }
        }
        if let Some(allocation) = self.allocation.take() {
            let _ = renderer
                .allocator
                .free(allocation)
                .map_err(|e| log::warn!("failed to free Hi-Z allocation: {e}"));
            unsafe {
                device.destroy_image(self.image, None);
            }
        }
    }
}
