use std::{collections::VecDeque, mem::ManuallyDrop};

use anyhow::{Context, Result, anyhow};
use ash::{ext, khr, vk};
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};

use crate::{meshing::PackedMesh, streaming::types::ChunkKey};

pub mod camera;
pub mod chunk_pool;
pub mod cull_pipeline;
pub mod device;
pub mod egui_backend;
pub mod frame;
pub mod helpers;
pub mod hiz;
pub mod instance;
pub mod mesh_pipeline;
pub mod perf_counters;
pub mod pipeline_cache;
pub mod spirv;
pub mod staging;
pub mod staging_ring;
pub mod submit;
pub mod swapchain;

// Re-exports — keep external import paths stable.
pub(crate) use helpers::{
    create_allocated_buffer, create_allocated_image, destroy_allocated_buffer,
    destroy_allocated_image,
};
pub use staging::StagingBuffer;
pub use submit::{FrameOutcome, submit_frame, submit_frame_sequence};

#[derive(Debug, Clone, PartialEq)]
pub enum RenderDelta {
    Upsert { key: ChunkKey, mesh: PackedMesh },
    Remove { key: ChunkKey },
}

pub fn shader_source_files() -> &'static [&'static str] {
    &[
        "shaders/chunk_mesh.vert",
        "shaders/chunk_mesh.frag",
        "shaders/chunk_cull.comp",
        "shaders/hiz_generate.comp",
        "shaders/egui.vert",
        "shaders/egui.frag",
    ]
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
    pub staging_ring: Option<staging_ring::StagingRing>,
    pub pending_chunk_deltas: VecDeque<RenderDelta>,
    pub mesh_pipeline: Option<mesh_pipeline::ChunkMeshPipeline>,
    pub cull_pipeline: Option<cull_pipeline::ChunkCullPipeline>,
    pub hiz_pyramid: Option<hiz::HiZPyramid>,
    pub pipeline_cache: Option<pipeline_cache::PipelineCache>,
    pub egui_backend: Option<egui_backend::EguiAshBackend>,
    pub pending_egui_output: Option<crate::app::PendingEguiOutput>,
}

impl Renderer {
    pub fn new(
        display_handle: raw_window_handle::RawDisplayHandle,
        window_handle: raw_window_handle::RawWindowHandle,
        window_extent: vk::Extent2D,
    ) -> Result<Self> {
        let entry = unsafe { ash::Entry::load().context("failed to load Vulkan entry")? };
        let bootstrap = instance::create_instance(&entry, display_handle)?;
        let instance = bootstrap.instance;

        #[cfg(debug_assertions)]
        let debug_utils_loader = if bootstrap.debug.debug_utils_enabled {
            Some(ext::debug_utils::Instance::new(&entry, &instance))
        } else {
            None
        };
        #[cfg(not(debug_assertions))]
        let debug_utils_loader = None;

        #[cfg(debug_assertions)]
        let debug_messenger =
            if bootstrap.debug.validation_layer_enabled && bootstrap.debug.debug_utils_enabled {
                Some(instance::setup_debug_messenger(&entry, &instance)?)
            } else {
                None
            };
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
        let mut allocator = Allocator::new(&AllocatorCreateDesc {
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
            &mut allocator,
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
            staging_ring: None,
            pending_chunk_deltas: VecDeque::new(),
            mesh_pipeline: None,
            cull_pipeline: None,
            hiz_pyramid: None,
            pipeline_cache: None,
            egui_backend: None,
            pending_egui_output: None,
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

            // Save and destroy pipeline cache before pipelines are torn down.
            if let Some(pipeline_cache) = self.pipeline_cache.take() {
                let _ = pipeline_cache.save(&self.device_ctx.device);
                pipeline_cache.destroy(&self.device_ctx.device);
            }

            if let Some(staging_ring) = self.staging_ring.take() {
                let _ = staging_ring.destroy(self);
            }

            if let Some(chunk_pool) = self.chunk_pool.take() {
                let _ = chunk_pool.destroy(self);
            }

            if let Some(mesh_pipeline) = self.mesh_pipeline.take() {
                mesh_pipeline.destroy(self);
            }

            if let Some(hiz_pyramid) = self.hiz_pyramid.take() {
                hiz_pyramid.destroy(self);
            }

            if let Some(cull_pipeline) = self.cull_pipeline.take() {
                cull_pipeline.destroy(self);
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

            // Destroy depth resources before render pass.
            self.device_ctx
                .device
                .destroy_image_view(self.swapchain_ctx.depth_image_view, None);
            if let Some(alloc) = self.swapchain_ctx.depth_allocation.take() {
                let _ = self.allocator.free(alloc);
            }
            self.device_ctx
                .device
                .destroy_image(self.swapchain_ctx.depth_image, None);

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

impl Renderer {
    pub fn enqueue_chunk_delta(&mut self, delta: RenderDelta) {
        self.pending_chunk_deltas.push_back(delta);
    }

    pub(crate) fn record_chunk_delta_uploads(&mut self, cmd: vk::CommandBuffer) -> Result<()> {
        let Some(chunk_pool) = self.chunk_pool.as_mut() else {
            self.pending_chunk_deltas.clear();
            return Ok(());
        };

        let Some(staging_ring) = self.staging_ring.as_mut() else {
            // No staging ring — clear deltas to avoid infinite accumulation.
            self.pending_chunk_deltas.clear();
            return Ok(());
        };

        let device = self.device_ctx.device.clone();
        chunk_pool.record_uploads(&device, cmd, staging_ring, &mut self.pending_chunk_deltas)?;

        Ok(())
    }
}
