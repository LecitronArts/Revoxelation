use std::{collections::VecDeque, mem::ManuallyDrop};

use anyhow::{Context, Result, anyhow};
use ash::{ext, khr, vk};
use gpu_allocator::vulkan::{Allocator, AllocatorCreateDesc};

#[allow(unused_imports)]
use crate::{meshing::{MeshletMesh, PackedMesh}, streaming::types::ChunkKey};

pub mod bindless;
pub mod camera;
pub mod chunk_pool;
pub mod cull_pipeline;
pub mod device;
pub mod egui_backend;
pub mod frame;
pub mod helpers;
pub mod hiz;
#[cfg(all(debug_assertions, feature = "hot-reload"))]
pub mod hot_reload;
pub mod instance;
pub mod lighting;
pub mod material;
pub mod mesh_pipeline;
pub mod perf_counters;
pub mod pipeline_cache;
pub mod pipeline_set;
pub mod point_light;
pub mod pool_manager;
pub mod shadow;
pub mod spirv;
pub mod staging;
pub mod staging_ring;
pub mod submit;
pub mod swapchain;
pub mod texture_array;
pub mod vulkan_core;

// Re-exports — keep external import paths stable.
pub(crate) use helpers::{
    create_allocated_buffer, create_allocated_image, destroy_allocated_buffer,
    destroy_allocated_image,
};
pub use staging::StagingBuffer;
pub use submit::{FrameOutcome, submit_frame, submit_frame_sequence};

#[derive(Debug, Clone, PartialEq)]
pub enum RenderDelta {
    Upsert { key: ChunkKey, mesh: MeshletMesh },
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
        "shaders/meshlet_cull.comp",
        "shaders/meshlet_draw.vert",
        "shaders/meshlet_draw.frag",
        "shaders/meshlet.task",
        "shaders/meshlet.mesh",
        "shaders/shadow_depth.vert",
    ]
}

// ---------------------------------------------------------------------------
// REFAC-01: Logical sub-struct definitions for Renderer decomposition.
//
// Sub-struct types are defined in their own modules for clean organization:
//   - vulkan_core.rs: VulkanCore — entry, instance, surface, debug
//   - pipeline_set.rs: PipelineSet — mesh, cull, meshlet, cache, hiz
//   - pool_manager.rs: PoolManager — chunk_pool, meshlet_pool, staging, bindless
//
// The Renderer still keeps flat fields for borrow-checker ergonomics,
// but these sub-struct types provide logical grouping documentation and
// can be used as borrow-friendly reference bundles.
// ---------------------------------------------------------------------------

