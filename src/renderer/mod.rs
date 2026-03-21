use std::{
    collections::VecDeque,
    mem::ManuallyDrop,
    ptr::addr_of_mut,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result, anyhow};
use ash::{ext, khr, vk};
use gpu_allocator::{
    MemoryLocation,
    vulkan::{
        Allocation, AllocationCreateDesc, AllocationScheme, Allocator, AllocatorCreateDesc,
    },
};

use crate::{
    meshing::PackedMesh,
    streaming::types::ChunkKey,
};

pub mod chunk_pool;
pub mod device;
pub mod egui_backend;
pub mod frame;
pub mod instance;
pub mod swapchain;

#[derive(Debug, Clone, PartialEq)]
pub enum RenderDelta {
    Upsert { key: ChunkKey, mesh: PackedMesh },
    Remove { key: ChunkKey },
}

pub struct Renderer {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub debug_utils_loader: Option<ext::debug_utils::Instance>,
    pub debug_messenger: Option<vk::DebugUtilsMessengerEXT>,
    pub surface_loader: khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub device_ctx: device::DeviceContext,
    pub swapchain_ctx: swapchain::SwapchainContext,
    pub command_pool: vk::CommandPool,
    pub allocator: ManuallyDrop<Allocator>,
    pub frames: [frame::FrameData; 2],
    pub current_frame: usize,
    pub chunk_pool: Option<chunk_pool::ChunkPool>,
    pub pending_chunk_deltas: VecDeque<RenderDelta>,
    pub egui_backend: Option<egui_backend::EguiAshBackend>,
}

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

    pub fn write(&mut self, data: &[u8]) {
        assert!(
            data.len() as u64 <= self.size,
            "staging write exceeds allocation size"
        );

        if let Some(mapped) = self.allocation.mapped_slice_mut() {
            mapped[..data.len()].copy_from_slice(data);
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

impl Renderer {
    pub fn new(
        display_handle: raw_window_handle::RawDisplayHandle,
        window_handle: raw_window_handle::RawWindowHandle,
        window_extent: vk::Extent2D,
    ) -> Result<Self> {
        let entry = unsafe { ash::Entry::load().context("failed to load Vulkan entry")? };
        let instance = instance::create_instance(&entry, display_handle)?;

        #[cfg(debug_assertions)]
        let debug_utils_loader = Some(ext::debug_utils::Instance::new(&entry, &instance));
        #[cfg(not(debug_assertions))]
        let debug_utils_loader = None;

        #[cfg(debug_assertions)]
        let debug_messenger = Some(instance::setup_debug_messenger(&entry, &instance)?);
        #[cfg(not(debug_assertions))]
        let debug_messenger = None;

        let surface = unsafe {
            ash_window::create_surface(&entry, &instance, display_handle, window_handle, None)
                .context("failed to create Vulkan surface")?
        };
        let surface_loader = khr::surface::Instance::new(&entry, &instance);
        let device_ctx = device::pick_physical_device(&instance, &surface_loader, surface)?;

        let command_pool = unsafe {
            device_ctx
                .device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default()
                        .queue_family_index(device_ctx.graphics_family)
                        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                    None,
                )
                .context("failed to create Vulkan command pool")?
        };
        let allocator = Allocator::new(&AllocatorCreateDesc {
            instance: instance.clone(),
            device: device_ctx.device.clone(),
            physical_device: device_ctx.physical_device,
            debug_settings: Default::default(),
            buffer_device_address: false,
            allocation_sizes: Default::default(),
        })
        .map_err(|error| anyhow!("failed to create Vulkan allocator: {error}"))?;
        let swapchain_ctx = swapchain::create_swapchain_context(
            &instance,
            &device_ctx,
            &surface_loader,
            surface,
            window_extent,
        )?;
        let frames = [
            frame::create_frame_data(&device_ctx.device, command_pool)?,
            frame::create_frame_data(&device_ctx.device, command_pool)?,
        ];

        Ok(Self {
            entry,
            instance,
            debug_utils_loader,
            debug_messenger,
            surface_loader,
            surface,
            device_ctx,
            swapchain_ctx,
            command_pool,
            allocator: ManuallyDrop::new(allocator),
            frames,
            current_frame: 0,
            chunk_pool: None,
            pending_chunk_deltas: VecDeque::new(),
            egui_backend: None,
        })
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device_ctx.device.device_wait_idle();

            if let Some(egui_backend) = self.egui_backend.take() {
                let _ = egui_backend.destroy(self);
            }

            if let Some(chunk_pool) = self.chunk_pool.take() {
                let _ = chunk_pool.destroy(self);
            }

            for frame in self.frames.iter().rev() {
                self.device_ctx.device.destroy_fence(frame.in_flight, None);
                self.device_ctx
                    .device
                    .destroy_semaphore(frame.render_finished, None);
                self.device_ctx
                    .device
                    .destroy_semaphore(frame.image_available, None);
            }

            for framebuffer in self.swapchain_ctx.framebuffers.drain(..).rev() {
                self.device_ctx
                    .device
                    .destroy_framebuffer(framebuffer, None);
            }

            if self.swapchain_ctx.render_pass != vk::RenderPass::null() {
                self.device_ctx
                    .device
                    .destroy_render_pass(self.swapchain_ctx.render_pass, None);
                self.swapchain_ctx.render_pass = vk::RenderPass::null();
            }

            for image_view in self.swapchain_ctx.image_views.drain(..).rev() {
                self.device_ctx.device.destroy_image_view(image_view, None);
            }

            if self.swapchain_ctx.swapchain != vk::SwapchainKHR::null() {
                self.swapchain_ctx
                    .swapchain_loader
                    .destroy_swapchain(self.swapchain_ctx.swapchain, None);
                self.swapchain_ctx.swapchain = vk::SwapchainKHR::null();
            }

            if self.command_pool != vk::CommandPool::null() {
                self.device_ctx
                    .device
                    .destroy_command_pool(self.command_pool, None);
                self.command_pool = vk::CommandPool::null();
            }

            ManuallyDrop::drop(&mut self.allocator);
            self.device_ctx.device.destroy_device(None);

            if self.surface != vk::SurfaceKHR::null() {
                self.surface_loader.destroy_surface(self.surface, None);
                self.surface = vk::SurfaceKHR::null();
            }

            if let Some(debug_messenger) = self.debug_messenger.take() {
                if let Some(debug_utils_loader) = &self.debug_utils_loader {
                    debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
                }
            }

            self.instance.destroy_instance(None);
        }
    }
}

