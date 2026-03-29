//! Sky renderer with Preetham/Hosek-Wilkie atmosphere model (LGHT-05).
//!
//! Renders a fullscreen triangle at depth=1.0 (behind all geometry) with a
//! procedural sky driven by sun direction and time of day. The sky params
//! SSBO at binding 23 contains the inverse view-projection matrix for ray
//! direction reconstruction in the fragment shader.

use anyhow::{Context, Result};
use ash::vk;
use glam::Mat4;
use gpu_allocator::vulkan::Allocation;
use gpu_allocator::MemoryLocation;

use super::Renderer;
use super::bindless::BINDING_SKY_PARAMS;
use super::helpers::create_allocated_buffer;
use super::spirv::create_shader_module;
use super::swapchain::MSAA_SAMPLES;

/// Atmosphere model selector.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtmosphereModel {
    Preetham = 0,
    HosekWilkie = 1,
}

impl AtmosphereModel {
    pub fn as_str(&self) -> &'static str {
        match self {
            AtmosphereModel::Preetham => "Preetham",
            AtmosphereModel::HosekWilkie => "Hosek-Wilkie",
        }
    }
}

/// GPU-side sky/atmosphere parameters uploaded to binding 23 SSBO.
///
/// Layout matches the GLSL `SkyParams` struct in sky.frag.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SkyParams {
    pub sun_direction: [f32; 3],       // normalized
    pub turbidity: f32,                // atmospheric turbidity (default 2.0)
    pub sun_color: [f32; 3],           // sun disk color
    pub sun_angular_radius: f32,       // sun disk size (default 0.01 radians)
    pub ground_albedo: [f32; 3],       // for Hosek-Wilkie model
    pub atmosphere_model: u32,         // 0=Preetham, 1=Hosek-Wilkie
    pub inv_view_proj: [f32; 16],      // mat4 for ray direction reconstruction
    pub camera_pos: [f32; 3],
    pub _pad: f32,
}

/// Runtime-adjustable sky configuration.
#[derive(Clone, Debug)]
pub struct SkyConfig {
    pub atmosphere_model: AtmosphereModel,
    pub turbidity: f32,       // 1.0-10.0 (default 2.0)
    pub sun_angular_radius: f32, // default 0.01 radians
    pub ground_albedo: [f32; 3],
    pub enabled: bool,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self {
            atmosphere_model: AtmosphereModel::Preetham,
            turbidity: 2.0,
            sun_angular_radius: 0.01,
            ground_albedo: [0.3, 0.3, 0.3],
            enabled: true,
        }
    }
}

/// Sky renderer — fullscreen triangle pipeline with Preetham sky model.
pub struct SkyRenderer {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    /// Double-buffered SSBOs for SkyParams at binding 23.
    pub sky_params_buffers: [vk::Buffer; 2],
    pub sky_params_allocs: [Option<Allocation>; 2],
    pub config: SkyConfig,
}

impl SkyRenderer {
    /// Create SkyRenderer with fullscreen triangle pipeline, sky params SSBOs.
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let device = renderer.device_ctx.device.clone();
        let bindless_layout = renderer
            .bindless
            .as_ref()
            .expect("bindless must exist before SkyRenderer")
            .descriptor_set_layout;

        // Load sky shaders.
        let vert_spv = include_bytes!(concat!(env!("OUT_DIR"), "/sky.vert.spv"));
        let frag_spv = include_bytes!(concat!(env!("OUT_DIR"), "/sky.frag.spv"));
        let vert_module = create_shader_module(&device, vert_spv)?;
        let frag_module = create_shader_module(&device, frag_spv)?;

