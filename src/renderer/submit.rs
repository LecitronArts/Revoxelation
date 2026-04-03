use anyhow::{Context, Result};
use ash::vk;
use glam::Mat4;

use super::Renderer;
use super::camera::{CameraUniforms, extract_frustum_planes};
use super::cull_pipeline::{HiZConfig, MeshletCullPushConstants};

pub fn submit_frame_sequence() -> &'static [&'static str] {
    &[
        "staging_ring_reset",
        "chunk_delta_uploads",
        "transfer_to_compute_barrier",
        "compute_cull_chunk",
        "chunk_to_meshlet_barrier",
        "compute_cull_meshlet",
        "meshlet_cull_to_draw_barrier",
        "indirect_barrier",
        "render_pass",
        "sky_draw",
        "meshlet_draw_or_chunk_draw",
        "egui",
        "hiz_generate",
        "ssao_compute",
    ]
}

/// Whether a frame was submitted or swapchain recreation is needed.
pub enum FrameOutcome {
    /// Frame submitted and presented successfully.
    Submitted,
    /// Swapchain is out of date — caller must recreate before next frame.
    NeedsRecreate,
}

// ---------------------------------------------------------------------------
// REFAC-02: Named sub-functions decomposed from submit_frame.
// ---------------------------------------------------------------------------

/// Wait for the in-flight fence and perform post-fence bookkeeping
/// (staging ring reset, GPU readback, capacity growth check).
///
/// Returns the current frame's command_buffer, image_available semaphore,
/// render_finished semaphore, and in_flight fence.
unsafe fn wait_fence_and_prepare(renderer: &mut Renderer) -> Result<()> {
    let current_frame = renderer.current_frame;
    let in_flight = renderer.frames[current_frame].in_flight;

    unsafe {
        renderer
            .device_ctx
            .device
            .wait_for_fences(&[in_flight], true, u64::MAX)
            .context("failed waiting for Vulkan in-flight fence")?;
    }

    // After fence wait, the staging ring region for this frame is safe to reuse.
    if let Some(staging_ring) = renderer.staging_ring.as_mut() {
        staging_ring.reset_current_frame();
    }

    // POLISH-06: Read previous frame's GPU readback data (after fence wait, safe to read).
    if let Some(readback) = &renderer.readback_counters {
        renderer.last_gpu_visible_meshlets = readback.read_previous_frame(current_frame);
    }

    // D-05: Check if ChunkPool needs capacity growth (after fence wait, before recording).
    {
        let needs = renderer
            .chunk_pool
            .as_ref()
            .is_some_and(|cp| cp.needs_grow());
        if needs {
            let mut chunk_pool = renderer.chunk_pool.take().unwrap();
            let bindless = renderer
                .bindless
                .take()
                .expect("bindless must exist for growth");
            if let Err(e) = chunk_pool.grow_capacity(renderer, &bindless) {
                log::error!("ChunkPool growth failed: {e:#}");
            }
            renderer.bindless = Some(bindless);
            renderer.chunk_pool = Some(chunk_pool);
        }
    }

    // Grow meshlet storage at the same safe point: after fence wait and before
    // any command recording touches the buffers for this frame.
    {
        let needs = renderer
            .meshlet_pool
            .as_ref()
            .is_some_and(|mp| mp.needs_grow());
        if needs {
            let mut meshlet_pool = renderer.meshlet_pool.take().unwrap();
            let bindless = renderer
                .bindless
                .take()
                .expect("bindless must exist for growth");
            if let Err(e) = meshlet_pool.grow_capacity(renderer, &bindless) {
                log::error!("MeshletPool growth failed: {e:#}");
            }
            renderer.bindless = Some(bindless);
            renderer.meshlet_pool = Some(meshlet_pool);
        }
    }

    Ok(())
}