static RENDERER: OnceLock<Mutex<Renderer>> = OnceLock::new();

pub fn install_renderer(renderer: Renderer) -> Result<()> {
    RENDERER
        .set(Mutex::new(renderer))
        .map_err(|_| anyhow!("renderer already initialized"))
}

pub fn renderer_state() -> Option<&'static Mutex<Renderer>> {
    RENDERER.get()
}

impl Renderer {
    pub fn enqueue_chunk_delta(&mut self, delta: RenderDelta) {
        self.pending_chunk_deltas.push_back(delta);
    }

    fn record_chunk_delta_uploads(&mut self, _cmd: vk::CommandBuffer) -> Result<()> {
        let Some(chunk_pool) = self.chunk_pool.as_mut() else {
            self.pending_chunk_deltas.clear();
            return Ok(());
        };

        while let Some(delta) = self.pending_chunk_deltas.pop_front() {
            match delta {
                RenderDelta::Upsert { key, mesh } => {
                    let _ = chunk_pool.prepare_upload(key, &mesh)?;
                }
                RenderDelta::Remove { key } => {
                    let _ = chunk_pool.prepare_remove(key);
                }
            }
        }

        Ok(())
    }
}

pub fn submit_frame(renderer: &mut Renderer, _frame_index: u64) -> Result<()> {
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

        let clear_values = [vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.1, 0.1, 0.15, 1.0],
            },
        }];
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

        if let Some(mut egui_backend) = renderer.egui_backend.take() {
            let paint_result = egui_backend.paint(
                renderer,
                command_buffer,
                egui::TexturesDelta::default(),
                Vec::new(),
                [
                    renderer.swapchain_ctx.extent.width as f32,
                    renderer.swapchain_ctx.extent.height as f32,
                ],
            );
            renderer.egui_backend = Some(egui_backend);
            paint_result?;
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
    allocator_mut(renderer)
        .free(allocation)
        .map_err(|error| anyhow!("failed to free Vulkan buffer allocation: {error}"))?;
    unsafe {
        renderer.device_ctx.device.destroy_buffer(buffer, None);
    }
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
    allocator_mut(renderer)
        .free(allocation)
        .map_err(|error| anyhow!("failed to free Vulkan image allocation: {error}"))?;
    unsafe {
        renderer.device_ctx.device.destroy_image(image, None);
    }
    Ok(())
}

pub(crate) fn allocator_mut(renderer: &mut Renderer) -> &mut Allocator {
    unsafe { &mut *addr_of_mut!(renderer.allocator).cast::<Allocator>() }
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
            .queue_submit(renderer.device_ctx.graphics_queue, &submit_infos, vk::Fence::null())
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
            (
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            ) => (
                vk::AccessFlags::SHADER_READ,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::TRANSFER,
            ),
            (
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            ) => (
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
            ),
            _ => (
                vk::AccessFlags::empty(),
                vk::AccessFlags::empty(),
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            ),
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
