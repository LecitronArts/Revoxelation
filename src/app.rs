use anyhow::{Context, Result, anyhow};
use ash::vk;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::time::Instant;
use winit::{
    event::{DeviceEvent, ElementState, Event, KeyEvent, WindowEvent},
    event_loop::EventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::WindowBuilder,
};

use crate::config::RuntimeConfig;
use crate::meshing::MeshingState;
use crate::renderer::{
    Renderer, FrameOutcome, chunk_pool::ChunkPool, cull_pipeline::ChunkCullPipeline, egui_backend::EguiAshBackend,
    mesh_pipeline::ChunkMeshPipeline, staging_ring::StagingRing, camera::{CameraKey, FpsCamera},
    swapchain::recreate_swapchain_context,
    pipeline_cache::PipelineCache,
    perf_counters::GpuPerfCounters,
    bindless::{BindlessTable, BINDING_SCENE, BINDING_FRUSTUM_PLANES, BINDING_DRAW_COUNT, BINDING_HIZ_CONFIG},
};
use crate::runtime::scheduler::StreamingState;

/// Pending egui output to be consumed by submit_frame.
pub struct PendingEguiOutput {
    pub textures_delta: egui::TexturesDelta,
    pub clipped_primitives: Vec<egui::ClippedPrimitive>,
    pub screen_size: [f32; 2],
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

impl App {
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
        let dt = now.duration_since(self.last_frame_time).as_secs_f32();
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

        // Tick day-night cycle (LGHT-05).
        if let Some(ls) = &mut self.renderer.lighting_state {
            ls.tick_day_night(dt);
        }

