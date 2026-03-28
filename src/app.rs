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
    bindless::BindlessTable,
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
        bindless.register_buffer(&renderer.device_ctx.device, 0, chunk_pool.scene_buffer(), vk::WHOLE_SIZE);
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
        bindless.register_buffer(&renderer.device_ctx.device, 4, cull_pipeline.frustum_planes_buffer, std::mem::size_of::<crate::renderer::camera::FrustumPlanes>() as u64);
        bindless.register_buffer(&renderer.device_ctx.device, 5, cull_pipeline.draw_count_buffer, std::mem::size_of::<u32>() as u64);
        bindless.register_buffer(&renderer.device_ctx.device, 6, cull_pipeline.hiz_config_buffer, std::mem::size_of::<crate::renderer::cull_pipeline::HiZConfig>() as u64);
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

    renderer.egui_backend = Some(EguiAshBackend::new(&mut renderer)?);

    let mut app = App {
        renderer,
        streaming: StreamingState::new(),
        meshing: MeshingState::default(),
        egui_ctx: egui::Context::default(),
        camera: FpsCamera::default(),
        frame_index: 0,
        keys_pressed: KeysPressed::default(),
        last_frame_time: Instant::now(),
        needs_resize: false,
        window_extent: extent,
        perf_counters: GpuPerfCounters::default(),
        config: RuntimeConfig::load(),
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
                app.camera.process_mouse(dx as f32, dy as f32, 1.0);
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
                    // D-07: When window extent is 0×0 (minimized), skip rendering entirely.
                    let size = window.inner_size();
                    if size.width == 0 || size.height == 0 {
                        return;
                    }

                    // D-08: If needs_resize, recreate swapchain before rendering.
                    if app.needs_resize {
                        let new_extent = vk::Extent2D {
                            width: size.width,
                            height: size.height,
                        };
                        if let Err(e) = recreate_swapchain_context(&mut app.renderer, new_extent) {
                            log::error!("Failed to recreate swapchain on resize: {e:#}");
                            return;
                        }
                        app.needs_resize = false;
                    }

                    // Delta time for smooth movement.
                    let now = Instant::now();
                    let dt = now.duration_since(app.last_frame_time).as_secs_f32();
                    app.last_frame_time = now;

                    // Apply continuous keyboard movement.
                    if app.keys_pressed.forward {
                        app.camera.process_keyboard(CameraKey::Forward, true, dt);
                    }
                    if app.keys_pressed.backward {
                        app.camera.process_keyboard(CameraKey::Backward, true, dt);
                    }
                    if app.keys_pressed.left {
                        app.camera.process_keyboard(CameraKey::Left, true, dt);
                    }
                    if app.keys_pressed.right {
                        app.camera.process_keyboard(CameraKey::Right, true, dt);
                    }
                    if app.keys_pressed.up {
                        app.camera.process_keyboard(CameraKey::Up, true, dt);
                    }
                    if app.keys_pressed.down {
                        app.camera.process_keyboard(CameraKey::Down, true, dt);
                    }

                    let screen_size = [size.width as f32, size.height as f32];

                    // Build egui frame.
                    let raw_input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(screen_size[0], screen_size[1]),
                        )),
                        ..Default::default()
                    };

                    let full_output = app.egui_ctx.run(raw_input, |ctx| {
                        egui::Window::new("Debug").show(ctx, |ui| {
                            ui.label(format!("Frame: {}", app.frame_index));
                            ui.separator();
                            let pc = &app.perf_counters;
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
                                &mut app.renderer.meshlet_cull_backface,
                                "Backface culling",
                            );
                            ui.checkbox(
                                &mut app.renderer.meshlet_cull_frustum,
                                "Frustum culling",
                            );
                            ui.checkbox(
                                &mut app.renderer.meshlet_cull_hiz,
                                "Hi-Z occlusion culling",
                            );
                            ui.checkbox(
                                &mut app.renderer.use_meshlet_rendering,
                                "Meshlet rendering",
                            );
                            ui.separator();
                            ui.label("SSE threshold (LOD)");
                            ui.add(
                                egui::Slider::new(&mut app.renderer.sse_threshold, 0.1..=16.0)
                                    .text("px"),
                            );
                        });
                    });

                    let clipped_primitives =
                        app.egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
                    let textures_delta = full_output.textures_delta;

                    // Store egui output for submit_frame to consume.
                    app.renderer.pending_egui_output = Some(PendingEguiOutput {
                        textures_delta,
                        clipped_primitives,
                        screen_size,
                    });

                    // Run scheduler frame, then handle renderer submission.
                    let camera_pos = app.camera.position.to_array();
                    let _result = crate::runtime::run_frame(
                        &mut app.streaming,
                        &mut app.meshing,
                        Some(&mut app.renderer),
                        app.frame_index,
                        camera_pos,
                        screen_size[1],
                        app.camera.fov_y,
                    );

                    // Drain pending render deltas and submit frame from app-owned renderer.
                    crate::runtime::scheduler::drain_pending_render_deltas_into_renderer(
                        &mut app.streaming,
                        &mut app.renderer,
                    );
                    let aspect = if screen_size[1] > 0.0 { screen_size[0] / screen_size[1] } else { 1.0 };
                    let camera_uniforms = app.camera.view_proj(aspect);
                    match crate::renderer::submit_frame(&mut app.renderer, app.frame_index, &camera_uniforms) {
                        Ok(FrameOutcome::Submitted) => {}
                        Ok(FrameOutcome::NeedsRecreate) => {
                            // submit_frame signalled swapchain is stale — recreate next frame.
                            app.needs_resize = true;
                        }
                        Err(e) => {
                            log::error!("submit_frame failed: {e:#}");
                        }
                    }

                    // Update performance counters for next frame's HUD.
                    let frame_time_ms = dt * 1000.0;
                    let total_chunks = app.renderer.chunk_pool.as_ref()
                        .map(|cp| cp.active_draw_count())
                        .unwrap_or(0);
                    let chunk_capacity = app.renderer.chunk_pool.as_ref()
                        .map(|cp| cp.capacity() as u32)
                        .unwrap_or(0);
                    app.perf_counters.frame_time_ms = frame_time_ms;
                    app.perf_counters.total_chunks = total_chunks;
                    app.perf_counters.chunk_capacity = chunk_capacity;
                    // visible_chunks approximated as total (actual readback deferred to future)
                    app.perf_counters.visible_chunks = total_chunks;

                    // Meshlet LOD statistics (MSHL-05).
                    if let Some(meshlet_pool) = &app.renderer.meshlet_pool {
                        app.perf_counters.total_meshlets = meshlet_pool.active_meshlet_count();
                    }
                    app.perf_counters.sse_threshold = app.renderer.sse_threshold;

                    // Shader hot-reload (debug builds with hot-reload feature only).
                    #[cfg(all(debug_assertions, feature = "hot-reload"))]
                    {
                        if let Err(e) = app.hot_reload.check_and_reload(&mut app.renderer) {
                            log::error!("Shader hot-reload error: {e:#}");
                        }
                    }

                    app.frame_index = app.frame_index.saturating_add(1);
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|err| anyhow!("winit event loop failed: {err}"))
}