/// Acquire the next swapchain image. Returns (image_index, suboptimal) or NeedsRecreate.
unsafe fn acquire_image(renderer: &Renderer) -> Result<Option<(u32, bool)>> {
    let current_frame = renderer.current_frame;
    let image_available = renderer.frames[current_frame].image_available;

    let acquire_result = unsafe {
        renderer.swapchain_ctx.swapchain_loader.acquire_next_image(
            renderer.swapchain_ctx.swapchain,
            u64::MAX,
            image_available,
            vk::Fence::null(),
        )
    };
    match acquire_result {
        Ok((idx, sub)) => Ok(Some((idx, sub))),
        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
            log::info!(
                "acquire_next_image returned ERROR_OUT_OF_DATE_KHR — requesting swapchain recreation"
            );
            Ok(None)
        }
        Err(e) => Err(anyhow::anyhow!(
            "failed to acquire Vulkan swapchain image: {e}"
        )),
    }
}

/// Begin recording into the frame's command buffer.
unsafe fn begin_command_buffer(renderer: &mut Renderer) -> Result<vk::CommandBuffer> {
    let current_frame = renderer.current_frame;
    let command_buffer = renderer.frames[current_frame].command_buffer;
    let in_flight = renderer.frames[current_frame].in_flight;

    unsafe {
        renderer
            .device_ctx
            .device
            .reset_fences(&[in_flight])
            .context("failed to reset Vulkan in-flight fence")?;
        renderer
            .device_ctx
            .device
            .reset_command_buffer(command_buffer, vk::CommandBufferResetFlags::empty())
            .context("failed to reset Vulkan command buffer")?;
        renderer
            .device_ctx
            .device
            .begin_command_buffer(command_buffer, &vk::CommandBufferBeginInfo::default())
            .context("failed to begin Vulkan command buffer")?;
    }

    Ok(command_buffer)
}