// Re-export sub-struct types at renderer level for convenient access.
pub use vulkan_core::VulkanCore;
pub use pipeline_set::PipelineSet;
pub use pool_manager::PoolManager;

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
    pub meshlet_pool: Option<chunk_pool::MeshletPool>,
    pub bindless: Option<bindless::BindlessTable>,
    pub staging_ring: Option<staging_ring::StagingRing>,
    pub pending_chunk_deltas: VecDeque<RenderDelta>,
    pub mesh_pipeline: Option<mesh_pipeline::ChunkMeshPipeline>,
    pub cull_pipeline: Option<cull_pipeline::ChunkCullPipeline>,
    pub meshlet_cull_pipeline: Option<cull_pipeline::MeshletCullPipeline>,
    /// Runtime toggles for meshlet culling modes (egui-accessible).
    pub meshlet_cull_backface: bool,
    pub meshlet_cull_frustum: bool,
    pub meshlet_cull_hiz: bool,
    pub hiz_pyramid: Option<hiz::HiZPyramid>,
    pub pipeline_cache: Option<pipeline_cache::PipelineCache>,
    pub egui_backend: Option<egui_backend::EguiAshBackend>,
    pub pending_egui_output: Option<crate::app::PendingEguiOutput>,
    pub texture_array: Option<texture_array::TextureArray>,
    pub material_buffer: Option<vk::Buffer>,
    pub material_allocation: Option<gpu_allocator::vulkan::Allocation>,
    /// Meshlet draw pipeline — either MeshShaderPath or ComputeIndirectPath (MSHL-03/04).
    pub meshlet_pipeline: Option<Box<dyn mesh_pipeline::MeshletPipeline>>,
    /// Whether the active meshlet_pipeline is a MeshShaderPath (skips meshlet_cull.comp).
    pub use_mesh_shader_path: bool,
    /// Runtime toggle: use meshlet rendering (true, default) or legacy per-chunk path (false).
    pub use_meshlet_rendering: bool,
    /// SSE threshold in pixels for LOD selection (MSHL-05). Default 2.0.
    pub sse_threshold: f32,
    /// GPU readback counters for real performance data (POLISH-06).
    pub readback_counters: Option<perf_counters::GpuReadbackCounters>,
    /// Latest GPU-counted visible meshlet count from readback (POLISH-06).
    /// Updated each frame after fence wait (reads previous frame's data).
    pub last_gpu_visible_meshlets: u32,
    /// Directional lighting state — sun direction, color, UBO management (LGHT-01).
    pub lighting_state: Option<lighting::LightingState>,
    /// Point light manager for emissive blocks (LGHT-01).
    pub point_light_manager: Option<point_light::PointLightManager>,
    /// PBR metallic-roughness texture array at binding 19 (LGHT-01).
    pub mr_texture_array: Option<texture_array::TextureArray>,
    /// PBR normal map texture array at binding 20 (LGHT-01).
    pub normal_texture_array: Option<texture_array::TextureArray>,
    /// PBR emissive texture array at binding 21 (LGHT-01).
    pub emissive_texture_array: Option<texture_array::TextureArray>,
    /// Cascaded shadow map for directional light shadows (LGHT-02).
    pub shadow_map: Option<shadow::CascadedShadowMap>,
    /// Shadow configuration (egui-adjustable, LGHT-02).
    pub shadow_config: shadow::ShadowConfig,
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
            meshlet_pool: None,
            bindless: None,
            staging_ring: None,
            pending_chunk_deltas: VecDeque::new(),
            mesh_pipeline: None,
            cull_pipeline: None,
            meshlet_cull_pipeline: None,
            meshlet_cull_backface: true,
            meshlet_cull_frustum: true,
            meshlet_cull_hiz: true,
            hiz_pyramid: None,
            pipeline_cache: None,
            egui_backend: None,
            pending_egui_output: None,
            texture_array: None,
            material_buffer: None,
            material_allocation: None,
            meshlet_pipeline: None,
            use_mesh_shader_path: false,
            use_meshlet_rendering: true,
            sse_threshold: 2.0,
            readback_counters: None,
            last_gpu_visible_meshlets: 0,
            lighting_state: None,
            point_light_manager: None,
            mr_texture_array: None,
            normal_texture_array: None,
            emissive_texture_array: None,
            shadow_map: None,
            shadow_config: shadow::ShadowConfig::default(),
        })
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            if let Err(e) = self.device_ctx.device.device_wait_idle() {
                log::warn!("failed to wait for device idle during cleanup: {e}");
            }

            if let Some(egui_backend) = self.egui_backend.take()
                && let Err(e) = egui_backend.destroy(self) {
                    log::warn!("failed to cleanup egui backend: {e}");
                }

            // Clean up readback counters before other GPU resources.
            if let Some(readback) = self.readback_counters.take()
                && let Err(e) = readback.destroy(self) {
                    log::warn!("failed to cleanup readback counters: {e}");
                }

            // Clean up CSM shadow map before bindless table (LGHT-02).
            if let Some(sm) = self.shadow_map.take()
                && let Err(e) = sm.destroy(self) {
                    log::warn!("failed to cleanup shadow map: {e}");
                }

            // Clean up point light manager before bindless table.
            if let Some(plm) = self.point_light_manager.take()
                && let Err(e) = plm.destroy(self) {
                    log::warn!("failed to cleanup point light manager: {e}");
                }

            // Clean up lighting state before bindless table.
            if let Some(ls) = self.lighting_state.take()
                && let Err(e) = ls.destroy(self) {
                    log::warn!("failed to cleanup lighting state: {e}");
                }

            // Clean up PBR texture arrays before bindless table.
            if let Some(ta) = self.emissive_texture_array.take()
                && let Err(e) = ta.destroy(self) {
                    log::warn!("failed to cleanup emissive texture array: {e}");
                }
            if let Some(ta) = self.normal_texture_array.take()
                && let Err(e) = ta.destroy(self) {
                    log::warn!("failed to cleanup normal texture array: {e}");
                }
            if let Some(ta) = self.mr_texture_array.take()
                && let Err(e) = ta.destroy(self) {
                    log::warn!("failed to cleanup MR texture array: {e}");
                }

            // Clean up texture array before bindless table.
            if let Some(texture_array) = self.texture_array.take()
                && let Err(e) = texture_array.destroy(self) {
                    log::warn!("failed to cleanup texture array: {e}");
                }

            // Clean up material buffer before bindless table.
            if let Some(alloc) = self.material_allocation.take()
                && let Some(buf) = self.material_buffer.take()
                    && let Err(e) = destroy_allocated_buffer(self, buf, alloc) {
                        log::warn!("failed to free material buffer: {e}");
                    }

            // Save and destroy pipeline cache before pipelines are torn down.
            if let Some(pipeline_cache) = self.pipeline_cache.take() {
                if let Err(e) = pipeline_cache.save(&self.device_ctx.device) {
                    log::warn!("failed to save pipeline cache: {e}");
                }
                pipeline_cache.destroy(&self.device_ctx.device);
            }

            if let Some(staging_ring) = self.staging_ring.take()
                && let Err(e) = staging_ring.destroy(self) {
                    log::warn!("failed to cleanup staging ring: {e}");
                }

            if let Some(chunk_pool) = self.chunk_pool.take()
                && let Err(e) = chunk_pool.destroy(self) {
                    log::warn!("failed to cleanup chunk pool: {e}");
                }

            if let Some(meshlet_pool) = self.meshlet_pool.take()
                && let Err(e) = meshlet_pool.destroy(self) {
                    log::warn!("failed to cleanup meshlet pool: {e}");
                }

            if let Some(mesh_pipeline) = self.mesh_pipeline.take() {
                mesh_pipeline.destroy(self);
            }

            if let Some(mut meshlet_pipeline) = self.meshlet_pipeline.take() {
                meshlet_pipeline.destroy_resources(&self.device_ctx.device);
            }

            if let Some(hiz_pyramid) = self.hiz_pyramid.take() {
                hiz_pyramid.destroy(self);
            }

            if let Some(cull_pipeline) = self.cull_pipeline.take() {
                cull_pipeline.destroy(self);
            }

            if let Some(meshlet_cull_pipeline) = self.meshlet_cull_pipeline.take() {
                meshlet_cull_pipeline.destroy(self);
            }

            if let Some(bindless) = self.bindless.take() {
                bindless.destroy(&self.device_ctx.device);
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
            if let Some(alloc) = self.swapchain_ctx.depth_allocation.take()
                && let Err(e) = self.allocator.free(alloc) {
                    log::warn!("failed to free depth allocation: {e}");
                }
            self.device_ctx
                .device
                .destroy_image(self.swapchain_ctx.depth_image, None);

            // Destroy MSAA color image.
            self.device_ctx
                .device
                .destroy_image_view(self.swapchain_ctx.msaa_color_view, None);
            if let Some(alloc) = self.swapchain_ctx.msaa_color_allocation.take()
                && let Err(e) = self.allocator.free(alloc) {
                    log::warn!("failed to free MSAA color allocation: {e}");
                }
            self.device_ctx
                .device
                .destroy_image(self.swapchain_ctx.msaa_color_image, None);

            // Destroy MSAA depth image.
            self.device_ctx
                .device
                .destroy_image_view(self.swapchain_ctx.msaa_depth_view, None);
            if let Some(alloc) = self.swapchain_ctx.msaa_depth_allocation.take()
                && let Err(e) = self.allocator.free(alloc) {
                    log::warn!("failed to free MSAA depth allocation: {e}");
                }
            self.device_ctx
                .device
                .destroy_image(self.swapchain_ctx.msaa_depth_image, None);

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

            if let Some(debug_messenger) = self.debug_messenger.take()
                && let Some(debug_utils_loader) = &self.debug_utils_loader {
                    debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
                }

            self.instance.destroy_instance(None);
        }
    }
}

