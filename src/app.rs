use anyhow::{Context, Result, anyhow};
use ash::vk;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::time::{Duration, Instant};
use winit::{
    event::{DeviceEvent, ElementState, Event, Ime, KeyEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, WindowBuilder},
};

use crate::config::RuntimeConfig;
use crate::meshing::MeshingState;
use crate::renderer::{
    FrameOutcome, Renderer,
    bindless::{
        BINDING_DRAW_COUNT, BINDING_FRUSTUM_PLANES, BINDING_HIZ_CONFIG, BINDING_HIZ_PYRAMID,
        BINDING_SCENE, BindlessTable,
    },
    camera::{CameraKey, FpsCamera},
    chunk_pool::{ChunkPool, MeshletPool},
    cull_pipeline::ChunkCullPipeline,
    egui_backend::EguiAshBackend,
    mesh_pipeline::{ChunkMeshPipeline, create_meshlet_pipeline},
    perf_counters::GpuPerfCounters,
    pipeline_cache::PipelineCache,
    staging_ring::StagingRing,
    swapchain::recreate_swapchain_context,
};
use crate::runtime::scheduler::StreamingState;

/// Pending egui output to be consumed by submit_frame.
pub struct PendingEguiOutput {
    pub textures_delta: egui::TexturesDelta,
    pub clipped_primitives: Vec<egui::ClippedPrimitive>,
    /// Screen size in egui **point** coordinates (logical pixels).
    /// Tessellated vertex positions and clip rects are also in points.
    pub screen_size_points: [f32; 2],
    /// Physical pixels per egui point — needed to convert clip rects to
    /// Vulkan scissor rects (which operate in physical pixel coordinates).
    pub pixels_per_point: f32,
}

/// Application root that owns all subsystems directly (no global state).
pub struct App {
    pub renderer: Renderer,
    pub streaming: StreamingState,
    pub meshing: MeshingState,
    pub egui_ctx: egui::Context,
    pub camera: FpsCamera,
    pub frame_index: u64,
    /// Tracked key states for continuous movement.
    pub keys_pressed: KeysPressed,
    pub last_frame_time: Instant,
    /// Engine start time for calculating spawn_time offsets (POLISH-08).
    pub engine_start_time: Instant,
    /// D-08: Flag indicating swapchain recreation is needed before next acquire.
    pub needs_resize: bool,
    /// Current window extent (updated on Resized events).
    #[allow(dead_code)]
    pub window_extent: vk::Extent2D,
    /// GPU performance counters for the HUD overlay.
    pub perf_counters: GpuPerfCounters,
    /// Runtime configuration loaded from config.toml.
    pub config: RuntimeConfig,
    /// Shader hot-reload tracker (debug + hot-reload feature only).
    #[cfg(all(debug_assertions, feature = "hot-reload"))]
    pub hot_reload: crate::renderer::hot_reload::ShaderHotReload,
    /// Accumulated egui input events between frames.
    pub egui_events: Vec<egui::Event>,
    /// Last known cursor position (for PointerButton events).
    pub cursor_pos: Option<egui::Pos2>,
    /// Whether egui wants pointer input (cached from previous frame).
    pub egui_wants_pointer: bool,
    /// Whether egui wants keyboard input (cached from previous frame).
    pub egui_wants_keyboard: bool,
    /// Current modifier state mirrored from winit.
    pub egui_modifiers: egui::Modifiers,
    /// Last known window focus state.
    pub window_focused: bool,
    /// Next throttled redraw time while idle.
    pub next_idle_redraw: Instant,
    /// Whether the mouse is captured for FPS camera look (B2 fix).
    pub mouse_captured: bool,
    /// Whether to show the HUD overlay (F1 toggle, H9 fix).
    pub show_hud: bool,
}

/// Tracks which movement keys are currently held down.
#[derive(Default)]
pub struct KeysPressed {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
}

impl KeysPressed {
    fn any_pressed(&self) -> bool {
        self.forward || self.backward || self.left || self.right || self.up || self.down
    }
}