/// Submit order note: `record_csm_shadow_passes` must run before
/// `dispatch_chunk_cull` so shadow casters are built from their own draw list
/// before the main-view cull overwrites the shared meshlet buffers.
///
/// Dispatch chunk-level and meshlet-level culling compute passes.
unsafe fn dispatch_chunk_cull(
    renderer: &mut Renderer,
    command_buffer: vk::CommandBuffer,
    camera_uniforms: &CameraUniforms,
) {
    let Some(cull_pipeline) = &renderer.cull_pipeline else {
        return;
    };
    let Some(chunk_pool) = &renderer.chunk_pool else {
        return;
    };

    let view_proj = Mat4::from_cols_array_2d(&camera_uniforms.view_proj);
    let frustum_planes = extract_frustum_planes(&view_proj);
    let active_draw_count = chunk_pool.active_draw_count();

    // Upload Hi-Z config to the cull pipeline SSBO.
    let hiz_enabled = renderer.hiz_pyramid.is_some();
    let (hiz_w, hiz_h, hiz_mips) = if let Some(hiz) = &renderer.hiz_pyramid {
        (hiz.width as f32, hiz.height as f32, hiz.mip_count)
    } else {
        (1.0, 1.0, 1)
    };
    let hiz_config = HiZConfig {
        view_proj: camera_uniforms.view_proj,
        hiz_size: [hiz_w, hiz_h],
        hiz_enabled: if hiz_enabled { 1 } else { 0 },
        hiz_mip_count: hiz_mips,
    };
    cull_pipeline.upload_hiz_config(&hiz_config);

    let bindless_set = renderer
        .bindless
        .as_ref()
        .map(|b| b.descriptor_set)
        .unwrap_or(vk::DescriptorSet::null());

    // ---- Level 1: Chunk-level frustum + Hi-Z culling ----
    cull_pipeline.dispatch(
        &renderer.device_ctx.device,
        command_buffer,
        active_draw_count,
        chunk_pool.scene_buffer_capacity() as u32,
        &frustum_planes,
        bindless_set,
    );

    // ---- Barrier: chunk_cull COMPUTE_WRITE → meshlet_cull COMPUTE_READ (D-09) ----
    let chunk_to_meshlet_barrier = vk::MemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ);
    unsafe {
        renderer.device_ctx.device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[chunk_to_meshlet_barrier],
            &[],
            &[],
        );
    }

    // ---- Level 2: Meshlet-level backface + frustum + Hi-Z culling ----
    // Skipped when mesh shader path is active (task shader does the culling).
    if !renderer.use_mesh_shader_path
        && let (Some(meshlet_cull), Some(meshlet_pool)) =
            (&renderer.meshlet_cull_pipeline, &renderer.meshlet_pool)
    {
        let total_meshlets = meshlet_pool.active_meshlet_count();
        if total_meshlets > 0 {
            let meshlet_pc = MeshletCullPushConstants {
                total_meshlet_count: total_meshlets,
                enable_backface: u32::from(renderer.meshlet_cull_backface),
                enable_frustum: u32::from(renderer.meshlet_cull_frustum),
                enable_hiz: u32::from(renderer.meshlet_cull_hiz),
                camera_pos: camera_uniforms.camera_pos,
                _pad: 0,
                sse_threshold: renderer.sse_threshold,
                screen_height: renderer.swapchain_ctx.extent.height as f32,
            };
            meshlet_cull.record_dispatch(
                &renderer.device_ctx.device,
                command_buffer,
                &meshlet_pc,
                bindless_set,
                meshlet_pool.meshlet_count_buffer,
            );

            // Barrier: meshlet cull COMPUTE_WRITE → INDIRECT_READ + VERTEX_INPUT.
            let meshlet_visible_barrier = vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::SHADER_READ
                        | vk::AccessFlags::INDIRECT_COMMAND_READ
                        | vk::AccessFlags::VERTEX_ATTRIBUTE_READ
                        | vk::AccessFlags::INDEX_READ,
                )
                .buffer(meshlet_pool.visible_meshlet_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            let meshlet_count_barrier = vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::SHADER_READ | vk::AccessFlags::INDIRECT_COMMAND_READ,
                )
                .buffer(meshlet_pool.meshlet_count_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            let meshlet_indirect_barrier = vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::INDIRECT_COMMAND_READ)
                .buffer(meshlet_pool.meshlet_indirect_buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE);
            unsafe {
                renderer.device_ctx.device.cmd_pipeline_barrier(
                    command_buffer,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::PipelineStageFlags::COMPUTE_SHADER
                        | vk::PipelineStageFlags::DRAW_INDIRECT
                        | vk::PipelineStageFlags::VERTEX_INPUT
                        | vk::PipelineStageFlags::VERTEX_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[
                        meshlet_visible_barrier,
                        meshlet_count_barrier,
                        meshlet_indirect_barrier,
                    ],
                    &[],
                );
            }
        }
    }

    // POLISH-06: Copy meshlet_count_buffer to readback buffer.
    if let (Some(readback), Some(meshlet_pool)) =
        (&renderer.readback_counters, &renderer.meshlet_pool)
    {
        let current_frame = renderer.current_frame;
        readback.record_copy(
            &renderer.device_ctx.device,
            command_buffer,
            current_frame,
            meshlet_pool.meshlet_count_buffer,
        );
    }

    // Barrier: compute shader writes → indirect draw reads.
    let dense_barrier = vk::BufferMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::INDIRECT_COMMAND_READ)
        .buffer(chunk_pool.scene_buffer())
        .offset(chunk_pool.dense_indirect_region_offset())
        .size(vk::WHOLE_SIZE);
    let draw_count_barrier = vk::BufferMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::INDIRECT_COMMAND_READ)
        .buffer(cull_pipeline.draw_count_buffer())
        .offset(0)
        .size(vk::WHOLE_SIZE);
    unsafe {
        renderer.device_ctx.device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::DRAW_INDIRECT,
            vk::DependencyFlags::empty(),
            &[],
            &[dense_barrier, draw_count_barrier],
            &[],
        );
    }
}