        let entry_name = c"main";
        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(entry_name),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(entry_name),
        ];

        // No vertex input — fullscreen triangle generated from gl_VertexIndex.
        let vertex_input_state = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);

        // Dynamic viewport/scissor.
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        // Rasterization: fill, no cull (fullscreen triangle).
        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
            .line_width(1.0);

        // MSAA must match the main render pass.
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(MSAA_SAMPLES);

        // Depth test: LESS_OR_EQUAL, no depth write.
        // Sky renders at depth=1.0 — geometry at depth < 1.0 will overwrite.
        let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(false)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);

        // Standard alpha blending (opaque sky).
        let color_blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
            .blend_enable(false)
            .color_write_mask(
                vk::ColorComponentFlags::R
                    | vk::ColorComponentFlags::G
                    | vk::ColorComponentFlags::B
                    | vk::ColorComponentFlags::A,
            )];
        let color_blend =
            vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_blend_attachment);

        // Pipeline layout: shared bindless set 0, no push constants (sky reads from SSBO).
        let set_layouts = [bindless_layout];
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts),
                    None,
                )
                .context("failed to create sky pipeline layout")?
        };

        let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_state)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterization)
            .multisample_state(&multisample)
            .depth_stencil_state(&depth_stencil)
            .color_blend_state(&color_blend)
            .dynamic_state(&dynamic_state)
            .layout(pipeline_layout)
            .render_pass(renderer.swapchain_ctx.render_pass)
            .subpass(0)];

        let cache = renderer
            .pipeline_cache
            .as_ref()
            .map(|pc| pc.handle())
            .unwrap_or(vk::PipelineCache::null());

        let pipeline = unsafe {
            device
                .create_graphics_pipelines(cache, &pipeline_info, None)
                .map_err(|(_pipelines, err)| {
                    anyhow::anyhow!("failed to create sky graphics pipeline: {err}")
                })?
                .into_iter()
                .next()
                .context("sky pipeline creation returned empty")?
        };

        // Clean up shader modules.
        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        // Create double-buffered sky params SSBOs.
        let size = std::mem::size_of::<SkyParams>() as u64;
        let (buf0, alloc0) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "sky-params-ssbo-0",
        )?;
        let (buf1, alloc1) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "sky-params-ssbo-1",
        )?;

        // Register frame 0 buffer at binding 23.
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_buffer(&renderer.device_ctx.device, BINDING_SKY_PARAMS, buf0, size);
        }

        Ok(Self {
            pipeline,
            pipeline_layout,
            sky_params_buffers: [buf0, buf1],
            sky_params_allocs: [Some(alloc0), Some(alloc1)],
            config: SkyConfig::default(),
        })
    }

    /// Update sky params SSBO for the current frame.
    pub fn update(
        &self,
        renderer: &Renderer,
        current_frame: usize,
        sun_direction: [f32; 3],
        sun_color: [f32; 3],
        camera_uniforms: &super::camera::CameraUniforms,
    ) {
        // Compute inverse view-projection for ray reconstruction.
        let view_proj = Mat4::from_cols_array_2d(&camera_uniforms.view_proj);
        let inv_view_proj = view_proj.inverse();

        let params = SkyParams {
            sun_direction,
            turbidity: self.config.turbidity,
            sun_color,
            sun_angular_radius: self.config.sun_angular_radius,
            ground_albedo: self.config.ground_albedo,
            atmosphere_model: self.config.atmosphere_model as u32,
            inv_view_proj: inv_view_proj.to_cols_array(),
            camera_pos: camera_uniforms.camera_pos,
            _pad: 0.0,
        };

        let data = bytemuck::bytes_of(&params);
        if let Some(alloc) = &self.sky_params_allocs[current_frame] {
            if let Some(mapped) = alloc.mapped_ptr() {
                let ptr = mapped.as_ptr() as *mut u8;
                unsafe {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
                }
            }
        }

        // Register current frame's buffer at binding 23.
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_buffer(
                &renderer.device_ctx.device,
                BINDING_SKY_PARAMS,
                self.sky_params_buffers[current_frame],
                std::mem::size_of::<SkyParams>() as u64,
            );
        }
    }

    /// Record draw commands for the sky fullscreen triangle.
    pub fn record_draw(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        bindless_set: vk::DescriptorSet,
        extent: vk::Extent2D,
    ) {
        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[bindless_set],
                &[],
            );

            // Set viewport and scissor (dynamic state).
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
            device.cmd_set_viewport(cmd, 0, &[viewport]);
            device.cmd_set_scissor(cmd, 0, &[scissor]);

            // Draw 3 vertices — fullscreen triangle trick (no VBO).
            device.cmd_draw(cmd, 3, 1, 0, 0);
        }
    }

    /// Clean up GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        unsafe {
            renderer
                .device_ctx
                .device
                .destroy_pipeline(self.pipeline, None);
            renderer
                .device_ctx
                .device
                .destroy_pipeline_layout(self.pipeline_layout, None);
        }

        for i in 0..2 {
            if let Some(alloc) = self.sky_params_allocs[i].take() {
                super::helpers::destroy_allocated_buffer(
                    renderer,
                    self.sky_params_buffers[i],
                    alloc,
                )?;
            }
        }

        Ok(())
    }
}
