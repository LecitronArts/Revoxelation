//! Cascaded Shadow Map (CSM) implementation for directional light shadows (LGHT-02).
//!
//! Creates 4 cascade depth maps rendered as depth-only passes before the main
//! render pass. Fragment shaders sample these via a comparison sampler at binding 16
//! for PCF soft shadows with cascade blending.

use anyhow::{Context, Result};
use ash::vk;
use glam::{Mat4, Vec3, Vec4};
use gpu_allocator::vulkan::Allocation;

use super::Renderer;
use super::bindless::BINDING_CSM_SHADOW_MAPS;
use super::spirv::create_shader_module;
use super::swapchain::DEPTH_FORMAT;

/// Default shadow map resolution per cascade.
pub const DEFAULT_SHADOW_RESOLUTION: u32 = 2048;
/// Number of cascades.
pub const CASCADE_COUNT: u32 = 4;

/// Runtime-adjustable shadow configuration (egui-controllable).
#[derive(Clone, Debug)]
pub struct ShadowConfig {
    /// Shadow map resolution per cascade (default 2048).
    pub resolution: u32,
    /// Practical split lambda (0=linear, 1=logarithmic, default 0.5).
    pub split_lambda: f32,
    /// Depth bias constant factor (default 1.25).
    pub bias_constant: f32,
    /// Depth bias slope factor (default 1.75).
    pub bias_slope: f32,
    /// Whether shadows are enabled.
    pub enabled: bool,
    /// Debug: colorize fragments by cascade index.
    pub debug_cascades: bool,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            resolution: DEFAULT_SHADOW_RESOLUTION,
            split_lambda: 0.5,
            bias_constant: 1.25,
            bias_slope: 1.75,
            enabled: true,
            debug_cascades: false,
        }
    }
}

/// Push constants for the shadow depth vertex shader (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShadowPushConstants {
    pub light_view_proj: [[f32; 4]; 4],
}

/// Owns 4-cascade CSM depth images, render pass, framebuffers, and depth-only pipeline.
pub struct CascadedShadowMap {
    /// Single 2D array image with 4 layers (D32_SFLOAT).
    pub depth_image: vk::Image,
    pub depth_allocation: Option<Allocation>,
    /// Per-layer image views for framebuffer attachments.
    pub layer_views: [vk::ImageView; 4],
    /// 2D_ARRAY view for shader sampling.
    pub array_view: vk::ImageView,
    /// Comparison sampler for hardware PCF.
    pub sampler: vk::Sampler,
    /// Depth-only render pass.
    pub render_pass: vk::RenderPass,
    /// Per-cascade framebuffers.
    pub framebuffers: [vk::Framebuffer; 4],
    /// Depth-only graphics pipeline.
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    /// Resolution of each cascade map.
    pub resolution: u32,
}

impl CascadedShadowMap {
    /// Create CSM resources: 4-layer depth image, render pass, pipeline, framebuffers.
    pub fn new(renderer: &mut Renderer, resolution: u32) -> Result<Self> {
        let device = renderer.device_ctx.device.clone();

        // Create 2D array depth image (4 layers, D32_SFLOAT).
        let image_create_info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(DEPTH_FORMAT)
            .extent(vk::Extent3D {
                width: resolution,
                height: resolution,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(CASCADE_COUNT)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let depth_image = unsafe {
            device
                .create_image(&image_create_info, None)
                .context("failed to create CSM depth array image")?
        };
        let requirements = unsafe { device.get_image_memory_requirements(depth_image) };
        let depth_allocation = super::helpers::allocator_mut(renderer)
            .allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
                name: "csm-depth-array",
                requirements,
                location: gpu_allocator::MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: gpu_allocator::vulkan::AllocationScheme::DedicatedImage(
                    depth_image,
                ),
            })
            .map_err(|e| anyhow::anyhow!("failed to allocate CSM depth image memory: {e}"))?;
        unsafe {
            device
                .bind_image_memory(
                    depth_image,
                    depth_allocation.memory(),
                    depth_allocation.offset(),
                )
                .context("failed to bind CSM depth image memory")?;
        }

        // Per-layer views for framebuffer attachments.
        let mut layer_views = [vk::ImageView::null(); 4];
        for i in 0..CASCADE_COUNT {
            let view_info = vk::ImageViewCreateInfo::default()
                .image(depth_image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(DEPTH_FORMAT)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::DEPTH)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(i)
                        .layer_count(1),
                );
            layer_views[i as usize] = unsafe {
                device
                    .create_image_view(&view_info, None)
                    .context("failed to create CSM layer image view")?
            };
        }