impl Renderer {
    pub fn enqueue_chunk_delta(&mut self, delta: RenderDelta) {
        self.pending_chunk_deltas.push_back(delta);
    }

    /// Return a logical VulkanCore view (REFAC-01).
    #[allow(dead_code)]
    pub fn vulkan_core(&self) -> VulkanCore<'_> {
        VulkanCore {
            entry: &self.entry,
            instance: &self.instance,
            surface_loader: &self.surface_loader,
            surface: self.surface,
        }
    }

    /// Return a logical PipelineSet view (REFAC-01).
    #[allow(dead_code)]
    pub fn pipeline_set(&self) -> PipelineSet<'_> {
        PipelineSet {
            mesh_pipeline: self.mesh_pipeline.as_ref(),
            cull_pipeline: self.cull_pipeline.as_ref(),
            meshlet_cull_pipeline: self.meshlet_cull_pipeline.as_ref(),
            meshlet_pipeline: self.meshlet_pipeline.as_deref(),
            pipeline_cache: self.pipeline_cache.as_ref(),
            hiz_pyramid: self.hiz_pyramid.as_ref(),
        }
    }

    /// Return a logical PoolManager view (REFAC-01).
    #[allow(dead_code)]
    pub fn pool_manager(&self) -> PoolManager<'_> {
        PoolManager {
            chunk_pool: self.chunk_pool.as_ref(),
            meshlet_pool: self.meshlet_pool.as_ref(),
            staging_ring: self.staging_ring.as_ref(),
            bindless: self.bindless.as_ref(),
            texture_array: self.texture_array.as_ref(),
        }
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