impl App {
    fn sync_ssao_pass(&mut self, width: u32, height: u32) -> Result<()> {
        let divisor = if self.renderer.config.ssao.half_resolution {
            2
        } else {
            1
        };
        let desired_width = width.max(1).div_ceil(divisor);
        let desired_height = height.max(1).div_ceil(divisor);
        let needs_recreate = match self.renderer.ssao_pass.as_ref() {
            Some(ssao) => ssao.width != desired_width || ssao.height != desired_height,
            None => true,
        };
        if !needs_recreate {
            return Ok(());
        }

        let ssao = match self.renderer.ssao_pass.take() {
            Some(ssao) => ssao.recreate(&mut self.renderer, width, height)?,
            None => {
                let ssao_config = self.renderer.config.ssao.clone();
                crate::renderer::ssao::SsaoPass::new(
                    &mut self.renderer,
                    width,
                    height,
                    &ssao_config,
                )?
            }
        };
        ssao.register_bindless(&self.renderer);
        self.renderer.ssao_pass = Some(ssao);
        Ok(())
    }

    fn should_redraw_immediately(&self) -> bool {
        self.needs_resize
            || self.keys_pressed.any_pressed()
            || !self.egui_events.is_empty()
            || self.streaming.has_in_flight_jobs()
            || !self.streaming.pending_render_deltas.is_empty()
            || self.streaming.job_queue.len() > 0
            || !self.meshing.dirty.is_empty()
            || !self.meshing.queued.is_empty()
            || !self.meshing.completed_meshes.is_empty()
            || self
                .renderer
                .lighting_state
                .as_ref()
                .is_some_and(|ls| ls.use_day_night_cycle && !ls.day_night.paused)
    }
    /// Main per-frame tick — extracted from the RedrawRequested handler body (REFAC-08).
    ///
    /// Returns `Ok(())` on success, or an error if a critical failure occurs.
    /// Swapchain recreation is handled internally (sets `needs_resize` flag).
    pub fn tick(&mut self, window: &winit::window::Window) {
        // D-07: When window extent is 0×0 (minimized), skip rendering entirely.
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        // D-08: If needs_resize, recreate swapchain before rendering.
        if self.needs_resize {
            let new_extent = vk::Extent2D {
                width: size.width,
                height: size.height,
            };
            if let Err(e) = recreate_swapchain_context(&mut self.renderer, new_extent) {
                log::error!("Failed to recreate swapchain on resize: {e:#}");
                return;
            }
            self.needs_resize = false;
        }

        // Delta time for smooth movement.
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame_time).as_secs_f32().min(0.1);
        self.last_frame_time = now;

        // Apply continuous keyboard movement.
        if self.keys_pressed.forward {
            self.camera.process_keyboard(CameraKey::Forward, true, dt);
        }
        if self.keys_pressed.backward {
            self.camera.process_keyboard(CameraKey::Backward, true, dt);
        }
        if self.keys_pressed.left {
            self.camera.process_keyboard(CameraKey::Left, true, dt);
        }
        if self.keys_pressed.right {
            self.camera.process_keyboard(CameraKey::Right, true, dt);
        }
        if self.keys_pressed.up {
            self.camera.process_keyboard(CameraKey::Up, true, dt);
        }
        if self.keys_pressed.down {
            self.camera.process_keyboard(CameraKey::Down, true, dt);
        }

        let screen_size = [size.width as f32, size.height as f32];
        let native_pixels_per_point = window.scale_factor() as f32;
        let pixels_per_point = (self.egui_ctx.zoom_factor() * native_pixels_per_point).max(1.0);
        let screen_size_points = egui::vec2(
            screen_size[0] / pixels_per_point,
            screen_size[1] / pixels_per_point,
        );

        // Tick day-night cycle (LGHT-05).
        if let Some(ls) = &mut self.renderer.lighting_state {
            ls.tick_day_night(dt);
        }