/// Begin the MSAA render pass with clear values.
///
/// Clear color is dynamically computed from the sky's zenith color at the
/// current time_of_day to roughly match the procedural sky (LGHT-05).
unsafe fn begin_render_pass(
    renderer: &Renderer,
    command_buffer: vk::CommandBuffer,
    image_index: u32,
) {
    // Compute dynamic clear color from day-night cycle (LGHT-05).
    let clear_color = if let Some(ls) = &renderer.lighting_state {
        if ls.use_day_night_cycle {
            let fog_c = ls.day_night.fog_color();
            // Darken the fog color slightly for the zenith approximation.
            [fog_c[0] * 0.8, fog_c[1] * 0.8, fog_c[2] * 0.9, 1.0]
        } else {
            [0.1, 0.1, 0.15, 1.0]
        }
    } else {
        [0.1, 0.1, 0.15, 1.0]
    };

    let clear_values = [
        // Attachment 0: MSAA color
        vk::ClearValue {
            color: vk::ClearColorValue {
                float32: clear_color,
            },
        },
        // Attachment 1: MSAA depth
        vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: 1.0,
                stencil: 0,
            },
        },
        // Attachment 2: Resolve color (swapchain) — cleared by resolve
        vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 1.0],
            },
        },
        // Attachment 3: Resolve depth — cleared by resolve
        vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: 1.0,
                stencil: 0,
            },
        },
    ];
    let render_pass_begin = vk::RenderPassBeginInfo::default()
        .render_pass(renderer.swapchain_ctx.render_pass)
        .framebuffer(renderer.swapchain_ctx.framebuffers[image_index as usize])
        .render_area(vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: renderer.swapchain_ctx.extent,
        })
        .clear_values(&clear_values);

    unsafe {
        renderer.device_ctx.device.cmd_begin_render_pass(
            command_buffer,
            &render_pass_begin,
            vk::SubpassContents::INLINE,
        );
    }
}

/// Draw meshlets (or legacy per-chunk path).
unsafe fn draw_meshlets(
    renderer: &Renderer,
    command_buffer: vk::CommandBuffer,
    camera_uniforms: &CameraUniforms,
    current_time: f32,
) {
    let bindless_set = renderer
        .bindless
        .as_ref()
        .map(|b| b.descriptor_set)
        .unwrap_or(vk::DescriptorSet::null());

    let used_meshlet_path = renderer.use_meshlet_rendering
        && renderer.meshlet_pipeline.is_some()
        && renderer.meshlet_pool.is_some();

    if used_meshlet_path {
        if let (Some(meshlet_pipeline), Some(meshlet_pool)) =
            (&renderer.meshlet_pipeline, &renderer.meshlet_pool)
        {
            let total_meshlets = meshlet_pool.active_meshlet_count();
            if total_meshlets > 0 {
                let extent = renderer.swapchain_ctx.extent;
                // MED-05: Use dynamic meshlet_capacity instead of hardcoded constant.
                let max_draw_count = meshlet_pool.meshlet_capacity() as u32;
                meshlet_pipeline.record_draw(
                    &renderer.device_ctx.device,
                    command_buffer,
                    bindless_set,
                    camera_uniforms,
                    meshlet_pool,
                    max_draw_count,
                    extent,
                    renderer.sse_threshold,
                    current_time,
                );
            }
        }
    } else if let (Some(mesh_pipeline), Some(chunk_pool)) =
        (&renderer.mesh_pipeline, &renderer.chunk_pool)
    {
        // Legacy per-chunk path (compute+indirect fallback when meshlet rendering is disabled)
        let active = chunk_pool.active_draw_count();
        if active > 0 {
            let max_draw_count = chunk_pool.scene_buffer_capacity() as u32;
            let draw_count_buffer = renderer
                .cull_pipeline
                .as_ref()
                .map(|cp| cp.draw_count_buffer())
                .unwrap_or(vk::Buffer::null());
            mesh_pipeline.draw(
                renderer,
                chunk_pool,
                command_buffer,
                max_draw_count,
                draw_count_buffer,
                camera_uniforms,
                bindless_set,
            );
        }
    }
}

/// Record egui overlay drawing.
fn draw_egui(renderer: &mut Renderer, command_buffer: vk::CommandBuffer) -> Result<()> {
    if let Some(mut egui_backend) = renderer.egui_backend.take() {
        let egui_output = renderer.pending_egui_output.take();
        let egui_frame = renderer.current_frame;
        if let Some(output) = egui_output {
            let paint_result = egui_backend.paint(
                renderer,
                command_buffer,
                egui_frame,
                output.textures_delta,
                output.clipped_primitives,
                output.screen_size,
            );
            renderer.egui_backend = Some(egui_backend);
            paint_result?;
        } else {
            // No egui output this frame — still process default textures.
            let extent = renderer.swapchain_ctx.extent;
            let paint_result = egui_backend.paint(
                renderer,
                command_buffer,
                egui_frame,
                egui::TexturesDelta::default(),
                Vec::new(),
                [extent.width as f32, extent.height as f32],
            );
            renderer.egui_backend = Some(egui_backend);
            paint_result?;
        }
    }
    Ok(())
}

