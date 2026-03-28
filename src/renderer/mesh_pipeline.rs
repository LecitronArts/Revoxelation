use anyhow::{Context, Result};
use ash::vk;

use super::{Renderer, chunk_pool::ChunkPool, spirv::create_shader_module};
use super::camera::CameraUniforms;
use super::swapchain::MSAA_SAMPLES;
use super::chunk_pool::MeshletPool;
use super::cull_pipeline::MeshletCullPushConstants;

// ============================================================================
// MeshletPipeline trait — abstracts meshlet rendering backend (D-06)
// ============================================================================

/// Abstracts the meshlet rendering backend.
///
/// `ComputeIndirectPath` and `MeshShaderPath` both implement this trait.
/// Selection occurs once at startup based on VK_EXT_mesh_shader support.
pub trait MeshletPipeline {
    /// Record draw commands for visible meshlets into the command buffer.
    ///
    /// - `cmd`: Vulkan command buffer (inside an active render pass).
    /// - `bindless_set`: shared descriptor set 0 containing all meshlet SSBOs.
    /// - `camera`: push constant camera uniforms.
    /// - `meshlet_pool`: GPU buffers for meshlet data (VB, IB, indirect, count).
    /// - `max_draw_count`: upper bound on indirect draw count (meshlet capacity).
    /// - `extent`: swapchain extent for viewport/scissor.
    #[allow(clippy::too_many_arguments)]
    fn record_draw(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        bindless_set: vk::DescriptorSet,
        camera: &CameraUniforms,
        meshlet_pool: &MeshletPool,
        max_draw_count: u32,
        extent: vk::Extent2D,
    );

    /// Destroy Vulkan resources owned by this pipeline.
    fn destroy_resources(&mut self, device: &ash::Device);
}

// ============================================================================
// ComputeIndirectPath — compute cull + indirect draw (D-07)
// ============================================================================

/// Software mesh shader emulation via compute cull + vkCmdDrawIndexedIndirectCount.
///
/// Uses meshlet_draw.vert/frag shaders. The vertex shader reads meshlet data
/// via gl_DrawID -> visible_meshlet_buffer -> GpuMeshlet -> GpuChunkInstance.
///
/// VB binding: meshlet_vertex_buffer (PackedVertex, stride 8)
/// IB binding: meshlet_tri_buffer (u32 indices, INDEX_TYPE_UINT32)
pub struct ComputeIndirectPath {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
}

impl ComputeIndirectPath {
    /// Create the meshlet draw graphics pipeline using meshlet_draw.vert/frag SPIR-V.
    ///
    /// Pipeline layout: shared bindless set 0 + CameraUniforms push constants (80 bytes).
    /// Vertex input: PackedVertex (uvec2, stride 8, VERTEX rate).
    pub fn new(renderer: &Renderer, bindless_layout: vk::DescriptorSetLayout) -> Result<Self> {
        let device = &renderer.device_ctx.device;

        // Push constant range for CameraUniforms (80 bytes, VERTEX stage)
        let push_constant_ranges = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX,
            offset: 0,
            size: std::mem::size_of::<CameraUniforms>() as u32,
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
                .context("failed to create meshlet draw pipeline layout")?
        };

        let vert_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/meshlet_draw.vert.spv")),
        )?;
        let frag_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/meshlet_draw.frag.spv")),
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

        // VB binding: stride 8 (PackedVertex = uvec2), VERTEX rate
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
        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info =
            vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
        let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .line_width(1.0)
            .cull_mode(vk::CullModeFlags::NONE)
            .front_face(vk::FrontFace::CLOCKWISE);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(MSAA_SAMPLES);
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
            .expect("pipeline cache must be initialized before meshlet draw pipeline")
            .handle();
        let pipeline = unsafe {
            device
                .create_graphics_pipelines(cache_handle, &pipeline_info, None)
                .map_err(|(_, err)| err)
                .context("failed to create meshlet draw graphics pipeline")?
                .into_iter()
                .next()
                .context("meshlet draw pipeline creation returned no pipeline")?
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

    pub fn destroy(self, renderer: &Renderer) {
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
    }
}