        // Build egui frame.
        let mut raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                screen_size_points,
            )),
            time: Some(self.engine_start_time.elapsed().as_secs_f64()),
            modifiers: self.egui_modifiers,
            events: std::mem::take(&mut self.egui_events),
            focused: self.window_focused,
            ..Default::default()
        };
        if let Some(viewport) = raw_input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.native_pixels_per_point = Some(native_pixels_per_point);
        }

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            // H9 fix: All debug windows gated behind show_hud (F1 toggles).
            if !self.show_hud {
                return;
            }

            use crate::ui::debug_panels;

            debug_panels::draw_debug_window(
                ctx,
                &self.perf_counters,
                self.frame_index,
                self.camera.position,
            );

            debug_panels::draw_meshlet_culling_window(ctx, &mut self.renderer.config);

            // Lighting controls (LGHT-01).
            if let Some(ls) = &mut self.renderer.lighting_state {
                debug_panels::draw_lighting_window(ctx, ls);
            }

            // CSM Shadow controls (LGHT-02).
            {
                let shadow_resolution = self.renderer.shadow_map.as_ref().map(|sm| sm.resolution);
                debug_panels::draw_shadow_window(
                    ctx,
                    &mut self.renderer.config.shadow,
                    shadow_resolution,
                );
            }

            // SSAO controls (LGHT-03).
            {
                let ao_size = self
                    .renderer
                    .ssao_pass
                    .as_ref()
                    .map(|ssao| (ssao.width, ssao.height));
                debug_panels::draw_ssao_window(ctx, &mut self.renderer.config.ssao, ao_size);
            }

            // Sky / Atmosphere controls (LGHT-05).
            if let Some(sky) = &mut self.renderer.sky_renderer {
                debug_panels::draw_sky_window(ctx, &mut sky.config);
            }

            // Day-Night Cycle + Fog controls (LGHT-05).
            if let Some(ls) = &mut self.renderer.lighting_state {
                debug_panels::draw_day_night_window(ctx, ls);
                debug_panels::draw_fog_window(ctx, ls);
            }
        });

        let clipped_primitives = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);
        let textures_delta = full_output.textures_delta;

        // Cache egui input priority for the next frame's event routing.
        self.egui_wants_pointer = self.egui_ctx.wants_pointer_input();
        self.egui_wants_keyboard = self.egui_ctx.wants_keyboard_input();

        // Store egui output for submit_frame to consume.
        self.renderer.pending_egui_output = Some(PendingEguiOutput {
            textures_delta,
            clipped_primitives,
            screen_size_points: [screen_size_points.x, screen_size_points.y],
            pixels_per_point,
        });

        if let Err(e) = self.sync_ssao_pass(size.width, size.height) {
            log::error!("failed to sync SSAO pass: {e:#}");
        }

        // Run scheduler frame, then handle renderer submission.
        let camera_pos = self.camera.position.to_array();
        let _result = crate::runtime::run_frame(
            &mut self.streaming,
            &mut self.meshing,
            Some(&mut self.renderer),
            self.frame_index,
            camera_pos,
            screen_size[1],
            self.camera.fov_y,
        );

        // Drain pending render deltas and submit frame from app-owned renderer.
        crate::runtime::scheduler::drain_pending_render_deltas_into_renderer(
            &mut self.streaming,
            &mut self.renderer,
        );
        if let Some(point_lights) = self.renderer.point_light_manager.as_mut() {
            point_lights.rebuild_from_payloads(&self.meshing.payloads, camera_pos);
        }
        let aspect = if screen_size[1] > 0.0 {
            screen_size[0] / screen_size[1]
        } else {
            1.0
        };
        let camera_uniforms = self.camera.view_proj(aspect);
        let current_time = self.engine_start_time.elapsed().as_secs_f32();
        match crate::renderer::submit_frame(
            &mut self.renderer,
            self.frame_index,
            &camera_uniforms,
            current_time,
        ) {
            Ok(FrameOutcome::Submitted) => {}
            Ok(FrameOutcome::NeedsRecreate) => {
                // submit_frame signalled swapchain is stale — recreate next frame.
                self.needs_resize = true;
            }
            Err(e) => {
                log::error!("submit_frame failed: {e:#}");
            }
        }

        // Update performance counters for next frame's HUD.
        let frame_time_ms = dt * 1000.0;
        let total_chunks = self
            .renderer
            .chunk_pool
            .as_ref()
            .map(|cp| cp.active_draw_count())
            .unwrap_or(0);
        let chunk_capacity = self
            .renderer
            .chunk_pool
            .as_ref()
            .map(|cp| cp.capacity() as u32)
            .unwrap_or(0);
        self.perf_counters.frame_time_ms = frame_time_ms;
        self.perf_counters.total_chunks = total_chunks;
        self.perf_counters.chunk_capacity = chunk_capacity;
        // visible_chunks approximated as total (actual readback deferred to future)
        self.perf_counters.visible_chunks = total_chunks;

        // Meshlet LOD statistics (MSHL-05).
        if let Some(meshlet_pool) = &self.renderer.meshlet_pool {
            self.perf_counters.total_meshlets = meshlet_pool.active_meshlet_count();
        }
        // POLISH-06: Use real GPU readback data for visible meshlet count.
        self.perf_counters.visible_meshlets = self.renderer.last_gpu_visible_meshlets;
        if self.perf_counters.total_meshlets > 0 {
            self.perf_counters.meshlet_cull_rate = 1.0
                - (self.perf_counters.visible_meshlets as f32
                    / self.perf_counters.total_meshlets as f32);
        } else {
            self.perf_counters.meshlet_cull_rate = 0.0;
        }
        self.perf_counters.sse_threshold = self.renderer.config.sse_threshold;

        // Shader hot-reload (debug builds with hot-reload feature only).
        #[cfg(all(debug_assertions, feature = "hot-reload"))]
        {
            if let Err(e) = self.hot_reload.check_and_reload(&mut self.renderer) {
                log::error!("Shader hot-reload error: {e:#}");
            }
        }

        self.frame_index = self.frame_index.saturating_add(1);
    }
}