        // Build egui frame.
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(screen_size[0], screen_size[1]),
            )),
            ..Default::default()
        };

        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            egui::Window::new("Debug").show(ctx, |ui| {
                ui.label(format!("Frame: {}", self.frame_index));
                ui.separator();
                let pc = &self.perf_counters;
                ui.label(format!(
                    "Chunks: {}/{} | Slots: {}/{} | Frame: {:.1}ms",
                    pc.visible_chunks, pc.total_chunks,
                    pc.total_chunks, pc.chunk_capacity,
                    pc.frame_time_ms
                ));

                // Meshlet LOD statistics (MSHL-05).
                ui.separator();
                ui.label(format!(
                    "Meshlets: {} (LOD0: {}, LOD1: {})",
                    pc.total_meshlets, pc.lod0_meshlets, pc.lod1_meshlets
                ));
                ui.label(format!(
                    "Visible: {} | Cull rate: {:.1}%",
                    pc.visible_meshlets, pc.meshlet_cull_rate * 100.0
                ));
            });

            // Meshlet culling controls (MSHL-05).
            egui::Window::new("Meshlet Culling").show(ctx, |ui| {
                ui.checkbox(
                    &mut self.renderer.meshlet_cull_backface,
                    "Backface culling",
                );
                ui.checkbox(
                    &mut self.renderer.meshlet_cull_frustum,
                    "Frustum culling",
                );
                ui.checkbox(
                    &mut self.renderer.meshlet_cull_hiz,
                    "Hi-Z occlusion culling",
                );
                ui.checkbox(
                    &mut self.renderer.use_meshlet_rendering,
                    "Meshlet rendering",
                );
                ui.separator();
                ui.label("SSE threshold (LOD)");
                ui.add(
                    egui::Slider::new(&mut self.renderer.sse_threshold, 0.1..=16.0)
                        .text("px"),
                );
            });

            // Lighting controls (LGHT-01).
            if let Some(ls) = &mut self.renderer.lighting_state {
                egui::Window::new("Lighting").show(ctx, |ui| {
                    ui.label("Sun Elevation");
                    ui.add(
                        egui::Slider::new(&mut ls.sun_elevation, 0.0..=90.0)
                            .text("deg"),
                    );
                    ui.label("Sun Azimuth");
                    ui.add(
                        egui::Slider::new(&mut ls.sun_azimuth, 0.0..=360.0)
                            .text("deg"),
                    );
                    ui.label("Sun Intensity");
                    ui.add(
                        egui::Slider::new(&mut ls.sun_intensity, 0.0..=5.0),
                    );
                    ui.label("Ambient Intensity");
                    ui.add(
                        egui::Slider::new(&mut ls.ambient_intensity, 0.0..=1.0),
                    );
                    ui.label("Time of Day");
                    ui.add(
                        egui::Slider::new(&mut ls.time_of_day, 0.0..=1.0),
                    );
                });
            }

            // CSM Shadow controls (LGHT-02).
            {
                let sc = &mut self.renderer.shadow_config;
                egui::Window::new("Shadows").show(ctx, |ui| {
                    ui.checkbox(&mut sc.enabled, "Shadows enabled");
                    ui.separator();
                    ui.label("Split Lambda");
                    ui.add(
                        egui::Slider::new(&mut sc.split_lambda, 0.0..=1.0)
                            .text("lambda"),
                    );
                    ui.label("Bias Constant");
                    ui.add(
                        egui::Slider::new(&mut sc.bias_constant, 0.0..=5.0),
                    );
                    ui.label("Bias Slope");
                    ui.add(
                        egui::Slider::new(&mut sc.bias_slope, 0.0..=5.0),
                    );
                    ui.checkbox(&mut sc.debug_cascades, "Debug cascade colors");
                    if let Some(sm) = &self.renderer.shadow_map {
                        ui.separator();
                        ui.label(format!("Resolution: {}x{}", sm.resolution, sm.resolution));
                        ui.label(format!("Cascades: {}", crate::renderer::shadow::CASCADE_COUNT));
                    }
                });
            }

            // SSAO controls (LGHT-03).
            {
                let ssao_cfg = &mut self.renderer.ssao_config;
                egui::Window::new("SSAO").show(ctx, |ui| {
                    ui.checkbox(&mut ssao_cfg.enabled, "SSAO enabled");
                    ui.separator();

                    // Algorithm selector.
                    let algo_label = ssao_cfg.algorithm.as_str();
                    egui::ComboBox::from_label("Algorithm")
                        .selected_text(algo_label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut ssao_cfg.algorithm,
                                crate::renderer::ssao::SsaoAlgorithm::Gtao,
                                "GTAO",
                            );
                            ui.selectable_value(
                                &mut ssao_cfg.algorithm,
                                crate::renderer::ssao::SsaoAlgorithm::HbaoPlus,
                                "HBAO+",
                            );
                            ui.selectable_value(
                                &mut ssao_cfg.algorithm,
                                crate::renderer::ssao::SsaoAlgorithm::ClassicSsao,
                                "Classic SSAO",
                            );
                        });

                    ui.label("AO Radius");
                    ui.add(
                        egui::Slider::new(&mut ssao_cfg.radius, 0.1..=2.0)
                            .text("world"),
                    );
                    ui.label("AO Intensity");
                    ui.add(
                        egui::Slider::new(&mut ssao_cfg.intensity, 0.0..=3.0),
                    );
                    ui.label("Sample Count");
                    ui.add(
                        egui::Slider::new(&mut ssao_cfg.sample_count, 4..=64),
                    );
                    ui.checkbox(&mut ssao_cfg.half_resolution, "Half resolution");
                    ui.checkbox(&mut ssao_cfg.debug_view, "Debug AO view");

                    if let Some(ssao) = &self.renderer.ssao_pass {
                        ui.separator();
                        ui.label(format!("AO size: {}x{}", ssao.width, ssao.height));
                    }
                });
            }

            // Sky / Atmosphere / Fog controls (LGHT-05).
            if let Some(sky) = &mut self.renderer.sky_renderer {
                egui::Window::new("Sky & Atmosphere").show(ctx, |ui| {
                    ui.checkbox(&mut sky.config.enabled, "Sky enabled");
                    ui.separator();

                    // Atmosphere model selector.
                    let model_label = sky.config.atmosphere_model.as_str();
                    egui::ComboBox::from_label("Atmosphere Model")
                        .selected_text(model_label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut sky.config.atmosphere_model,
                                crate::renderer::sky::AtmosphereModel::Preetham,
                                "Preetham",
                            );
                            ui.selectable_value(
                                &mut sky.config.atmosphere_model,
                                crate::renderer::sky::AtmosphereModel::HosekWilkie,
                                "Hosek-Wilkie",
                            );
                        });

                    ui.label("Turbidity");
                    ui.add(
                        egui::Slider::new(&mut sky.config.turbidity, 1.0..=10.0),
                    );
                    ui.label("Sun Disk Size");
                    ui.add(
                        egui::Slider::new(&mut sky.config.sun_angular_radius, 0.001..=0.05)
                            .text("rad"),
                    );
                });
            }

            // Day-Night Cycle controls (LGHT-05).
            if let Some(ls) = &mut self.renderer.lighting_state {
                egui::Window::new("Day-Night Cycle").show(ctx, |ui| {
                    ui.checkbox(&mut ls.use_day_night_cycle, "Use day-night cycle");
                    ui.separator();

                    // Time display.
                    let time_str = ls.day_night.time_as_hhmm();
                    let elevation = ls.day_night.sun_elevation();
                    ui.label(format!("Time: {} | Sun elev: {:.2}", time_str, elevation));
                    ui.separator();

                    // Time of day slider.
                    ui.label("Time of Day");
                    let time_labels = "Midnight          Dawn          Noon          Dusk";
                    ui.label(time_labels);
                    ui.add(
                        egui::Slider::new(&mut ls.day_night.time_of_day, 0.0..=1.0),
                    );

                    // Day speed slider.
                    ui.label("Day Speed (seconds per game day)");
                    ui.add(
                        egui::Slider::new(&mut ls.day_night.day_speed, 60.0..=3600.0)
                            .text("sec"),
                    );

                    // Pause toggle.
                    ui.checkbox(&mut ls.day_night.paused, "Paused");

                    // Lighting summary.
                    ui.separator();
                    ui.label(format!("Sun dir: [{:.2}, {:.2}, {:.2}]",
                        ls.sun_color[0], ls.sun_color[1], ls.sun_color[2]));
                    ui.label(format!("Sun intensity: {:.2}", ls.sun_intensity));
                    ui.label(format!("Ambient: [{:.2}, {:.2}, {:.2}] @ {:.2}",
                        ls.ambient_color[0], ls.ambient_color[1], ls.ambient_color[2],
                        ls.ambient_intensity));
                });

                // Fog controls (LGHT-05).
                egui::Window::new("Distance Fog").show(ctx, |ui| {
                    ui.checkbox(&mut ls.fog_config.enabled, "Fog enabled");
                    ui.separator();

                    // Fog type selector.
                    let fog_label = ls.fog_config.fog_type.as_str();
                    egui::ComboBox::from_label("Fog Type")
                        .selected_text(fog_label)
                        .show_ui(ui, |ui| {
                            for &ft in crate::renderer::lighting::FogType::all() {
                                ui.selectable_value(
                                    &mut ls.fog_config.fog_type,
                                    ft,
                                    ft.as_str(),
                                );
                            }
                        });

                    ui.label("Fog Density");
                    ui.add(
                        egui::Slider::new(&mut ls.fog_config.density, 0.001..=0.1)
                            .logarithmic(true),
                    );

                    // Linear fog start/end (only relevant for linear fog type).
                    ui.label("Fog Start (linear)");
                    ui.add(
                        egui::Slider::new(&mut ls.fog_config.start, 10.0..=500.0)
                            .text("m"),
                    );
                    ui.label("Fog End (linear)");
                    ui.add(
                        egui::Slider::new(&mut ls.fog_config.end, 50.0..=2000.0)
                            .text("m"),
                    );

                    // Show current fog color.
                    let fc = ls.day_night.fog_color();
                    ui.label(format!("Fog color: [{:.2}, {:.2}, {:.2}]", fc[0], fc[1], fc[2]));
                });
            }
        });

        let clipped_primitives =
            self.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let textures_delta = full_output.textures_delta;

        // Store egui output for submit_frame to consume.
        self.renderer.pending_egui_output = Some(PendingEguiOutput {
            textures_delta,
            clipped_primitives,
            screen_size,
        });

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
        let aspect = if screen_size[1] > 0.0 { screen_size[0] / screen_size[1] } else { 1.0 };
        let camera_uniforms = self.camera.view_proj(aspect);
        let current_time = self.engine_start_time.elapsed().as_secs_f32();
        match crate::renderer::submit_frame(&mut self.renderer, self.frame_index, &camera_uniforms, current_time) {
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
        let total_chunks = self.renderer.chunk_pool.as_ref()
            .map(|cp| cp.active_draw_count())
            .unwrap_or(0);
        let chunk_capacity = self.renderer.chunk_pool.as_ref()
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
        self.perf_counters.sse_threshold = self.renderer.sse_threshold;

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
        let chunk_pool = renderer.chunk_pool.as_ref().expect("chunk pool must be initialized before bindless registration");
        bindless.register_buffer(&renderer.device_ctx.device, BINDING_SCENE, chunk_pool.scene_buffer(), vk::WHOLE_SIZE);
    }

    let bindless_layout = bindless.descriptor_set_layout;
    renderer.bindless = Some(bindless);

    // Create pipelines using the shared bindless layout.
    renderer.mesh_pipeline = Some(ChunkMeshPipeline::new(&renderer, bindless_layout)?);
    renderer.cull_pipeline = Some(ChunkCullPipeline::new(&mut renderer, bindless_layout)?);

    // Create meshlet cull pipeline (MSHL-02, D-10).
    renderer.meshlet_cull_pipeline = Some(
        crate::renderer::cull_pipeline::MeshletCullPipeline::new(&renderer, bindless_layout)?
    );

    // Register cull pipeline auxiliary buffers with the bindless table.
    {
        let cull_pipeline = renderer.cull_pipeline.as_ref().expect("cull pipeline must be initialized");
        let bindless = renderer.bindless.as_ref().expect("bindless must be initialized");
        bindless.register_buffer(&renderer.device_ctx.device, BINDING_FRUSTUM_PLANES, cull_pipeline.frustum_planes_buffer, std::mem::size_of::<crate::renderer::camera::FrustumPlanes>() as u64);
        bindless.register_buffer(&renderer.device_ctx.device, BINDING_DRAW_COUNT, cull_pipeline.draw_count_buffer, std::mem::size_of::<u32>() as u64);
        bindless.register_buffer(&renderer.device_ctx.device, BINDING_HIZ_CONFIG, cull_pipeline.hiz_config_buffer, std::mem::size_of::<crate::renderer::cull_pipeline::HiZConfig>() as u64);
    }

    // Create and upload material table to bindless binding 8.
    {
        let material_table = crate::renderer::material::MaterialTable::default_table();
        let (buf, alloc) = material_table.upload(&mut renderer)?;
        renderer.material_buffer = Some(buf);
        renderer.material_allocation = Some(alloc);
    }

    // Create texture array and register at bindless binding 9.
    renderer.texture_array = Some(crate::renderer::texture_array::TextureArray::new(&mut renderer)?);

    // Create PBR texture arrays (MR, normal, emissive) at bindings 19/20/21 (LGHT-01).
    renderer.mr_texture_array = Some(crate::renderer::texture_array::new_mr_array_16(&mut renderer)?);
    renderer.normal_texture_array = Some(crate::renderer::texture_array::new_normal_array_16(&mut renderer)?);
    renderer.emissive_texture_array = Some(crate::renderer::texture_array::new_emissive_array_16(&mut renderer)?);

    // Create directional lighting state (binding 18 SSBO) (LGHT-01).
    renderer.lighting_state = Some(crate::renderer::lighting::LightingState::new(&mut renderer)?);

    // Create point light manager (binding 22 SSBO) (LGHT-01).
    renderer.point_light_manager = Some(crate::renderer::point_light::PointLightManager::new(&mut renderer)?);

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
        let ssao_config = renderer.ssao_config.clone();
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
    renderer.readback_counters = Some(
        crate::renderer::perf_counters::GpuReadbackCounters::new(&mut renderer)?
    );

    let config = RuntimeConfig::load();

    let mut app = App {
        renderer,
        streaming: StreamingState::new(),
        meshing: MeshingState::default(),
        egui_ctx: egui::Context::default(),
        camera: {
            let mut cam = FpsCamera::default();
            cam.move_speed = config.camera_speed;
            cam
        },
        frame_index: 0,
        keys_pressed: KeysPressed::default(),
        last_frame_time: Instant::now(),
        engine_start_time: Instant::now(),
        needs_resize: false,
        window_extent: extent,
        perf_counters: GpuPerfCounters::default(),
        config,
        #[cfg(all(debug_assertions, feature = "hot-reload"))]
        hot_reload: crate::renderer::hot_reload::ShaderHotReload::new(),
    };

    event_loop
        .run(move |event, elwt| match event {
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta: (dx, dy) },
                ..
            } => {
                app.camera.process_mouse(dx as f32, dy as f32);
            }
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                // D-08: Handle window Resized — store new extent, flag for recreation.
                WindowEvent::Resized(_new_size) => {
                    app.needs_resize = true;
                    log::info!(
                        "Window Resized to {}x{} — flagged for swapchain recreation",
                        _new_size.width,
                        _new_size.height,
                    );
                }
                WindowEvent::KeyboardInput {
                    event:
                        KeyEvent {
                            physical_key: PhysicalKey::Code(key_code),
                            state,
                            ..
                        },
                    ..
                } => {
                    let pressed = state == ElementState::Pressed;
                    match key_code {
                        KeyCode::KeyW => app.keys_pressed.forward = pressed,
                        KeyCode::KeyS => app.keys_pressed.backward = pressed,
                        KeyCode::KeyA => app.keys_pressed.left = pressed,
                        KeyCode::KeyD => app.keys_pressed.right = pressed,
                        KeyCode::Space => app.keys_pressed.up = pressed,
                        KeyCode::ShiftLeft => app.keys_pressed.down = pressed,
                        KeyCode::Escape if pressed => elwt.exit(),
                        _ => {}
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