        // 2D_ARRAY view for shader sampling (all 4 layers).
        let array_view = unsafe {
            device
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(depth_image)
                        .view_type(vk::ImageViewType::TYPE_2D_ARRAY)
                        .format(DEPTH_FORMAT)
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(vk::ImageAspectFlags::DEPTH)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(CASCADE_COUNT),
                        ),
                    None,
                )
                .context("failed to create CSM array image view")?
        };

        // Comparison sampler for hardware PCF.
        let sampler = unsafe {
            device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::LINEAR)
                        .min_filter(vk::Filter::LINEAR)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_BORDER)
                        .border_color(vk::BorderColor::FLOAT_OPAQUE_WHITE)
                        .compare_enable(true)
                        .compare_op(vk::CompareOp::LESS_OR_EQUAL)
                        .min_lod(0.0)
                        .max_lod(1.0),
                    None,
                )
                .context("failed to create CSM comparison sampler")?
        };

        // Depth-only render pass.
        let render_pass = create_shadow_render_pass(&device)?;

        // Per-cascade framebuffers.
        let mut framebuffers = [vk::Framebuffer::null(); 4];
        for i in 0..CASCADE_COUNT {
            let attachments = [layer_views[i as usize]];
            let fb_info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(resolution)
                .height(resolution)
                .layers(1);
            framebuffers[i as usize] = unsafe {
                device
                    .create_framebuffer(&fb_info, None)
                    .context("failed to create CSM framebuffer")?
            };
        }

        // Depth-only pipeline — needs immutable renderer reference after allocator borrow ends.
        // We've already finished all mutable allocator work above.
        let (pipeline, pipeline_layout) = create_shadow_pipeline(renderer, render_pass)?;

        // Transition image to DEPTH_STENCIL_ATTACHMENT_OPTIMAL for first use.
        super::helpers::submit_one_shot_commands(renderer, |device, cmd| {
            let barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
                .image(depth_image)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::DEPTH)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(CASCADE_COUNT),
                );
            unsafe {
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
            }
            Ok(())
        })?;

        Ok(Self {
            depth_image,
            depth_allocation: Some(depth_allocation),
            layer_views,
            array_view,
            sampler,
            render_pass,
            framebuffers,
            pipeline,
            pipeline_layout,
            resolution,
        })
    }

    /// Register the CSM array view + comparison sampler at bindless binding 16.
    pub fn register_shadow_maps(&self, renderer: &Renderer) {
        if let Some(bindless) = &renderer.bindless {
            bindless.register_image(
                &renderer.device_ctx.device,
                BINDING_CSM_SHADOW_MAPS,
                self.array_view,
                self.sampler,
                vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL,
            );
        }
    }

    /// Record shadow depth passes for all 4 cascades.
    ///
    /// Each cascade: begin render pass → bind pipeline → push light VP → draw all visible meshlets.
    /// After all passes, transition depth image to SHADER_READ_ONLY for fragment shader sampling.
    #[allow(clippy::too_many_arguments)]
    pub fn record_shadow_passes(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        meshlet_pool: Option<&super::chunk_pool::MeshletPool>,
        cascade_matrices: &[Mat4; 4],
    ) {
        let Some(meshlet_pool) = meshlet_pool else {
            return;
        };
        let total_meshlets = meshlet_pool.active_meshlet_count();
        if total_meshlets == 0 {
            return;
        }

        let viewport = vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: self.resolution as f32,
            height: self.resolution as f32,
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: self.resolution,
                height: self.resolution,
            },
        };

        let vertex_buffers = [meshlet_pool.meshlet_vertex_buffer];
        let vertex_offsets: [vk::DeviceSize; 1] = [0];

        for cascade in 0..CASCADE_COUNT as usize {
            let clear_values = [vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            }];
            let render_pass_begin = vk::RenderPassBeginInfo::default()
                .render_pass(self.render_pass)
                .framebuffer(self.framebuffers[cascade])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: vk::Extent2D {
                        width: self.resolution,
                        height: self.resolution,
                    },
                })
                .clear_values(&clear_values);

            let shadow_pc = ShadowPushConstants {
                light_view_proj: cascade_matrices[cascade].to_cols_array_2d(),
            };

            unsafe {
                device.cmd_begin_render_pass(cmd, &render_pass_begin, vk::SubpassContents::INLINE);
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
                device.cmd_set_viewport(cmd, 0, &[viewport]);
                device.cmd_set_scissor(cmd, 0, &[scissor]);
                device.cmd_push_constants(
                    cmd,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    bytemuck::bytes_of(&shadow_pc),
                );
                // Bind shared bindless descriptor set 0 (for scene_data, meshlet_meta, visible_buf).
                // This is already bound from the cull dispatch, but we need to rebind for this pipeline layout.
                // The caller must ensure bindless_set is bound.

                device.cmd_bind_vertex_buffers(cmd, 0, &vertex_buffers, &vertex_offsets);
                device.cmd_bind_index_buffer(
                    cmd,
                    meshlet_pool.meshlet_tri_buffer,
                    0,
                    vk::IndexType::UINT32,
                );
                // Draw all visible meshlets via indirect count.
                let max_draw_count = meshlet_pool.meshlet_capacity() as u32;
                device.cmd_draw_indexed_indirect_count(
                    cmd,
                    meshlet_pool.meshlet_indirect_buffer,
                    0,
                    meshlet_pool.meshlet_count_buffer,
                    0,
                    max_draw_count,
                    std::mem::size_of::<vk::DrawIndexedIndirectCommand>() as u32,
                );
                device.cmd_end_render_pass(cmd);
            }
        }
    }

    /// Transition shadow depth image after all cascade passes:
    /// DEPTH_STENCIL_ATTACHMENT → DEPTH_STENCIL_READ_ONLY for shader sampling.
    pub fn transition_to_read(&self, device: &ash::Device, cmd: vk::CommandBuffer) {
        let barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)
            .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::SHADER_READ)
            .image(self.depth_image)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(CASCADE_COUNT),
            );
        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
    }

    /// Transition shadow depth image back to attachment for next frame's shadow passes.
    pub fn transition_to_attachment(&self, device: &ash::Device, cmd: vk::CommandBuffer) {
        let barrier = vk::ImageMemoryBarrier::default()
            .old_layout(vk::ImageLayout::DEPTH_STENCIL_READ_ONLY_OPTIMAL)
            .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
            .src_access_mask(vk::AccessFlags::SHADER_READ)
            .dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
            .image(self.depth_image)
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::DEPTH)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(CASCADE_COUNT),
            );
        unsafe {
            device.cmd_pipeline_barrier(
                cmd,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[barrier],
            );
        }
    }

    /// Clean up all GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        let device = &renderer.device_ctx.device;
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
            for fb in &self.framebuffers {
                device.destroy_framebuffer(*fb, None);
            }
            device.destroy_render_pass(self.render_pass, None);
            device.destroy_sampler(self.sampler, None);
            device.destroy_image_view(self.array_view, None);
            for view in &self.layer_views {
                device.destroy_image_view(*view, None);
            }
            device.destroy_image(self.depth_image, None);
        }
        if let Some(alloc) = self.depth_allocation.take() {
            super::helpers::allocator_mut(renderer)
                .free(alloc)
                .map_err(|e| anyhow::anyhow!("failed to free CSM depth allocation: {e}"))?;
        }
        Ok(())
    }
}