impl MeshletPipeline for ComputeIndirectPath {
    fn record_draw(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        bindless_set: vk::DescriptorSet,
        camera: &CameraUniforms,
        meshlet_pool: &MeshletPool,
        max_draw_count: u32,
        extent: vk::Extent2D,
    ) {
        // Negative-height viewport flips Vulkan's Y-down clip space to Y-up,
        // matching glam's perspective_rh (OpenGL convention). Core since Vulkan 1.1.
        let viewport = vk::Viewport {
            x: 0.0,
            y: extent.height as f32,
            width: extent.width as f32,
            height: -(extent.height as f32),
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent,
        };

        let vertex_buffers = [meshlet_pool.meshlet_vertex_buffer];
        let vertex_offsets: [vk::DeviceSize; 1] = [0];

        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            device.cmd_set_viewport(cmd, 0, &[viewport]);
            device.cmd_set_scissor(cmd, 0, &[scissor]);
            device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(camera),
            );
            // Bind the shared bindless descriptor set 0
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[bindless_set],
                &[],
            );
            // Bind meshlet_vertex_buffer as VB 0 (PackedVertex, stride 8)
            device.cmd_bind_vertex_buffers(cmd, 0, &vertex_buffers, &vertex_offsets);
            // Bind meshlet_tri_buffer as IB (INDEX_TYPE_UINT32 — widened from u8 during upload)
            device.cmd_bind_index_buffer(
                cmd,
                meshlet_pool.meshlet_tri_buffer,
                0,
                vk::IndexType::UINT32,
            );
            // vkCmdDrawIndexedIndirectCount: draw visible meshlets
            //   - indirect buffer: meshlet_indirect_buffer (binding 14)
            //   - count buffer: meshlet_count_buffer (binding 15)
            //   - max_draw_count: meshlet capacity
            //   - stride: 20 bytes (VkDrawIndexedIndirectCommand = 5 x u32)
            device.cmd_draw_indexed_indirect_count(
                cmd,
                meshlet_pool.meshlet_indirect_buffer,
                0,
                meshlet_pool.meshlet_count_buffer,
                0,
                max_draw_count,
                std::mem::size_of::<vk::DrawIndexedIndirectCommand>() as u32,
            );
        }
    }

    fn destroy_resources(&mut self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}

// ============================================================================
// MeshShaderPath — VK_EXT_mesh_shader hardware path (MSHL-04)
// ============================================================================

/// Hardware mesh shader rendering via VK_EXT_mesh_shader (task + mesh + fragment).
///
/// The task shader performs per-meshlet culling (same as meshlet_cull.comp)
/// and emits visible meshlets to the mesh shader. The mesh shader reads
/// meshlet vertex/triangle data from SSBOs and outputs transformed geometry.
///
/// When this path is active, meshlet_cull.comp is NOT dispatched.
/// The chunk-level cull (chunk_cull.comp) still runs.
pub struct MeshShaderPath {
    pub pipeline: vk::Pipeline,
    pub pipeline_layout: vk::PipelineLayout,
    /// ash mesh shader extension dispatch table (for cmd_draw_mesh_tasks_ext).
    pub mesh_shader_fn: ash::ext::mesh_shader::Device,
}

/// Combined push constant layout for MeshShaderPath.
///
/// Bytes [0..40): MeshletCullPushConstants (used by task shader for culling).
/// Bytes [40..48): padding for mat4 alignment.
/// Bytes [48..128): CameraUniforms (used by mesh shader for vertex transform).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshShaderPushConstants {
    /// Task shader push constants (culling config, 40 bytes).
    pub cull: MeshletCullPushConstants,
    /// Padding to align camera to 16-byte boundary (mat4 requires it).
    pub _pad_align: [u32; 2],
    /// Mesh shader push constants (camera uniforms, 80 bytes).
    pub camera: CameraUniforms,
}

