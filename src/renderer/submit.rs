use anyhow::{Context, Result};
use ash::vk;
use glam::Mat4;

use super::Renderer;
use super::camera::{CameraUniforms, extract_frustum_planes};
use super::chunk_pool::INITIAL_MESHLET_CAPACITY;
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
        "meshlet_draw_or_chunk_draw",
        "egui",
        "hiz_generate",
    ]
}

/// Whether a frame was submitted or swapchain recreation is needed.
pub enum FrameOutcome {
    /// Frame submitted and presented successfully.
    Submitted,
    /// Swapchain is out of date — caller must recreate before next frame.
    NeedsRecreate,
}

pub fn submit_frame(renderer: &mut Renderer, _frame_index: u64, camera_uniforms: &CameraUniforms) -> Result<FrameOutcome> {
    let current_frame = renderer.current_frame;
    let command_buffer = renderer.frames[current_frame].command_buffer;
    let image_available = renderer.frames[current_frame].image_available;
    let render_finished = renderer.frames[current_frame].render_finished;
    let in_flight = renderer.frames[current_frame].in_flight;
    let wait_semaphores = [image_available];
    let signal_semaphores = [render_finished];
    let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
    let command_buffers = [command_buffer];

    let needs_recreate = unsafe {
        renderer
            .device_ctx
            .device
            .wait_for_fences(&[in_flight], true, u64::MAX)
            .context("failed waiting for Vulkan in-flight fence")?;

        // After fence wait, the staging ring region for this frame is safe to reuse.
        if let Some(staging_ring) = renderer.staging_ring.as_mut() {
            staging_ring.reset_current_frame();
        }

        // D-05: Check if ChunkPool needs capacity growth (after fence wait, before recording).
        // Growth is rare (2× doubling) and uses a one-shot command buffer with fence wait.
        {
            let needs = renderer.chunk_pool.as_ref().is_some_and(|cp| cp.needs_grow());
            if needs {
                // Temporarily take chunk_pool and bindless to satisfy borrow checker.
                let mut chunk_pool = renderer.chunk_pool.take().unwrap();
                let bindless = renderer.bindless.take().expect("bindless must exist for growth");
                if let Err(e) = chunk_pool.grow_capacity(renderer, &bindless) {
                    log::error!("ChunkPool growth failed: {e:#}");
                }
                renderer.bindless = Some(bindless);
                renderer.chunk_pool = Some(chunk_pool);
            }
        }

        // D-05: ERROR_OUT_OF_DATE_KHR from acquire_next_image → skip frame, recreate.
        let acquire_result = renderer
            .swapchain_ctx
            .swapchain_loader
            .acquire_next_image(
                renderer.swapchain_ctx.swapchain,
                u64::MAX,
                image_available,
                vk::Fence::null(),
            );
        let (image_index, _suboptimal) = match acquire_result {
            Ok(pair) => pair,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                log::info!("acquire_next_image returned ERROR_OUT_OF_DATE_KHR — requesting swapchain recreation");
                return Ok(FrameOutcome::NeedsRecreate);
            }
            Err(e) => {
                return Err(anyhow::anyhow!("failed to acquire Vulkan swapchain image: {e}"));
            }
        };

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

        // Record staging→GpuOnly copy commands for pending chunk deltas.
        renderer.record_chunk_delta_uploads(command_buffer)?;

        // Memory barrier: ensure all transfer writes complete before compute shader reads.
        if renderer.chunk_pool.is_some() && renderer.staging_ring.is_some() {
            let transfer_barrier = vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::SHADER_READ
                        | vk::AccessFlags::VERTEX_ATTRIBUTE_READ
                        | vk::AccessFlags::INDEX_READ,
                );
            renderer.device_ctx.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER
                    | vk::PipelineStageFlags::VERTEX_INPUT
                    | vk::PipelineStageFlags::VERTEX_SHADER,
                vk::DependencyFlags::empty(),
                &[transfer_barrier],
                &[],
                &[],
            );
        }

        // Upload Hi-Z config and dispatch cull shader.
        if let (Some(cull_pipeline), Some(chunk_pool)) =
            (&renderer.cull_pipeline, &renderer.chunk_pool)
        {
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

            // Pass the shared bindless descriptor set to the cull dispatch.
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
            // Ensure chunk cull writes complete before meshlet cull reads.
            let chunk_to_meshlet_barrier = vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);
            renderer.device_ctx.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[chunk_to_meshlet_barrier],
                &[],
                &[],
            );

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
                    };
                    meshlet_cull.record_dispatch(
                        &renderer.device_ctx.device,
                        command_buffer,
                        &meshlet_pc,
                        bindless_set,
                        meshlet_pool.meshlet_count_buffer,
                    );

                    // Barrier: meshlet cull COMPUTE_WRITE → INDIRECT_READ + VERTEX_INPUT for draw commands.
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
                            vk::AccessFlags::SHADER_READ
                                | vk::AccessFlags::INDIRECT_COMMAND_READ,
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
                    renderer.device_ctx.device.cmd_pipeline_barrier(
                        command_buffer,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                        vk::PipelineStageFlags::COMPUTE_SHADER
                            | vk::PipelineStageFlags::DRAW_INDIRECT
                            | vk::PipelineStageFlags::VERTEX_INPUT
                            | vk::PipelineStageFlags::VERTEX_SHADER,
                        vk::DependencyFlags::empty(),
                        &[],
                        &[meshlet_visible_barrier, meshlet_count_barrier, meshlet_indirect_barrier],
                        &[],
                    );
                }
            }

            // Barrier: compute shader writes → indirect draw reads for both dense indirect
            // (within scene_buffer) and draw count buffers.
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

        let clear_values = [
            vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.1, 0.1, 0.15, 1.0],
                },
            },
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

        renderer.device_ctx.device.cmd_begin_render_pass(
            command_buffer,
            &render_pass_begin,
            vk::SubpassContents::INLINE,
        );

        // ---- Meshlet rendering path (default) vs legacy per-chunk path ----
        let bindless_set = renderer
            .bindless
            .as_ref()
            .map(|b| b.descriptor_set)
            .unwrap_or(vk::DescriptorSet::null());

        let used_meshlet_path = renderer.use_meshlet_rendering
            && renderer.meshlet_pipeline.is_some()
            && renderer.meshlet_pool.is_some();

        if used_meshlet_path {
            // Meshlet rendering: ComputeIndirectPath::record_draw via MeshletPipeline trait
            if let (Some(meshlet_pipeline), Some(meshlet_pool)) =
                (&renderer.meshlet_pipeline, &renderer.meshlet_pool)
            {
                let total_meshlets = meshlet_pool.active_meshlet_count();
                if total_meshlets > 0 {
                    let extent = renderer.swapchain_ctx.extent;
                    meshlet_pipeline.record_draw(
                        &renderer.device_ctx.device,
                        command_buffer,
                        bindless_set,
                        camera_uniforms,
                        meshlet_pool,
                        INITIAL_MESHLET_CAPACITY as u32,
                        extent,
                    );
                }
            }
        } else if let (Some(mesh_pipeline), Some(chunk_pool)) =
            (&renderer.mesh_pipeline, &renderer.chunk_pool)
        {
            // Legacy per-chunk path (fallback when meshlet rendering is disabled)
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

        renderer.device_ctx.device.cmd_end_render_pass(command_buffer);

        // After render pass: generate Hi-Z depth pyramid from the current frame's depth output
        // for the NEXT frame's occlusion culling (D-04: 1-frame temporal latency).
        if renderer.hiz_pyramid.is_some() {
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
            renderer.device_ctx.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[depth_to_read],
            );

            // Dispatch Hi-Z generation.
            if let Some(hiz) = &renderer.hiz_pyramid {
                hiz.generate(&renderer.device_ctx.device, command_buffer);
            }

            // Transition depth image back: SHADER_READ_ONLY_OPTIMAL → DEPTH_STENCIL_ATTACHMENT_OPTIMAL
            // for the next frame's render pass.
            let depth_to_attach = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
                .src_access_mask(vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ)
                .image(renderer.swapchain_ctx.depth_image)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::DEPTH)
                        .level_count(1)
                        .layer_count(1),
                );
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

        renderer
            .device_ctx
            .device
            .end_command_buffer(command_buffer)
            .context("failed to end Vulkan command buffer")?;

        let submit_infos = [vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&command_buffers)
            .signal_semaphores(&signal_semaphores)];
        renderer
            .device_ctx
            .device
            .queue_submit(
                renderer.device_ctx.graphics_queue,
                &submit_infos,
                in_flight,
            )
            .context("failed to submit Vulkan graphics queue")?;

        let swapchains = [renderer.swapchain_ctx.swapchain];
        let image_indices = [image_index];
        // D-06: SUBOPTIMAL or OUT_OF_DATE from queue_present → recreate after present completes.
        let present_result = renderer
            .swapchain_ctx
            .swapchain_loader
            .queue_present(
                renderer.device_ctx.present_queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&signal_semaphores)
                    .swapchains(&swapchains)
                    .image_indices(&image_indices),
            );
        

        match present_result {
            Ok(false) => false, // success, not suboptimal
            Ok(true) => {
                // SUBOPTIMAL — present succeeded but swapchain should be recreated.
                log::info!("queue_present returned SUBOPTIMAL — requesting swapchain recreation");
                true
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                log::info!("queue_present returned ERROR_OUT_OF_DATE_KHR — requesting swapchain recreation");
                true
            }
            Err(e) => {
                return Err(anyhow::anyhow!("failed to present Vulkan swapchain image: {e}"));
            }
        }
    };

    // Advance staging ring to next frame's region for the next submit.
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