/// Generate Hi-Z depth pyramid from the current frame's resolved depth output.
unsafe fn generate_hiz(renderer: &Renderer, command_buffer: vk::CommandBuffer) {
    if renderer.hiz_pyramid.is_none() {
        return;
    }

    // Transition depth image: DEPTH_STENCIL_ATTACHMENT_OPTIMAL → SHADER_READ_ONLY_OPTIMAL
    let depth_to_read = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .image(renderer.swapchain_ctx.depth_image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::DEPTH)
                .level_count(1)
                .layer_count(1),
        );
    unsafe {
        renderer.device_ctx.device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[depth_to_read],
        );
    }

    // Dispatch Hi-Z generation.
    if let Some(hiz) = &renderer.hiz_pyramid {
        hiz.generate(&renderer.device_ctx.device, command_buffer);
    }

    // Transition depth image back for next frame.
    let depth_to_attach = vk::ImageMemoryBarrier::default()
        .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
        .src_access_mask(vk::AccessFlags::SHADER_READ)
        .dst_access_mask(
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ,
        )
        .image(renderer.swapchain_ctx.depth_image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::DEPTH)
                .level_count(1)
                .layer_count(1),
        );
    unsafe {
        renderer.device_ctx.device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::COMPUTE_SHADER,
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[depth_to_attach],
        );
    }
}

/// Record SSAO compute + bilateral blur passes (LGHT-03).
///
/// Runs after Hi-Z generation — reads the resolved depth via binding 7 (Hi-Z mip 0).
/// Writes blurred AO to binding 17 for fragment shader consumption on this or next frame.
unsafe fn record_ssao_pass(renderer: &Renderer, command_buffer: vk::CommandBuffer) {
    let Some(ssao) = &renderer.ssao_pass else {
        return;
    };
    if !renderer.ssao_config.enabled {
        return;
    }

    if let Err(e) = ssao.refresh_descriptor_sets(renderer) {
        log::error!("failed to refresh SSAO descriptors: {e:#}");
        return;
    }

    ssao.record_dispatch(
        &renderer.device_ctx.device,
        command_buffer,
        &renderer.ssao_config,
    );
}

/// Submit command buffer and queue present. Returns true if swapchain needs recreation.
unsafe fn present(
    renderer: &Renderer,
    command_buffer: vk::CommandBuffer,
    image_index: u32,
) -> Result<bool> {
    let current_frame = renderer.current_frame;
    let image_available = renderer.frames[current_frame].image_available;
    let render_finished = renderer.frames[current_frame].render_finished;
    let in_flight = renderer.frames[current_frame].in_flight;
    let wait_semaphores = [image_available];
    let signal_semaphores = [render_finished];
    let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
    let command_buffers = [command_buffer];

    unsafe {
        renderer
            .device_ctx
            .device
            .end_command_buffer(command_buffer)
            .context("failed to end Vulkan command buffer")?;
    }

    let submit_infos = [vk::SubmitInfo::default()
        .wait_semaphores(&wait_semaphores)
        .wait_dst_stage_mask(&wait_stages)
        .command_buffers(&command_buffers)
        .signal_semaphores(&signal_semaphores)];
    unsafe {
        renderer
            .device_ctx
            .device
            .queue_submit(renderer.device_ctx.graphics_queue, &submit_infos, in_flight)
            .context("failed to submit Vulkan graphics queue")?;
    }

    let swapchains = [renderer.swapchain_ctx.swapchain];
    let image_indices = [image_index];
    let present_result = unsafe {
        renderer.swapchain_ctx.swapchain_loader.queue_present(
            renderer.device_ctx.present_queue,
            &vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(&swapchains)
                .image_indices(&image_indices),
        )
    };

    match present_result {
        Ok(false) => Ok(false),
        Ok(true) => {
            log::info!("queue_present returned SUBOPTIMAL — requesting swapchain recreation");
            Ok(true)
        }
        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
            log::info!(
                "queue_present returned ERROR_OUT_OF_DATE_KHR — requesting swapchain recreation"
            );
            Ok(true)
        }
        Err(e) => Err(anyhow::anyhow!(
            "failed to present Vulkan swapchain image: {e}"
        )),
    }
}

