use anyhow::{Result, anyhow};
use ash::vk;
use gpu_allocator::{MemoryLocation, vulkan::{Allocation, AllocationScheme}};

use super::Renderer;
use super::helpers::{
    create_allocated_buffer, destroy_allocated_buffer, submit_one_shot_commands,
    transition_image_layout,
};

pub struct StagingBuffer {
    pub buffer: vk::Buffer,
    pub allocation: Allocation,
    pub size: vk::DeviceSize,
}

impl StagingBuffer {
    pub fn new(renderer: &mut Renderer, size: vk::DeviceSize) -> Result<Self> {
        let (buffer, allocation) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "staging",
        )?;

        Ok(Self {
            buffer,
            allocation,
            size,
        })
    }

    /// Write data into the staging buffer (MED-04).
    ///
    /// Returns `Err` if the allocation is not mapped (e.g. on non-UMA GPUs with
    /// `GpuOnly` memory — should never happen for staging buffers, but fail-safe).
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        assert!(
            data.len() as u64 <= self.size,
            "staging write exceeds allocation size"
        );

        if let Some(mapped) = self.allocation.mapped_slice_mut() {
            mapped[..data.len()].copy_from_slice(data);
            Ok(())
        } else {
            Err(anyhow!("staging buffer memory is not mapped — cannot write {} bytes", data.len()))
        }
    }

    pub fn copy_to(&self, renderer: &Renderer, dst: vk::Buffer, size: vk::DeviceSize) -> Result<()> {
        submit_one_shot_commands(renderer, |device, command_buffer| {
            let regions = [vk::BufferCopy::default().size(size)];
            unsafe {
                device.cmd_copy_buffer(command_buffer, self.buffer, dst, &regions);
            }
            Ok(())
        })
    }

    pub(crate) fn copy_to_image(
        &self,
        renderer: &Renderer,
        image: vk::Image,
        extent: vk::Extent3D,
        offset: [u32; 2],
        old_layout: vk::ImageLayout,
    ) -> Result<()> {
        submit_one_shot_commands(renderer, |device, command_buffer| {
            transition_image_layout(
                device,
                command_buffer,
                image,
                old_layout,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );

            let subresource = vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(0)
                .base_array_layer(0)
                .layer_count(1);
            let regions = [vk::BufferImageCopy::default()
                .buffer_offset(0)
                .image_subresource(subresource)
                .image_offset(vk::Offset3D {
                    x: offset[0] as i32,
                    y: offset[1] as i32,
                    z: 0,
                })
                .image_extent(extent)];

            unsafe {
                device.cmd_copy_buffer_to_image(
                    command_buffer,
                    self.buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &regions,
                );
            }

            transition_image_layout(
                device,
                command_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
            Ok(())
        })
    }

    pub fn destroy(self, renderer: &mut Renderer) -> Result<()> {
        destroy_allocated_buffer(renderer, self.buffer, self.allocation)
    }
}