impl MeshShaderPath {
    /// Create the mesh shader graphics pipeline (task + mesh + fragment stages).
    ///
    /// Pipeline layout: shared bindless set 0 + extended push constants (128 bytes:
    /// 40 bytes MeshletCullPushConstants for task shader + 8 bytes padding + 80 bytes CameraUniforms for mesh shader).
    /// No vertex input — mesh shader generates vertices from SSBOs.
    pub fn new(
        renderer: &Renderer,
        bindless_layout: vk::DescriptorSetLayout,
        mesh_shader_fn: ash::ext::mesh_shader::Device,
    ) -> Result<Self> {
        let device = &renderer.device_ctx.device;

        // Push constant range for combined task+mesh push constants (128 bytes).
        // TASK_BIT_EXT for task shader (cull config at offset 0..40),
        // MESH_BIT_EXT for mesh shader (camera uniforms at offset 48..128).
        let push_constant_ranges = [
            vk::PushConstantRange {
                stage_flags: vk::ShaderStageFlags::TASK_EXT,
                offset: 0,
                size: std::mem::size_of::<MeshletCullPushConstants>() as u32,
            },
            vk::PushConstantRange {
                stage_flags: vk::ShaderStageFlags::MESH_EXT,
                offset: 48, // 40 bytes cull + 8 bytes padding for mat4 alignment
                size: std::mem::size_of::<CameraUniforms>() as u32,
            },
        ];

        let set_layouts = [bindless_layout];
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default()
                        .set_layouts(&set_layouts)
                        .push_constant_ranges(&push_constant_ranges),
                    None,
                )
                .context("failed to create mesh shader pipeline layout")?
        };

        let task_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/meshlet.task.spv")),
        )?;
        let mesh_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/meshlet.mesh.spv")),
        )?;
        let frag_module = create_shader_module(
            device,
            include_bytes!(concat!(env!("OUT_DIR"), "/meshlet_draw.frag.spv")),
        )?;

        let entry_name = c"main";
        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .module(task_module)
                .name(entry_name)
                .stage(vk::ShaderStageFlags::TASK_EXT),
            vk::PipelineShaderStageCreateInfo::default()
                .module(mesh_module)
                .name(entry_name)
                .stage(vk::ShaderStageFlags::MESH_EXT),
            vk::PipelineShaderStageCreateInfo::default()
                .module(frag_module)
                .name(entry_name)
                .stage(vk::ShaderStageFlags::FRAGMENT),
        ];

        // No vertex input — mesh shader generates vertices from SSBOs.
        let vertex_input_state = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
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
            .rasterization_samples(MSAA_SAMPLES);
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
            .expect("pipeline cache must be initialized before mesh shader pipeline")
            .handle();
        let pipeline = unsafe {
            device
                .create_graphics_pipelines(cache_handle, &pipeline_info, None)
                .map_err(|(_, err)| err)
                .context("failed to create mesh shader graphics pipeline")?
                .into_iter()
                .next()
                .context("mesh shader pipeline creation returned no pipeline")?
        };

        unsafe {
            device.destroy_shader_module(task_module, None);
            device.destroy_shader_module(mesh_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        Ok(Self {
            pipeline,
            pipeline_layout,
            mesh_shader_fn,
        })
    }

    pub fn destroy(self, renderer: &Renderer) {
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
    }
}

impl MeshletPipeline for MeshShaderPath {
    fn record_draw(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        bindless_set: vk::DescriptorSet,
        camera: &CameraUniforms,
        meshlet_pool: &MeshletPool,
        max_draw_count: u32,
        extent: vk::Extent2D,
    ) {
        let _ = max_draw_count; // mesh shader path does not use indirect count
        let total_meshlets = meshlet_pool.active_meshlet_count();
        if total_meshlets == 0 {
            return;
        }

        // Negative-height viewport flips Vulkan's Y-down clip space to Y-up.
        let viewport = vk::Viewport {
            x: 0.0,
            y: extent.height as f32,
            width: extent.width as f32,
            height: -(extent.height as f32),
            min_depth: 0.0,
            max_depth: 1.0,
        };
        let scissor = vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent,
        };

