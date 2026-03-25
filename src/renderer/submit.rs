use anyhow::{Context, Result};
use ash::vk;

use super::Renderer;
use super::camera::CameraUniforms;

pub fn submit_frame_sequence() -> &'static [&'static str] {
    &[
        "chunk_delta_uploads",
        "compute_cull",
        "indirect_barrier",
        "render_pass",
        "bind_chunk_pipeline",
        "draw_indexed_indirect",
        "egui",
    ]
}

pub fn submit_frame(renderer: &mut Renderer, _frame_index: u64, camera_uniforms: &CameraUniforms) -> Result<()> {
    let current_frame = renderer.current_frame;
    let command_buffer = renderer.frames[current_frame].command_buffer;
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
            .wait_for_fences(&[in_flight], true, u64::MAX)
            .context("failed waiting for Vulkan in-flight fence")?;

        let (image_index, _) = renderer
            .swapchain_ctx
            .swapchain_loader
            .acquire_next_image(
                renderer.swapchain_ctx.swapchain,
                u64::MAX,
                image_available,
                vk::Fence::null(),
            )
            .context("failed to acquire Vulkan swapchain image")?;

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

        renderer.record_chunk_delta_uploads(command_buffer)?;

        if let (Some(cull_pipeline), Some(chunk_pool)) =
            (&renderer.cull_pipeline, &renderer.chunk_pool)
        {
            cull_pipeline.dispatch(renderer, command_buffer, chunk_pool.active_draw_count());

            let barriers = [vk::BufferMemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::SHADER_WRITE)
                .dst_access_mask(vk::AccessFlags::INDIRECT_COMMAND_READ)
                .buffer(chunk_pool.dense_indirect_buffer())
                .offset(0)
                .size(vk::WHOLE_SIZE)];
            renderer.device_ctx.device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::DRAW_INDIRECT,
                vk::DependencyFlags::empty(),
                &[],
                &barriers,
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
        renderer
            .swapchain_ctx
            .swapchain_loader
            .queue_present(
                renderer.device_ctx.present_queue,
                &vk::PresentInfoKHR::default()
                    .wait_semaphores(&signal_semaphores)
                    .swapchains(&swapchains)
                    .image_indices(&image_indices),
            )
            .context("failed to present Vulkan swapchain image")?;
    }

    renderer.current_frame = (renderer.current_frame + 1) % renderer.frames.len();
    Ok(())
}