/// Compute 4 cascade light-space view-projection matrices and split distances.
///
/// Uses the practical split scheme (λ blend between linear and logarithmic).
/// Tight-fits each cascade to the camera frustum slice.
/// Snaps to texel grid to reduce shadow shimmer.
pub fn compute_cascade_matrices(
    camera_view_proj_inv: &Mat4,
    camera_near: f32,
    camera_far: f32,
    sun_direction: Vec3,
    lambda: f32,
    resolution: u32,
) -> ([Mat4; 4], [f32; 4]) {
    let mut cascade_splits = [0.0_f32; 4];

    // Practical split scheme (Nvidia GPU Gems 3).
    for i in 0..CASCADE_COUNT {
        let p = (i + 1) as f32 / CASCADE_COUNT as f32;
        let log_split = camera_near * (camera_far / camera_near).powf(p);
        let uniform_split = camera_near + (camera_far - camera_near) * p;
        cascade_splits[i as usize] = lambda * log_split + (1.0 - lambda) * uniform_split;
    }

    let mut cascade_matrices = [Mat4::IDENTITY; 4];

    for i in 0..CASCADE_COUNT as usize {
        let near_split = if i == 0 {
            camera_near
        } else {
            cascade_splits[i - 1]
        };
        let far_split = cascade_splits[i];

        // Get frustum corners in world space for this cascade slice.
        let corners = frustum_corners_world_space(
            camera_view_proj_inv,
            near_split,
            far_split,
            camera_near,
            camera_far,
        );

        // Compute center of the frustum slice.
        let mut center = Vec3::ZERO;
        for c in &corners {
            center += *c;
        }
        center /= corners.len() as f32;

        // Light view matrix: looking along sun_direction.
        let light_dir = sun_direction.normalize_or_zero();
        let light_view = Mat4::look_at_rh(
            center - light_dir * 100.0, // pull back along light direction
            center,
            Vec3::Y,
        );

        // Transform frustum corners to light space to compute tight AABB.
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        let mut min_z = f32::MAX;
        let mut max_z = f32::MIN;

        for corner in &corners {
            let lc = light_view * Vec4::new(corner.x, corner.y, corner.z, 1.0);
            min_x = min_x.min(lc.x);
            max_x = max_x.max(lc.x);
            min_y = min_y.min(lc.y);
            max_y = max_y.max(lc.y);
            min_z = min_z.min(lc.z);
            max_z = max_z.max(lc.z);
        }

        // Extend Z range to catch shadow casters behind the camera.
        let z_extra = (max_z - min_z) * 2.0;
        min_z -= z_extra;

        // Texel grid snapping to prevent shadow shimmer.
        let texel_size_x = (max_x - min_x) / resolution as f32;
        let texel_size_y = (max_y - min_y) / resolution as f32;
        if texel_size_x > 0.0 {
            min_x = (min_x / texel_size_x).floor() * texel_size_x;
            max_x = (max_x / texel_size_x).ceil() * texel_size_x;
        }
        if texel_size_y > 0.0 {
            min_y = (min_y / texel_size_y).floor() * texel_size_y;
            max_y = (max_y / texel_size_y).ceil() * texel_size_y;
        }

        let light_proj = Mat4::orthographic_rh(min_x, max_x, min_y, max_y, min_z, max_z);
        cascade_matrices[i] = light_proj * light_view;
    }

    (cascade_matrices, cascade_splits)
}