pub fn run() -> Result<()> {
    let event_loop = EventLoop::new().context("failed to create winit event loop")?;
    let window = WindowBuilder::new()
        .with_title("Revoxelation")
        .build(&event_loop)
        .context("failed to create window")?;

    let size = window.inner_size();
    let display_handle = window
        .display_handle()
        .map_err(|err| anyhow!("failed to get display handle: {err}"))?
        .as_raw();
    let window_handle = window
        .window_handle()
        .map_err(|err| anyhow!("failed to get window handle: {err}"))?
        .as_raw();
    let extent = vk::Extent2D {
        width: size.width.max(1),
        height: size.height.max(1),
    };

    let mut renderer = Renderer::new(display_handle, window_handle, extent)?;
    renderer.chunk_pool = Some(ChunkPool::new(&mut renderer)?);
    renderer.meshlet_pool = Some(MeshletPool::new(&mut renderer)?);
    // 32 MB staging ring, 2 frames (16 MB per frame).
    renderer.staging_ring = Some(StagingRing::new(&mut renderer, 32 * 1024 * 1024, 2)?);
    // Load persistent pipeline cache from disk (or create empty).
    renderer.pipeline_cache = Some(PipelineCache::load(&renderer.device_ctx.device)?);

    // Create the unified bindless descriptor set 0 BEFORE pipelines (D-04).
    // Pass mesh_shader_supported to add TASK/MESH stage flags conditionally (HIGH-07).
    let bindless = BindlessTable::new(
        &renderer.device_ctx.device,
        renderer.device_ctx.mesh_shader_supported,
    )?;

    // Register the unified scene_buffer with the bindless table at binding 0 (D-07).
    // Bindings 1-3 are now free (all 4 regions merged into scene_buffer).
    {
        let chunk_pool = renderer
            .chunk_pool
            .as_ref()
            .expect("chunk pool must be initialized before bindless registration");
        bindless.register_buffer(
            &renderer.device_ctx.device,
            BINDING_SCENE,
            chunk_pool.scene_buffer(),
            vk::WHOLE_SIZE,
        );
        let meshlet_pool = renderer
            .meshlet_pool
            .as_ref()
            .expect("meshlet pool must be initialized before bindless registration");
        bindless.register_meshlet_buffers(&renderer.device_ctx.device, meshlet_pool);
    }

    let bindless_layout = bindless.descriptor_set_layout;
    renderer.bindless = Some(bindless);

    // Create pipelines using the shared bindless layout.
    renderer.mesh_pipeline = Some(ChunkMeshPipeline::new(&renderer, bindless_layout)?);
    renderer.meshlet_pipeline = Some(create_meshlet_pipeline(&renderer, bindless_layout)?);
    renderer.config.use_mesh_shader_path =
        renderer.device_ctx.mesh_shader_supported && renderer.device_ctx.mesh_shader_fn.is_some();
    renderer.cull_pipeline = Some(ChunkCullPipeline::new(&mut renderer, bindless_layout)?);

    // Create meshlet cull pipeline (MSHL-02, D-10).
    renderer.meshlet_cull_pipeline = Some(
        crate::renderer::cull_pipeline::MeshletCullPipeline::new(&renderer, bindless_layout)?,
    );

    // Register cull pipeline auxiliary buffers with the bindless table.
    {
        let cull_pipeline = renderer
            .cull_pipeline
            .as_ref()
            .expect("cull pipeline must be initialized");
        let bindless = renderer
            .bindless
            .as_ref()
            .expect("bindless must be initialized");
        bindless.register_buffer(
            &renderer.device_ctx.device,
            BINDING_FRUSTUM_PLANES,
            cull_pipeline.frustum_planes_buffer,
            std::mem::size_of::<crate::renderer::camera::FrustumPlanes>() as u64,
        );
        bindless.register_buffer(
            &renderer.device_ctx.device,
            BINDING_DRAW_COUNT,
            cull_pipeline.draw_count_buffer,
            std::mem::size_of::<u32>() as u64,
        );
        bindless.register_buffer(
            &renderer.device_ctx.device,
            BINDING_HIZ_CONFIG,
            cull_pipeline.hiz_config_buffer,
            std::mem::size_of::<crate::renderer::cull_pipeline::HiZConfig>() as u64,
        );
    }

    // Create the initial Hi-Z pyramid so culling and SSAO are wired from frame 0.
    {
        let hiz =
            crate::renderer::hiz::HiZPyramid::new(&mut renderer, extent.width, extent.height)?;
        let bindless = renderer
            .bindless
            .as_ref()
            .expect("bindless must be initialized");
        bindless.register_image(
            &renderer.device_ctx.device,
            BINDING_HIZ_PYRAMID,
            hiz.full_view,
            hiz.sampler,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        );
        renderer.hiz_pyramid = Some(hiz);
    }

    // Create and upload material table to bindless binding 8.
    {
        let material_table = crate::renderer::material::MaterialTable::default_table();
        let (buf, alloc) = material_table.upload(&mut renderer)?;
        renderer.material_buffer = Some(buf);
        renderer.material_allocation = Some(alloc);
    }

    // Create texture array and register at bindless binding 9.
    renderer.texture_array = Some(crate::renderer::texture_array::TextureArray::new(
        &mut renderer,
    )?);

    // Create PBR texture arrays (MR, normal, emissive) at bindings 19/20/21 (LGHT-01).
    renderer.mr_texture_array = Some(crate::renderer::texture_array::new_mr_array_16(
        &mut renderer,
    )?);
    renderer.normal_texture_array = Some(crate::renderer::texture_array::new_normal_array_16(
        &mut renderer,
    )?);
    renderer.emissive_texture_array = Some(crate::renderer::texture_array::new_emissive_array_16(
        &mut renderer,
    )?);

    // Create directional lighting state (binding 18 SSBO) (LGHT-01).
    renderer.lighting_state = Some(crate::renderer::lighting::LightingState::new(
        &mut renderer,
    )?);

    // Create point light manager (binding 22 SSBO) (LGHT-01).
    renderer.point_light_manager = Some(crate::renderer::point_light::PointLightManager::new(
        &mut renderer,
    )?);

    // Create CSM shadow map (4 cascades, default 2048 resolution) (LGHT-02).
    {
        let csm = crate::renderer::shadow::CascadedShadowMap::new(
            &mut renderer,
            crate::renderer::shadow::DEFAULT_SHADOW_RESOLUTION,
        )?;
        csm.register_shadow_maps(&renderer);
        renderer.shadow_map = Some(csm);
    }

    // Create SSAO pass (LGHT-03).
    {
        let ssao_config = renderer.config.ssao.clone();
        let ssao = crate::renderer::ssao::SsaoPass::new(
            &mut renderer,
            extent.width,
            extent.height,
            &ssao_config,
        )?;
        ssao.register_bindless(&renderer);
        renderer.ssao_pass = Some(ssao);
    }

    // Create sky renderer (LGHT-05).
    renderer.sky_renderer = Some(crate::renderer::sky::SkyRenderer::new(&mut renderer)?);

    renderer.egui_backend = Some(EguiAshBackend::new(&mut renderer)?);

    // POLISH-06: Create GPU readback counters for real performance data.
    renderer.readback_counters = Some(crate::renderer::perf_counters::GpuReadbackCounters::new(
        &mut renderer,
    )?);

    let config = RuntimeConfig::load();

    let mut app = App {
        renderer,
        streaming: StreamingState::new(),
        meshing: MeshingState::default(),
        egui_ctx: egui::Context::default(),
        camera: {
            let mut cam = FpsCamera::default();
            cam.move_speed = config.camera_speed;
            cam.fov_y = config.camera_fov.to_radians();
            cam.mouse_sensitivity = config.mouse_sensitivity;
            cam
        },
        frame_index: 0,
        keys_pressed: KeysPressed::default(),
        last_frame_time: Instant::now(),
        engine_start_time: Instant::now(),
        needs_resize: false,
        window_extent: extent,
        perf_counters: GpuPerfCounters::default(),
        config: config.clone(),
        #[cfg(all(debug_assertions, feature = "hot-reload"))]
        hot_reload: crate::renderer::hot_reload::ShaderHotReload::new(),
        egui_events: Vec::new(),
        cursor_pos: None,
        egui_wants_pointer: false,
        egui_wants_keyboard: false,
        egui_modifiers: egui::Modifiers::default(),
        window_focused: true,
        next_idle_redraw: Instant::now(),
        mouse_captured: false,
        show_hud: config.show_hud,
    };

    event_loop
        .run(move |event, elwt| match event {
            Event::AboutToWait => {
                let now = Instant::now();
                if app.should_redraw_immediately() {
                    window.request_redraw();
                } else if now >= app.next_idle_redraw {
                    app.next_idle_redraw = now + Duration::from_millis(50);
                    window.request_redraw();
                } else {
                    elwt.set_control_flow(ControlFlow::WaitUntil(app.next_idle_redraw));
                }
            }
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta: (dx, dy) },
                ..
            } => {
                // Only forward mouse motion to camera when mouse is captured (B2 fix).
                if app.mouse_captured && !app.egui_wants_pointer {
                    app.camera.process_mouse(dx as f32, dy as f32);
                }
            }
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                // D-08: Handle window Resized — store new extent, flag for recreation.
                WindowEvent::Resized(_new_size) => {
                    app.needs_resize = true;
                    app.window_extent = vk::Extent2D {
                        width: _new_size.width,
                        height: _new_size.height,
                    };
                    log::info!(
                        "Window Resized to {}x{} — flagged for swapchain recreation",
                        _new_size.width,
                        _new_size.height,
                    );
                }
                // Track cursor position and forward to egui.
                WindowEvent::CursorMoved { position, .. } => {
                    let pixels_per_point =
                        (app.egui_ctx.zoom_factor() * window.scale_factor() as f32).max(1.0);
                    let pos = egui::pos2(
                        position.x as f32 / pixels_per_point,
                        position.y as f32 / pixels_per_point,
                    );
                    app.cursor_pos = Some(pos);
                    app.egui_events.push(egui::Event::PointerMoved(pos));
                }
                WindowEvent::CursorLeft { .. } => {
                    app.cursor_pos = None;
                    app.egui_events.push(egui::Event::PointerGone);
                }
                // Mouse button events — forward to egui when it wants input,
                // otherwise ignore (camera uses DeviceEvent::MouseMotion).
                // Right-click toggles mouse capture for FPS camera look (B2 fix).
                WindowEvent::MouseInput { state, button, .. } => {
                    // B2: Right-click toggles mouse capture for FPS-style camera.
                    if button == winit::event::MouseButton::Right
                        && state == ElementState::Pressed
                        && !app.egui_wants_pointer
                    {
                        app.mouse_captured = !app.mouse_captured;
                        if app.mouse_captured {
                            let _ = window.set_cursor_grab(CursorGrabMode::Confined);
                            window.set_cursor_visible(false);
                        } else {
                            let _ = window.set_cursor_grab(CursorGrabMode::None);
                            window.set_cursor_visible(true);
                        }
                    }

                    let egui_button = match button {
                        winit::event::MouseButton::Left => Some(egui::PointerButton::Primary),
                        winit::event::MouseButton::Right => Some(egui::PointerButton::Secondary),
                        winit::event::MouseButton::Middle => Some(egui::PointerButton::Middle),
                        _ => None,
                    };
                    if let (Some(btn), Some(pos)) = (egui_button, app.cursor_pos) {
                        app.egui_events.push(egui::Event::PointerButton {
                            pos,
                            button: btn,
                            pressed: state == ElementState::Pressed,
                            modifiers: app.egui_modifiers,
                        });
                    }
                }
                // Scroll wheel events.
                WindowEvent::MouseWheel { delta, .. } => {
                    let pixels_per_point =
                        (app.egui_ctx.zoom_factor() * window.scale_factor() as f32).max(1.0);
                    let (unit, scroll) = match delta {
                        winit::event::MouseScrollDelta::LineDelta(x, y) => {
                            (egui::MouseWheelUnit::Line, egui::vec2(x, y))
                        }
                        winit::event::MouseScrollDelta::PixelDelta(pos) => (
                            egui::MouseWheelUnit::Point,
                            egui::vec2(pos.x as f32, pos.y as f32) / pixels_per_point,
                        ),
                    };
                    app.egui_events.push(egui::Event::MouseWheel {
                        unit,
                        delta: scroll,
                        modifiers: app.egui_modifiers,
                    });
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    app.egui_modifiers = modifiers_from_winit(modifiers);
                }
                WindowEvent::Ime(ime) => match ime {
                    Ime::Enabled => app
                        .egui_events
                        .push(egui::Event::Ime(egui::ImeEvent::Enabled)),
                    Ime::Preedit(text, Some(_)) => app
                        .egui_events
                        .push(egui::Event::Ime(egui::ImeEvent::Preedit(text))),
                    Ime::Preedit(_, None) => {}
                    Ime::Commit(text) => app
                        .egui_events
                        .push(egui::Event::Ime(egui::ImeEvent::Commit(text))),
                    Ime::Disabled => app
                        .egui_events
                        .push(egui::Event::Ime(egui::ImeEvent::Disabled)),
                },
                WindowEvent::Focused(focused) => {
                    app.window_focused = focused;
                    app.egui_events.push(egui::Event::WindowFocused(focused));
                    // H1 fix: Clear all held keys when window loses focus to prevent sticky movement.
                    // Also release mouse capture on focus loss (B2).
                    if !focused {
                        app.keys_pressed = KeysPressed::default();
                        if app.mouse_captured {
                            app.mouse_captured = false;
                            let _ = window.set_cursor_grab(CursorGrabMode::None);
                            window.set_cursor_visible(true);
                        }
                    }
                }
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key,
                            state,
                            repeat,
                            text,
                            ..
                        },
                    ..
                } => {
                    let pressed = state == ElementState::Pressed;
                    let key_code = match physical_key {
                        PhysicalKey::Code(key_code) => Some(key_code),
                        _ => None,
                    };

                    if let Some(key_code) = key_code
                        && let Some(egui_key) = winit_key_to_egui(key_code)
                    {
                        app.egui_events.push(egui::Event::Key {
                            key: egui_key,
                            physical_key: None,
                            pressed,
                            repeat,
                            modifiers: app.egui_modifiers,
                        });
                    }
                    if pressed
                        && !app.egui_modifiers.ctrl
                        && !app.egui_modifiers.command
                        && let Some(text) = text.as_ref()
                    {
                        let text = text.to_string();
                        if !text.chars().any(char::is_control) {
                            app.egui_events.push(egui::Event::Text(text));
                        }
                    }

                    // Camera controls — only when egui doesn't want keyboard.
                    if let Some(key_code) = key_code {
                        if !app.egui_wants_keyboard {
                            match key_code {
                                KeyCode::KeyW => app.keys_pressed.forward = pressed,
                                KeyCode::KeyS => app.keys_pressed.backward = pressed,
                                KeyCode::KeyA => app.keys_pressed.left = pressed,
                                KeyCode::KeyD => app.keys_pressed.right = pressed,
                                KeyCode::Space => app.keys_pressed.up = pressed,
                                KeyCode::ShiftLeft => app.keys_pressed.down = pressed,
                                _ => {}
                            }
                        } else if !pressed {
                            match key_code {
                                KeyCode::KeyW => app.keys_pressed.forward = false,
                                KeyCode::KeyS => app.keys_pressed.backward = false,
                                KeyCode::KeyA => app.keys_pressed.left = false,
                                KeyCode::KeyD => app.keys_pressed.right = false,
                                KeyCode::Space => app.keys_pressed.up = false,
                                KeyCode::ShiftLeft => app.keys_pressed.down = false,
                                _ => {}
                            }
                        }
                        // F1: Toggle HUD visibility (H9 fix).
                        if key_code == KeyCode::F1 && pressed {
                            app.show_hud = !app.show_hud;
                        }
                        // Escape: Release mouse capture first, exit on second press (M8 fix).
                        if key_code == KeyCode::Escape && pressed {
                            if app.mouse_captured {
                                app.mouse_captured = false;
                                let _ = window.set_cursor_grab(CursorGrabMode::None);
                                window.set_cursor_visible(true);
                            } else {
                                elwt.exit();
                            }
                        }
                    }
                }
                WindowEvent::RedrawRequested => {
                    app.tick(&window);
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|err| anyhow!("winit event loop failed: {err}"))
}

