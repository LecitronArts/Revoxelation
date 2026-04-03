use anyhow::{Context, Result, anyhow};
use ash::vk;
use gpu_allocator::{
    MemoryLocation,
    vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator},
};

use super::Renderer;

pub(crate) fn allocator_mut(renderer: &mut Renderer) -> &mut Allocator {
    &mut renderer.allocator
}

pub(crate) fn create_allocated_buffer(
    renderer: &mut Renderer,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    location: MemoryLocation,
    allocation_scheme: AllocationScheme,
    name: &'static str,
) -> Result<(vk::Buffer, Allocation)> {
    let buffer = unsafe {
        renderer
            .device_ctx
            .device
            .create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
            .context("failed to create Vulkan buffer")?
    };

    let requirements = unsafe {
        renderer
            .device_ctx
            .device
            .get_buffer_memory_requirements(buffer)
    };
    let allocation = allocator_mut(renderer)
        .allocate(&AllocationCreateDesc {
            name,
            requirements,
            location,
            linear: true,
            allocation_scheme,
        })
        .map_err(|error| anyhow!("failed to allocate Vulkan buffer memory: {error}"))?;

    unsafe {
        renderer
            .device_ctx
            .device
            .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
            .context("failed to bind Vulkan buffer memory")?;
    }

    Ok((buffer, allocation))
}

pub(crate) fn destroy_allocated_buffer(
    renderer: &mut Renderer,
    buffer: vk::Buffer,
    allocation: Allocation,
) -> Result<()> {
    // Correct Vulkan destruction order (HIGH-02): destroy resource BEFORE freeing memory.
    unsafe {
        renderer.device_ctx.device.destroy_buffer(buffer, None);
    }
    allocator_mut(renderer)
        .free(allocation)
        .map_err(|error| anyhow!("failed to free Vulkan buffer allocation: {error}"))?;
    Ok(())
}

pub(crate) fn create_allocated_image(
    renderer: &mut Renderer,
    extent: vk::Extent3D,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    allocation_scheme: AllocationScheme,
    name: &'static str,
) -> Result<(vk::Image, Allocation)> {
    let image = unsafe {
        renderer
            .device_ctx
            .device
            .create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(extent)
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )
            .context("failed to create Vulkan image")?
    };

    let requirements = unsafe {
        renderer
            .device_ctx
            .device
            .get_image_memory_requirements(image)
    };
    let allocation_scheme = match allocation_scheme {
        AllocationScheme::DedicatedImage(_) => AllocationScheme::DedicatedImage(image),
        other => other,
    };
    let allocation = allocator_mut(renderer)
        .allocate(&AllocationCreateDesc {
            name,
            requirements,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme,
        })
        .map_err(|error| anyhow!("failed to allocate Vulkan image memory: {error}"))?;

    unsafe {
        renderer
            .device_ctx
            .device
            .bind_image_memory(image, allocation.memory(), allocation.offset())
            .context("failed to bind Vulkan image memory")?;
    }

    Ok((image, allocation))
}

pub(crate) fn destroy_allocated_image(
    renderer: &mut Renderer,
    image: vk::Image,
    allocation: Allocation,
) -> Result<()> {
    // Correct Vulkan destruction order (HIGH-02): destroy resource BEFORE freeing memory.
    unsafe {
        renderer.device_ctx.device.destroy_image(image, None);
    }
    allocator_mut(renderer)
        .free(allocation)
        .map_err(|error| anyhow!("failed to free Vulkan image allocation: {error}"))?;
    Ok(())
}

pub(crate) fn submit_one_shot_commands<F>(renderer: &Renderer, record: F) -> Result<()>
where
    F: FnOnce(&ash::Device, vk::CommandBuffer) -> Result<()>,
{
    let device = &renderer.device_ctx.device;
    let command_buffer = unsafe {
        device
            .allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(renderer.command_pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
            .context("failed to allocate one-shot command buffer")?
            .into_iter()
            .next()
            .context("one-shot command allocation returned no buffers")?
    };

    unsafe {
        device
            .begin_command_buffer(
                command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
            .context("failed to begin one-shot command buffer")?;
    }

    record(device, command_buffer)?;

    unsafe {
        device
            .end_command_buffer(command_buffer)
            .context("failed to end one-shot command buffer")?;

        let command_buffers = [command_buffer];
        let submit_infos = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
        device
            .queue_submit(
                renderer.device_ctx.graphics_queue,
                &submit_infos,
                vk::Fence::null(),
            )
            .context("failed to submit one-shot command buffer")?;
        device
            .queue_wait_idle(renderer.device_ctx.graphics_queue)
            .context("failed waiting for one-shot command buffer")?;
        device.free_command_buffers(renderer.command_pool, &command_buffers);
    }

    Ok(())
}

pub(crate) fn transition_image_layout(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
    let (src_access_mask, dst_access_mask, src_stage_mask, dst_stage_mask) =
        match (old_layout, new_layout) {
            (vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL) => (
                vk::AccessFlags::empty(),
                vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
            ),
            (vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL, vk::ImageLayout::TRANSFER_DST_OPTIMAL) => (
                vk::AccessFlags::SHADER_READ,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::TRANSFER,
            ),
            (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL) => (
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
            ),
            _ => {
                // MED-03: Catch-all uses conservative access masks and emits a warning.
                // This ensures no silent zero-synchronization for unhandled transitions.
                log::warn!(
                    "transition_image_layout: unhandled layout transition from {:?} to {:?}",
                    old_layout,
                    new_layout,
                );
                (
                    vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
                    vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                    vk::PipelineStageFlags::ALL_COMMANDS,
                )
            }
        };

    let barriers = [vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_access_mask(src_access_mask)
        .dst_access_mask(dst_access_mask)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1),
        )];

    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            src_stage_mask,
            dst_stage_mask,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &barriers,
        );
    }
}