        // Build combined push constants: cull config (task) + camera (mesh).
        // Cull toggles are all enabled by default. The caller can extend the
        // MeshletPipeline trait in the future to pass per-frame cull config.
        let cull_pc = MeshletCullPushConstants {
            total_meshlet_count: total_meshlets,
            enable_backface: 1,
            enable_frustum: 1,
            enable_hiz: 1,
            camera_pos: camera.camera_pos,
            _pad: 0,
            sse_threshold: 2.0,
            screen_height: extent.height as f32,
        };
        let combined_pc = MeshShaderPushConstants {
            cull: cull_pc,
            _pad_align: [0; 2],
            camera: *camera,
        };

        unsafe {
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            device.cmd_set_viewport(cmd, 0, &[viewport]);
            device.cmd_set_scissor(cmd, 0, &[scissor]);

            // Push constants split into two calls to match pipeline layout ranges (CRIT-03).
            // Range 1: TASK_EXT at offset 0, size 40 (MeshletCullPushConstants).
            // Range 2: MESH_EXT at offset 48, size 80 (CameraUniforms).
            // A single push covering [0..128) would span the gap at [40..48),
            // violating the Vulkan spec requirement that push constant data must
            // exactly match a declared range.
            let combined_bytes = bytemuck::bytes_of(&combined_pc);
            device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::TASK_EXT,
                0,
                &combined_bytes[0..40],
            );
            device.cmd_push_constants(
                cmd,
                self.pipeline_layout,
                vk::ShaderStageFlags::MESH_EXT,
                48,
                &combined_bytes[48..128],
            );

            // Bind the shared bindless descriptor set 0.
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[bindless_set],
                &[],
            );

            // Dispatch task+mesh shader: one workgroup per 32 meshlets.
            let group_count_x = total_meshlets.div_ceil(32);
            self.mesh_shader_fn.cmd_draw_mesh_tasks(cmd, group_count_x, 1, 1);
        }
    }

    fn destroy_resources(&mut self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline(self.pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);
        }
    }
}

/// Create the appropriate meshlet rendering pipeline based on mesh shader support.
///
/// If `mesh_shader_supported` is true and a mesh shader dispatch table is available,
/// creates a `MeshShaderPath`. Otherwise falls back to `ComputeIndirectPath`.
///
/// Returns a boxed trait object for runtime polymorphism.
pub fn create_meshlet_pipeline(
    renderer: &Renderer,
    bindless_layout: vk::DescriptorSetLayout,
) -> Result<Box<dyn MeshletPipeline>> {
    if renderer.device_ctx.mesh_shader_supported
        && let Some(mesh_shader_fn) = renderer.device_ctx.mesh_shader_fn.clone()
    {
        log::info!("Creating MeshShaderPath (VK_EXT_mesh_shader hardware path)");
        let path = MeshShaderPath::new(renderer, bindless_layout, mesh_shader_fn)?;
        return Ok(Box::new(path));
    }
    log::info!("Creating ComputeIndirectPath (compute+indirect fallback)");
    let path = ComputeIndirectPath::new(renderer, bindless_layout)?;
    Ok(Box::new(path))
}

// ============================================================================
// ChunkMeshPipeline — legacy per-chunk draw path (retained behind runtime flag)
// ============================================================================

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
            .rasterization_samples(MSAA_SAMPLES);
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
    #[allow(clippy::too_many_arguments)]
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
        // Negative-height viewport flips Vulkan's Y-down clip space to Y-up,
        // matching glam's perspective_rh (OpenGL convention). Core since Vulkan 1.1.
        let viewport = vk::Viewport {
            x: 0.0,
            y: extent.height as f32,
            width: extent.width as f32,
            height: -(extent.height as f32),
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
