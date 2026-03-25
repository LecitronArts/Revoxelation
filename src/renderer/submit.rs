use anyhow::{Context, Result};
use ash::vk;
use glam::Mat4;

use super::Renderer;
use super::camera::{CameraUniforms, extract_frustum_planes};

pub fn submit_frame_sequence() -> &'static [&'static str] {
    &[
        "staging_ring_reset",
        "chunk_delta_uploads",
        "transfer_to_compute_barrier",
        "compute_cull",
        "indirect_barrier",
        "render_pass",
        "bind_chunk_pipeline",
        "draw_indexed_indirect",
        "egui",
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

        if let (Some(cull_pipeline), Some(chunk_pool)) =
            (&renderer.cull_pipeline, &renderer.chunk_pool)
        {
            let view_proj = Mat4::from_cols_array_2d(&camera_uniforms.view_proj);
            let frustum_planes = extract_frustum_planes(&view_proj);
            let active_draw_count = chunk_pool.active_draw_count();

            cull_pipeline.dispatch(
                &renderer.device_ctx.device,
                command_buffer,
                active_draw_count,
                &frustum_planes,
            );

            // Barrier: compute shader writes → indirect draw reads for both dense indirect
            // and draw count buffers.
            let dense_barrier = vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::INDIRECT_COMMAND_READ)
                .buffer(chunk_pool.dense_indirect_buffer())
                .offset(0)
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

        if let (Some(mesh_pipeline), Some(chunk_pool)) =
            (&renderer.mesh_pipeline, &renderer.chunk_pool)
        {
            let draw_count = chunk_pool.active_draw_count();
            if draw_count > 0 {
                mesh_pipeline.draw(renderer, chunk_pool, command_buffer, draw_count, camera_uniforms);
            }
        }

        if let Some(mut egui_backend) = renderer.egui_backend.take() {
            let egui_output = renderer.pending_egui_output.take();
            if let Some(output) = egui_output {
                let paint_result = egui_backend.paint(
                    renderer,
                    command_buffer,
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
                    egui::TexturesDelta::default(),
                    Vec::new(),
                    [extent.width as f32, extent.height as f32],
                );
                renderer.egui_backend = Some(egui_backend);
                paint_result?;
            }
        }

        renderer.device_ctx.device.cmd_end_render_pass(command_buffer);
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
        let needs_recreate = match present_result {
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
        };

        needs_recreate
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