/// Record CSM shadow depth passes for all 4 cascades (LGHT-02).
///
/// Inserted between dispatch_chunk_cull and begin_render_pass.
/// Computes cascade matrices from camera + sun direction, renders depth-only
/// passes, then transitions shadow images to read-only for fragment sampling.
unsafe fn record_csm_shadow_passes(
    renderer: &mut Renderer,
    command_buffer: vk::CommandBuffer,
    camera_uniforms: &CameraUniforms,
) {
    if !renderer.shadow_config.enabled {
        return;
    }
    let Some(shadow_map) = &renderer.shadow_map else {
        return;
    };

    // Get sun direction from lighting state.
    let sun_direction = if let Some(ls) = &renderer.lighting_state {
        let elev = ls.sun_elevation.to_radians();
        let azim = ls.sun_azimuth.to_radians();
        let cos_elev = elev.cos();
        glam::Vec3::new(cos_elev * azim.sin(), elev.sin(), cos_elev * azim.cos())
            .normalize_or_zero()
    } else {
        glam::Vec3::new(0.0, 1.0, 0.0)
    };

    // Compute camera inverse view-proj for frustum corner extraction.
    let view_proj = Mat4::from_cols_array_2d(&camera_uniforms.view_proj);
    let view_proj_inv = view_proj.inverse();

    let camera_near = 0.1_f32;
    let camera_far = 2000.0_f32;
    let lambda = renderer.shadow_config.split_lambda;
    let resolution = shadow_map.resolution;

    let (cascade_matrices, cascade_splits) = super::shadow::compute_cascade_matrices(
        &view_proj_inv,
        camera_near,
        camera_far,
        sun_direction,
        lambda,
        resolution,
    );

    // Write cascade matrices and splits to lighting params SSBO.
    if let Some(ls) = &renderer.lighting_state {
        let current_frame = renderer.current_frame;
        if let Some(alloc) = &ls.ssbo_allocs[current_frame] {
            if let Some(mapped) = alloc.mapped_ptr() {
                let ptr = mapped.as_ptr() as *mut u8;
                // shadow_matrices start at offset 48 (3*f32 + f32 + 3*f32 + f32 + 3*f32 + f32 = 48 bytes)
                let shadow_matrices_offset = 48usize;
                let shadow_matrices_data: [[f32; 16]; 4] = [
                    cascade_matrices[0].to_cols_array(),
                    cascade_matrices[1].to_cols_array(),
                    cascade_matrices[2].to_cols_array(),
                    cascade_matrices[3].to_cols_array(),
                ];
                let matrix_bytes = bytemuck::bytes_of(&shadow_matrices_data);
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        matrix_bytes.as_ptr(),
                        ptr.add(shadow_matrices_offset),
                        matrix_bytes.len(),
                    );
                }
                // cascade_splits at offset 48 + 256 = 304
                let splits_offset = shadow_matrices_offset + std::mem::size_of::<[[f32; 16]; 4]>();
                let splits_bytes = bytemuck::bytes_of(&cascade_splits);
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        splits_bytes.as_ptr(),
                        ptr.add(splits_offset),
                        splits_bytes.len(),
                    );
                }
            }
        }
    }

    // Bind bindless descriptor set for shadow pipeline.
    let bindless_set = renderer
        .bindless
        .as_ref()
        .map(|b| b.descriptor_set)
        .unwrap_or(vk::DescriptorSet::null());

    // Record shadow depth passes for all 4 cascades.
    // Need to bind bindless set inside each cascade render pass.
    let meshlet_pool = renderer.meshlet_pool.as_ref();
    let total_meshlets = meshlet_pool
        .map(|mp| mp.active_meshlet_count())
        .unwrap_or(0);
    if total_meshlets == 0 {
        return;
    }
    let meshlet_pool = meshlet_pool.unwrap();

    let viewport = vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: resolution as f32,
        height: resolution as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    };
    let scissor = vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent: vk::Extent2D {
            width: resolution,
            height: resolution,
        },
    };
    let vertex_buffers = [meshlet_pool.meshlet_vertex_buffer];
    let vertex_offsets: [vk::DeviceSize; 1] = [0];
    let max_draw_count = meshlet_pool.meshlet_capacity() as u32;

    for cascade in 0..super::shadow::CASCADE_COUNT as usize {
        let clear_values = [vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: 1.0,
                stencil: 0,
            },
        }];
        let render_pass_begin = vk::RenderPassBeginInfo::default()
            .render_pass(shadow_map.render_pass)
            .framebuffer(shadow_map.framebuffers[cascade])
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: resolution,
                    height: resolution,
                },
            })
            .clear_values(&clear_values);

        let shadow_pc = super::shadow::ShadowPushConstants {
            light_view_proj: cascade_matrices[cascade].to_cols_array_2d(),
        };

        unsafe {
            renderer.device_ctx.device.cmd_begin_render_pass(
                command_buffer,
                &render_pass_begin,
                vk::SubpassContents::INLINE,
            );
            renderer.device_ctx.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                shadow_map.pipeline,
            );
            renderer
                .device_ctx
                .device
                .cmd_set_viewport(command_buffer, 0, &[viewport]);
            renderer
                .device_ctx
                .device
                .cmd_set_scissor(command_buffer, 0, &[scissor]);
            renderer.device_ctx.device.cmd_set_depth_bias(
                command_buffer,
                renderer.shadow_config.bias_constant,
                0.0,
                renderer.shadow_config.bias_slope,
            );
            renderer.device_ctx.device.cmd_push_constants(
                command_buffer,
                shadow_map.pipeline_layout,
                vk::ShaderStageFlags::VERTEX,
                0,
                bytemuck::bytes_of(&shadow_pc),
            );
            renderer.device_ctx.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                shadow_map.pipeline_layout,
                0,
                &[bindless_set],
                &[],
            );
            renderer.device_ctx.device.cmd_bind_vertex_buffers(
                command_buffer,
                0,
                &vertex_buffers,
                &vertex_offsets,
            );
            renderer.device_ctx.device.cmd_bind_index_buffer(
                command_buffer,
                meshlet_pool.meshlet_tri_buffer,
                0,
                vk::IndexType::UINT32,
            );
            renderer.device_ctx.device.cmd_draw_indexed_indirect_count(
                command_buffer,
                meshlet_pool.meshlet_indirect_buffer,
                0,
                meshlet_pool.meshlet_count_buffer,
                0,
                max_draw_count,
                std::mem::size_of::<vk::DrawIndexedIndirectCommand>() as u32,
            );
            renderer
                .device_ctx
                .device
                .cmd_end_render_pass(command_buffer);
        }
    }

    // Transition shadow depth images to read-only for fragment shader sampling.
    shadow_map.transition_to_read(&renderer.device_ctx.device, command_buffer);
}