/// Get the 8 frustum corners for a sub-range [near_split, far_split]
/// of the full frustum defined by camera_view_proj_inv, mapping from
/// clip-space NDC back to world space.
fn frustum_corners_world_space(
    camera_view_proj_inv: &Mat4,
    near_split: f32,
    far_split: f32,
    camera_near: f32,
    camera_far: f32,
) -> [Vec3; 8] {
    // NDC corners (Vulkan: z in [0, 1])
    let ndc_corners = [
        Vec4::new(-1.0, -1.0, 0.0, 1.0),
        Vec4::new(1.0, -1.0, 0.0, 1.0),
        Vec4::new(-1.0, 1.0, 0.0, 1.0),
        Vec4::new(1.0, 1.0, 0.0, 1.0),
        Vec4::new(-1.0, -1.0, 1.0, 1.0),
        Vec4::new(1.0, -1.0, 1.0, 1.0),
        Vec4::new(-1.0, 1.0, 1.0, 1.0),
        Vec4::new(1.0, 1.0, 1.0, 1.0),
    ];

    // Transform NDC to world space.
    let mut world_corners = [Vec3::ZERO; 8];
    for (i, ndc) in ndc_corners.iter().enumerate() {
        let wc = *camera_view_proj_inv * *ndc;
        world_corners[i] = Vec3::new(wc.x / wc.w, wc.y / wc.w, wc.z / wc.w);
    }

    // Interpolate between near plane corners (0-3) and far plane corners (4-7).
    let full_range = camera_far - camera_near;
    let near_t = if full_range > 0.0 {
        (near_split - camera_near) / full_range
    } else {
        0.0
    };
    let far_t = if full_range > 0.0 {
        (far_split - camera_near) / full_range
    } else {
        1.0
    };

    let mut result = [Vec3::ZERO; 8];
    for i in 0..4 {
        let near_world = world_corners[i];
        let far_world = world_corners[i + 4];
        let dir = far_world - near_world;
        result[i] = near_world + dir * near_t; // near plane
        result[i + 4] = near_world + dir * far_t; // far plane
    }

    result
}

