use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, anyhow};
use ash::{ext, khr, vk};

pub mod device;
pub mod frame;
pub mod instance;
pub mod swapchain;

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
    pub frames: [frame::FrameData; 2],
    pub current_frame: usize,
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
            frames,
            current_frame: 0,
        })
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device_ctx.device.device_wait_idle();

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

pub fn submit_frame(renderer: &mut Renderer, _frame_index: u64) -> Result<()> {
    let device = &renderer.device_ctx.device;
    let frame = &renderer.frames[renderer.current_frame];
    let wait_semaphores = [frame.image_available];
    let signal_semaphores = [frame.render_finished];
    let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
    let command_buffers = [frame.command_buffer];

    unsafe {
        device
            .wait_for_fences(&[frame.in_flight], true, u64::MAX)
            .context("failed waiting for Vulkan in-flight fence")?;

        let (image_index, _) = renderer
            .swapchain_ctx
            .swapchain_loader
            .acquire_next_image(
                renderer.swapchain_ctx.swapchain,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            )
            .context("failed to acquire Vulkan swapchain image")?;

        device
            .reset_fences(&[frame.in_flight])
            .context("failed to reset Vulkan in-flight fence")?;
        device
            .reset_command_buffer(frame.command_buffer, vk::CommandBufferResetFlags::empty())
            .context("failed to reset Vulkan command buffer")?;
        device
            .begin_command_buffer(frame.command_buffer, &vk::CommandBufferBeginInfo::default())
            .context("failed to begin Vulkan command buffer")?;

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

        device.cmd_begin_render_pass(
            frame.command_buffer,
            &render_pass_begin,
            vk::SubpassContents::INLINE,
        );
        device.cmd_end_render_pass(frame.command_buffer);
        device
            .end_command_buffer(frame.command_buffer)
            .context("failed to end Vulkan command buffer")?;

        let submit_infos = [vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&command_buffers)
            .signal_semaphores(&signal_semaphores)];
        device
            .queue_submit(
                renderer.device_ctx.graphics_queue,
                &submit_infos,
                frame.in_flight,
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