// ---------------------------------------------------------------------------
// Main submit_frame — orchestrator calling the named sub-functions above.
// ---------------------------------------------------------------------------

pub fn submit_frame(
    renderer: &mut Renderer,
    _frame_index: u64,
    camera_uniforms: &CameraUniforms,
    current_time: f32,
) -> Result<FrameOutcome> {
    unsafe {
        // 1. Wait for previous frame's fence and prepare.
        wait_fence_and_prepare(renderer)?;

        // 2. Acquire swapchain image.
        let Some((image_index, _suboptimal)) = acquire_image(renderer)? else {
            return Ok(FrameOutcome::NeedsRecreate);
        };

        // 3. Begin command buffer.
        let command_buffer = begin_command_buffer(renderer)?;

        // 3.5. Upload lighting and point light data (LGHT-01).
        // Must happen before draw commands so the SSBOs are ready for fragment shaders.
        {
            let current_frame = renderer.current_frame;
            if let Some(ls) = &renderer.lighting_state {
                ls.update(renderer, current_frame);
            }
            if let Some(plm) = &renderer.point_light_manager {
                plm.upload(renderer, current_frame);
            }
        }

        // 3.6. Update sky params SSBO (LGHT-05).
        // Must happen after lighting_state.update() so sun direction is current.
        {
            let current_frame = renderer.current_frame;
            let sun_direction = renderer
                .lighting_state
                .as_ref()
                .map(|ls| ls.compute_sun_direction_pub())
                .unwrap_or([0.0, 1.0, 0.0]);
            let sun_color = renderer
                .lighting_state
                .as_ref()
                .map(|ls| ls.sun_color)
                .unwrap_or([1.0, 1.0, 1.0]);
            if let Some(sky) = &renderer.sky_renderer {
                sky.update(
                    renderer,
                    current_frame,
                    sun_direction,
                    sun_color,
                    camera_uniforms,
                );
            }
        }

        // 4. Record staging→GPU copy commands for pending chunk deltas.
        renderer.record_chunk_delta_uploads(command_buffer)?;
        renderer.record_shadow_draw_setup(command_buffer)?;

        // 5. Transfer→compute barrier.
        if renderer.chunk_pool.is_some() && renderer.staging_ring.is_some() {
            let mut dst_stages = vk::PipelineStageFlags::COMPUTE_SHADER
                | vk::PipelineStageFlags::DRAW_INDIRECT
                | vk::PipelineStageFlags::VERTEX_INPUT
                | vk::PipelineStageFlags::VERTEX_SHADER;
            if renderer.use_mesh_shader_path {
                dst_stages |= vk::PipelineStageFlags::TASK_SHADER_EXT
                    | vk::PipelineStageFlags::MESH_SHADER_EXT;
            }
            let transfer_barrier = vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::SHADER_READ
                        | vk::AccessFlags::INDIRECT_COMMAND_READ
                        | vk::AccessFlags::VERTEX_ATTRIBUTE_READ
                        | vk::AccessFlags::INDEX_READ,
                );
            renderer.device_ctx.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                dst_stages,
                vk::DependencyFlags::empty(),
                &[transfer_barrier],
                &[],
                &[],
            );
        }

        // 6. Record CSM shadow depth passes before main-view culling overwrites
        // the shared visible/indirect meshlet buffers.
        record_csm_shadow_passes(renderer, command_buffer, camera_uniforms);

        // 6.5. Dispatch chunk + meshlet culling for the main camera view.
        dispatch_chunk_cull(renderer, command_buffer, camera_uniforms);

        // 7. Begin render pass.
        begin_render_pass(renderer, command_buffer, image_index);

        // 7.5: Draw sky (fullscreen triangle behind all geometry, LGHT-05).
        if let Some(sky_renderer) = &renderer.sky_renderer {
            if sky_renderer.config.enabled {
                let bindless_set = renderer
                    .bindless
                    .as_ref()
                    .map(|b| b.descriptor_set)
                    .unwrap_or(vk::DescriptorSet::null());
                sky_renderer.record_draw(
                    &renderer.device_ctx.device,
                    command_buffer,
                    bindless_set,
                    renderer.swapchain_ctx.extent,
                );
            }
        }

        // 8. Draw meshlets (geometry overwrites sky at closer depth).
        draw_meshlets(renderer, command_buffer, camera_uniforms, current_time);

        // 9. Draw egui overlay.
        draw_egui(renderer, command_buffer)?;

        // 10. End render pass.
        renderer
            .device_ctx
            .device
            .cmd_end_render_pass(command_buffer);

        // 11. Generate Hi-Z pyramid.
        generate_hiz(renderer, command_buffer);

        // 11.5. SSAO compute + bilateral blur (LGHT-03).
        // Runs after Hi-Z generation — reads the resolved depth at binding 7 (Hi-Z mip 0).
        // AO result written to binding 17 for next frame's fragment shader consumption.
        record_ssao_pass(renderer, command_buffer);

        // 11.6. Transition CSM shadow maps back to attachment for next frame (LGHT-02).
        if renderer.shadow_config.enabled {
            if let Some(shadow_map) = &renderer.shadow_map {
                shadow_map.transition_to_attachment(&renderer.device_ctx.device, command_buffer);
            }
        }

        // 12. Submit and present.
        let needs_recreate = present(renderer, command_buffer, image_index)?;

        // Advance staging ring to next frame's region.
        if let Some(staging_ring) = renderer.staging_ring.as_mut() {
            staging_ring.advance_frame();
        }

        renderer.current_frame = (renderer.current_frame + 1) % renderer.frames.len();

        if needs_recreate {
            Ok(FrameOutcome::NeedsRecreate)
        } else {
            Ok(FrameOutcome::Submitted)
        }
    }
}
