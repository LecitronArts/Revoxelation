use anyhow::{Context, Result};
use ash::{Device, vk};

pub struct FrameData {
    pub command_buffer: vk::CommandBuffer,
    pub image_available: vk::Semaphore,
    pub render_finished: vk::Semaphore,
    pub in_flight: vk::Fence,
}

pub fn create_frame_data(device: &Device, command_pool: vk::CommandPool) -> Result<FrameData> {
    let command_buffer = unsafe {
        device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .context("failed to allocate primary command buffer")?
            .into_iter()
            .next()
            .context("command buffer allocation returned no buffers")?
    };

    let image_available = unsafe {
        device
            .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            .context("failed to create image-available semaphore")?
    };
    let render_finished = unsafe {
        device
            .create_semaphore(&vk::SemaphoreCreateInfo::default(), None)
            .context("failed to create render-finished semaphore")?
    };
    let in_flight = unsafe {
        device
            .create_fence(
                &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                None,
            )
            .context("failed to create in-flight fence")?
    };

    Ok(FrameData {
        command_buffer,
        image_available,
        render_finished,
        in_flight,
    })
}