fn modifiers_from_winit(modifiers: winit::event::Modifiers) -> egui::Modifiers {
    let state = modifiers.state();
    let alt = state.alt_key();
    let ctrl = state.control_key();
    let shift = state.shift_key();
    let mac_cmd = cfg!(target_os = "macos") && state.super_key();
    let command = if cfg!(target_os = "macos") {
        mac_cmd
    } else {
        ctrl
    };
    egui::Modifiers {
        alt,
        ctrl,
        shift,
        mac_cmd,
        command,
    }
}

/// Map winit physical key codes to egui key identifiers.
fn winit_key_to_egui(key: KeyCode) -> Option<egui::Key> {
    Some(match key {
        KeyCode::ArrowDown => egui::Key::ArrowDown,
        KeyCode::ArrowLeft => egui::Key::ArrowLeft,
        KeyCode::ArrowRight => egui::Key::ArrowRight,
        KeyCode::ArrowUp => egui::Key::ArrowUp,
        KeyCode::Enter => egui::Key::Enter,
        KeyCode::Tab => egui::Key::Tab,
        KeyCode::Backspace => egui::Key::Backspace,
        KeyCode::Delete => egui::Key::Delete,
        KeyCode::Home => egui::Key::Home,
        KeyCode::End => egui::Key::End,
        KeyCode::PageUp => egui::Key::PageUp,
        KeyCode::PageDown => egui::Key::PageDown,
        KeyCode::Escape => egui::Key::Escape,
        KeyCode::Space => egui::Key::Space,
        KeyCode::Digit0 => egui::Key::Num0,
        KeyCode::Digit1 => egui::Key::Num1,
        KeyCode::Digit2 => egui::Key::Num2,
        KeyCode::Digit3 => egui::Key::Num3,
        KeyCode::Digit4 => egui::Key::Num4,
        KeyCode::Digit5 => egui::Key::Num5,
        KeyCode::Digit6 => egui::Key::Num6,
        KeyCode::Digit7 => egui::Key::Num7,
        KeyCode::Digit8 => egui::Key::Num8,
        KeyCode::Digit9 => egui::Key::Num9,
        KeyCode::KeyA => egui::Key::A,
        KeyCode::KeyB => egui::Key::B,
        KeyCode::KeyC => egui::Key::C,
        KeyCode::KeyD => egui::Key::D,
        KeyCode::KeyE => egui::Key::E,
        KeyCode::KeyF => egui::Key::F,
        KeyCode::KeyG => egui::Key::G,
        KeyCode::KeyH => egui::Key::H,
        KeyCode::KeyI => egui::Key::I,
        KeyCode::KeyJ => egui::Key::J,
        KeyCode::KeyK => egui::Key::K,
        KeyCode::KeyL => egui::Key::L,
        KeyCode::KeyM => egui::Key::M,
        KeyCode::KeyN => egui::Key::N,
        KeyCode::KeyO => egui::Key::O,
        KeyCode::KeyP => egui::Key::P,
        KeyCode::KeyQ => egui::Key::Q,
        KeyCode::KeyR => egui::Key::R,
        KeyCode::KeyS => egui::Key::S,
        KeyCode::KeyT => egui::Key::T,
        KeyCode::KeyU => egui::Key::U,
        KeyCode::KeyV => egui::Key::V,
        KeyCode::KeyW => egui::Key::W,
        KeyCode::KeyX => egui::Key::X,
        KeyCode::KeyY => egui::Key::Y,
        KeyCode::KeyZ => egui::Key::Z,
        _ => return None,
    })
}
