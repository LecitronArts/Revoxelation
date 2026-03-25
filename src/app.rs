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

use crate::meshing::MeshingState;
use crate::renderer::{
    Renderer, chunk_pool::ChunkPool, cull_pipeline::ChunkCullPipeline, egui_backend::EguiAshBackend,
    mesh_pipeline::ChunkMeshPipeline, camera::{CameraKey, FpsCamera},
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
    renderer.mesh_pipeline = Some(ChunkMeshPipeline::new(&renderer)?);
    renderer.cull_pipeline = Some(ChunkCullPipeline::new(&renderer)?);
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

                    let size = window.inner_size();
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
                    let _result = crate::runtime::run_frame(
                        &mut app.streaming,
                        &mut app.meshing,
                        Some(&mut app.renderer),
                        app.frame_index,
                    );

                    // Drain pending render deltas and submit frame from app-owned renderer.
                    crate::runtime::scheduler::drain_pending_render_deltas_into_renderer(
                        &mut app.streaming,
                        &mut app.renderer,
                    );
                    let aspect = if screen_size[1] > 0.0 { screen_size[0] / screen_size[1] } else { 1.0 };
                    let camera_uniforms = app.camera.view_proj(aspect);
                    if let Err(e) = crate::renderer::submit_frame(&mut app.renderer, app.frame_index, &camera_uniforms) {
                        log::error!("submit_frame failed: {e:#}");
                    }

                    app.frame_index = app.frame_index.saturating_add(1);
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|err| anyhow!("winit event loop failed: {err}"))
}