/// Create a depth-only render pass for shadow map rendering.
fn create_shadow_render_pass(device: &ash::Device) -> Result<vk::RenderPass> {
    let attachment = [vk::AttachmentDescription::default()
        .format(DEPTH_FORMAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
        .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)];

    let depth_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .depth_stencil_attachment(&depth_ref)];

    let dependencies = [vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::LATE_FRAGMENT_TESTS)
        .dst_stage_mask(vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS)
        .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
        .dst_access_mask(
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        )];

    let create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachment)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    unsafe {
        device
            .create_render_pass(&create_info, None)
            .context("failed to create shadow render pass")
    }
}

/// Create a depth-only graphics pipeline for shadow map rendering.
///
/// No fragment shader (depth writes only). Depth bias enabled for shadow acne prevention.
fn create_shadow_pipeline(
    renderer: &Renderer,
    render_pass: vk::RenderPass,
) -> Result<(vk::Pipeline, vk::PipelineLayout)> {
    let device = &renderer.device_ctx.device;

    // Pipeline layout: shared bindless set 0 + shadow push constants (64 bytes, VERTEX only).
    let bindless_layout = renderer
        .bindless
        .as_ref()
        .expect("bindless must be initialized before shadow pipeline")
        .descriptor_set_layout;

    let push_constant_ranges = [vk::PushConstantRange {
        stage_flags: vk::ShaderStageFlags::VERTEX,
        offset: 0,
        size: std::mem::size_of::<ShadowPushConstants>() as u32,
    }];
    let set_layouts = [bindless_layout];
    let pipeline_layout = unsafe {
        device
            .create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&push_constant_ranges),
                None,
            )
            .context("failed to create shadow pipeline layout")?
    };

    let vert_module = create_shader_module(
        device,
        include_bytes!(concat!(env!("OUT_DIR"), "/shadow_depth.vert.spv")),
    )?;

    let entry_name = c"main";
    let shader_stages = [vk::PipelineShaderStageCreateInfo::default()
        .module(vert_module)
        .name(entry_name)
        .stage(vk::ShaderStageFlags::VERTEX)];

    // VB binding: stride 8 (PackedVertex = uvec2), VERTEX rate (same as meshlet draw).
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
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let dynamic_states = [
        vk::DynamicState::VIEWPORT,
        vk::DynamicState::SCISSOR,
        vk::DynamicState::DEPTH_BIAS,
    ];
    let dynamic_state_info =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .line_width(1.0)
        .cull_mode(vk::CullModeFlags::NONE) // No culling for shadow casters
        .front_face(vk::FrontFace::CLOCKWISE)
        .depth_bias_enable(true)
        .depth_bias_constant_factor(0.0)
        .depth_bias_slope_factor(0.0)
        .depth_bias_clamp(0.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    // No color attachments — depth-only.
    let color_blend = vk::PipelineColorBlendStateCreateInfo::default();
    let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(true)
        .depth_write_enable(true)
        .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);

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
        .render_pass(render_pass)
        .subpass(0)];

    let cache_handle = renderer
        .pipeline_cache
        .as_ref()
        .expect("pipeline cache must be initialized before shadow pipeline")
        .handle();
    let pipeline = unsafe {
        device
            .create_graphics_pipelines(cache_handle, &pipeline_info, None)
            .map_err(|(_, err)| err)
            .context("failed to create shadow depth graphics pipeline")?
            .into_iter()
            .next()
            .context("shadow pipeline creation returned no pipeline")?
    };

    unsafe {
        device.destroy_shader_module(vert_module, None);
    }

    Ok((pipeline, pipeline_layout))
}
