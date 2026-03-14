use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use egui::{CollapsingHeader, Color32, ComboBox, Slider};
use log::{info, warn};
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, ElementState, Event, MouseButton, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::CursorGrabMode;
use winit::window::WindowBuilder;

use crate::ecs::{CameraSettings, CameraState, ControllerSettings, LogicScheduler};
use crate::renderer::{
    DEBUG_OVERLAY_MODE_CLAMP_DIFF, DEBUG_OVERLAY_MODE_HISTORY_VALIDITY,
    DEBUG_OVERLAY_MODE_HISTORY_WEIGHT, DEBUG_OVERLAY_MODE_MAX, DEBUG_OVERLAY_MODE_MOTION,
    DEBUG_OVERLAY_MODE_NONE, DEBUG_OVERLAY_MODE_PROBE, DEBUG_OVERLAY_MODE_REJECT_REASON,
    DEBUG_OVERLAY_MODE_TEMPORAL_VARIANCE, Renderer, RendererSettings, RendererStats,
};
use crate::world::{VoxelWorld, WorldGenConfig};

#[derive(Debug, Default, Clone, Copy)]
struct UiActions {
    reset_camera_pose: bool,
    reset_history: bool,
}

pub fn run() -> Result<()> {
    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("Revoxelation - Compute Path Traced Voxels")
            .with_inner_size(PhysicalSize::new(1280, 720))
            .build(&event_loop)?,
    );

    let world = Arc::new(VoxelWorld::new());
    world.spawn_generation(WorldGenConfig::default());

    let mut logic = LogicScheduler::new();
    let mut renderer = pollster::block_on(Renderer::new(window.clone(), world.clone()))?;
    let egui_ctx = egui::Context::default();
    let mut egui_state = egui_winit::State::new(
        egui_ctx.clone(),
        egui::ViewportId::ROOT,
        &event_loop,
        Some(window.scale_factor() as f32),
        None,
    );
    let mut last_status_log = Instant::now();
    let mut last_world_sync = Instant::now();
    let mut last_frame_time = Instant::now();
    let mut frame_ms = 16.6_f32;
    let mut pointer_captured = false;

    event_loop.run(move |event, target| {
        target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::WindowEvent { event, window_id } if window_id == window.id() => {
                let egui_event = egui_state.on_window_event(&window, &event);
                if egui_event.repaint {
                    window.request_redraw();
                }
                match event {
                    WindowEvent::CloseRequested => target.exit(),
                    WindowEvent::Resized(new_size) => renderer.resize(new_size),
                    WindowEvent::Focused(false) => {
                        pointer_captured = false;
                        release_pointer(&window);
                        logic.clear_input();
                    }
                    WindowEvent::MouseInput {
                        state: ElementState::Pressed,
                        button: MouseButton::Left,
                        ..
                    } => {
                        if !pointer_captured && !egui_event.consumed && capture_pointer(&window) {
                            pointer_captured = true;
                        }
                    }
                    WindowEvent::KeyboardInput { event, .. } => {
                        if egui_event.consumed {
                            return;
                        }
                        if let PhysicalKey::Code(code) = event.physical_key {
                            let pressed = event.state == ElementState::Pressed;
                            if code == KeyCode::Escape && pressed {
                                pointer_captured = false;
                                release_pointer(&window);
                                logic.clear_input();
                            } else {
                                logic.set_key_state(code, pressed);
                            }
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        logic.update();
                        if world.take_dirty() {
                            let (_, _, finished) = world.generation_snapshot();
                            if finished || last_world_sync.elapsed().as_millis() >= 200 {
                                renderer.sync_world(&world);
                                last_world_sync = Instant::now();
                            }
                        }

                        if last_status_log.elapsed().as_secs_f32() > 1.0 {
                            let (generated, total, finished) = world.generation_snapshot();
                            info!("world gen: {generated}/{total} chunks, done={finished}");
                            last_status_log = Instant::now();
                        }

                        let mut ui_settings = renderer.settings();
                        let mut camera_settings = logic.camera_settings();
                        let mut controller_settings = logic.controller_settings();
                        let camera_state = logic.camera_state();
                        let ui_stats = renderer.stats();
                        let mut ui_actions = UiActions::default();
                        let raw_input = egui_state.take_egui_input(&window);
                        let full_output = egui_ctx.run(raw_input, |ctx| {
                            draw_renderer_ui(
                                ctx,
                                &mut ui_settings,
                                &mut camera_settings,
                                &mut controller_settings,
                                ui_stats,
                                camera_state,
                                frame_ms,
                                pointer_captured,
                                &mut ui_actions,
                            );
                        });
                        egui_state.handle_platform_output(&window, full_output.platform_output);
                        renderer.update_settings(ui_settings);
                        logic.set_camera_settings(camera_settings);
                        logic.set_controller_settings(controller_settings);
                        if ui_actions.reset_camera_pose {
                            logic.reset_camera_pose();
                        }
                        if ui_actions.reset_history {
                            renderer.force_reset_history();
                        }

                        let camera = logic.primary_camera();
                        let paint_jobs =
                            egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
                        match renderer.render(
                            &camera,
                            &paint_jobs,
                            &full_output.textures_delta,
                            full_output.pixels_per_point,
                        ) {
                            Ok(()) => {}
                            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                                renderer.reconfigure();
                            }
                            Err(wgpu::SurfaceError::OutOfMemory) => target.exit(),
                            Err(wgpu::SurfaceError::Timeout) => warn!("surface timeout"),
                        }

                        let now = Instant::now();
                        let dt_ms = (now - last_frame_time).as_secs_f32() * 1000.0;
                        frame_ms = frame_ms * 0.9 + dt_ms * 0.1;
                        last_frame_time = now;
                    }
                    _ => {}
                }
            }
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                if pointer_captured {
                    logic.add_mouse_delta(delta.0 as f32, delta.1 as f32);
                }
            }
            Event::AboutToWait => window.request_redraw(),
            _ => {}
        }
    })?;

    Ok(())
}

fn draw_renderer_ui(
    ctx: &egui::Context,
    settings: &mut RendererSettings,
    camera_settings: &mut CameraSettings,
    controller_settings: &mut ControllerSettings,
    stats: RendererStats,
    camera_state: CameraState,
    frame_ms: f32,
    pointer_captured: bool,
    actions: &mut UiActions,
) {
    egui::SidePanel::left("revoxelation_control_panel")
        .resizable(true)
        .min_width(300.0)
        .default_width(360.0)
        .show(ctx, |ui| {
            ui.heading("Revoxelation Control");
            ui.label(if pointer_captured {
                "Pointer captured (Esc release)"
            } else {
                "Pointer free (Left click capture)"
            });
            ui.label(format!(
                "{:.2} ms | {:.1} FPS | {}x{}",
                frame_ms,
                1000.0 / frame_ms.max(0.1),
                stats.resolution[0],
                stats.resolution[1]
            ));
            ui.horizontal(|ui| {
                if ui.button("Reset Camera Pose").clicked() {
                    actions.reset_camera_pose = true;
                }
                if ui.button("Reset Accumulation").clicked() {
                    actions.reset_history = true;
                }
            });
            ui.separator();

            CollapsingHeader::new("Presets")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Speed").clicked() {
                            settings.max_bounces = 2;
                            settings.dda_max_steps = 256;
                            settings.svgf_enabled = true;
                            settings.svgf_passes = 2;
                            settings.restir_di_enabled = true;
                            settings.restir_gi_enabled = false;
                            settings.restir_spatial_radius = 0;
                            actions.reset_history = true;
                        }
                        if ui.button("Balanced").clicked() {
                            *settings = RendererSettings::default();
                            actions.reset_history = true;
                        }
                        if ui.button("Quality").clicked() {
                            settings.max_bounces = 6;
                            settings.dda_max_steps = 1024;
                            settings.max_history = 64.0;
                            settings.svgf_enabled = true;
                            settings.svgf_passes = 5;
                            settings.svgf_step_scale = 1;
                            settings.restir_di_enabled = true;
                            settings.restir_gi_enabled = true;
                            settings.restir_spatial_radius = 2;
                            settings.restir_temporal_boost = 1.5;
                            settings.restir_gi_directional_gate = 0.35;
                            settings.restir_gi_reuse_m_cap = 10;
                            settings.restir_gi_reuse_weight_cap = 20.0;
                            settings.restir_gi_jacobian_min = 0.3;
                            settings.restir_gi_jacobian_max = 2.5;
                            actions.reset_history = true;
                        }
                    });
                });

            CollapsingHeader::new("Path Tracing")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(Slider::new(&mut settings.max_bounces, 1..=16).text("Bounces"));
                    ui.add(
                        Slider::new(&mut settings.dda_max_steps, 64..=2048)
                            .logarithmic(true)
                            .text("DDA Max Steps"),
                    );
                    ui.add(
                        Slider::new(&mut settings.max_history, 1.0..=256.0)
                            .logarithmic(true)
                            .text("History Cap"),
                    );
                    ui.checkbox(&mut settings.debug_overlay, "Probe Debug Overlay");
                    let mut overlay_mode =
                        settings.debug_overlay_mode.clamp(0, DEBUG_OVERLAY_MODE_MAX);
                    ComboBox::from_label("Overlay Mode")
                        .selected_text(debug_overlay_mode_label(overlay_mode))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_NONE,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_NONE),
                            );
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_PROBE,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_PROBE),
                            );
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_MOTION,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_MOTION),
                            );
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_HISTORY_VALIDITY,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_HISTORY_VALIDITY),
                            );
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_HISTORY_WEIGHT,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_HISTORY_WEIGHT),
                            );
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_REJECT_REASON,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_REJECT_REASON),
                            );
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_CLAMP_DIFF,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_CLAMP_DIFF),
                            );
                            ui.selectable_value(
                                &mut overlay_mode,
                                DEBUG_OVERLAY_MODE_TEMPORAL_VARIANCE,
                                debug_overlay_mode_label(DEBUG_OVERLAY_MODE_TEMPORAL_VARIANCE),
                            );
                        });
                    settings.debug_overlay_mode = overlay_mode;
                });

            CollapsingHeader::new("Lighting")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(
                        Slider::new(&mut settings.sun_intensity, 0.0..=32.0).text("Sun Intensity"),
                    );
                    ui.add(
                        Slider::new(&mut settings.environment_intensity, 0.0..=4.0)
                            .text("Environment"),
                    );
                    ui.add(Slider::new(&mut settings.exposure, 0.05..=8.0).text("Exposure"));
                    ui.add(
                        Slider::new(&mut settings.sun_yaw_degrees, -180.0..=180.0).text("Sun Yaw"),
                    );
                    ui.add(
                        Slider::new(&mut settings.sun_pitch_degrees, 1.0..=89.0).text("Sun Pitch"),
                    );
                });

            CollapsingHeader::new("ReSTIR")
                .default_open(true)
                .show(ui, |ui| {
                    ui.checkbox(&mut settings.restir_di_enabled, "Enable ReSTIR DI");
                    ui.checkbox(&mut settings.restir_gi_enabled, "Enable ReSTIR GI");
                    ui.add(
                        Slider::new(&mut settings.restir_spatial_radius, 0..=2)
                            .text("Spatial Radius"),
                    );
                    ui.add(
                        Slider::new(&mut settings.restir_temporal_boost, 0.0..=4.0)
                            .text("Temporal Boost"),
                    );
                    ui.add(
                        Slider::new(&mut settings.restir_gi_directional_gate, -0.25..=0.99)
                            .text("GI Direction Gate"),
                    );
                    ui.add(
                        Slider::new(&mut settings.restir_gi_reuse_m_cap, 1..=32)
                            .text("GI Reuse Sample Cap"),
                    );
                    ui.add(
                        Slider::new(&mut settings.restir_gi_reuse_weight_cap, 1.0..=128.0)
                            .logarithmic(true)
                            .text("GI Reuse Weight Cap"),
                    );
                    ui.add(
                        Slider::new(&mut settings.restir_gi_jacobian_min, 0.01..=1.0)
                            .text("GI Jacobian Min"),
                    );
                    ui.add(
                        Slider::new(&mut settings.restir_gi_jacobian_max, 0.01..=16.0)
                            .logarithmic(true)
                            .text("GI Jacobian Max"),
                    );
                    ui.add(
                        Slider::new(&mut settings.rr_start_bounce, 1..=12).text("RR Start Bounce"),
                    );
                    ui.add(Slider::new(&mut settings.rr_min_survival, 0.01..=0.9).text("RR Min"));
                    ui.add(Slider::new(&mut settings.rr_max_survival, 0.1..=0.995).text("RR Max"));
                });

            CollapsingHeader::new("SVGF")
                .default_open(true)
                .show(ui, |ui| {
                    ui.checkbox(&mut settings.svgf_enabled, "Enable SVGF");
                    ui.add(Slider::new(&mut settings.svgf_passes, 0..=5).text("A-Trous Passes"));
                    ui.add(Slider::new(&mut settings.svgf_step_scale, 1..=4).text("Step Scale"));
                    ui.add(
                        Slider::new(&mut settings.svgf_normal_phi, 0.05..=8.0).text("Normal Phi"),
                    );
                    ui.add(
                        Slider::new(&mut settings.svgf_depth_phi, 1.0..=256.0)
                            .logarithmic(true)
                            .text("Depth Phi"),
                    );
                    ui.add(Slider::new(&mut settings.svgf_luma_phi, 0.1..=8.0).text("Luma Phi"));
                    ui.add(
                        Slider::new(&mut settings.svgf_clamp_sigma, 0.0..=8.0).text("Clamp Sigma"),
                    );
                    ui.add(
                        Slider::new(&mut settings.svgf_invalid_variance_boost, 1.0..=12.0)
                            .text("Invalid Variance"),
                    );
                    ui.add(
                        Slider::new(&mut settings.svgf_center_weight, 0.5..=12.0)
                            .text("Center Weight"),
                    );
                    ui.add(
                        Slider::new(&mut settings.svgf_history_normal_reject_cos, 0.5..=0.999)
                            .text("History Normal Reject"),
                    );
                    ui.add(
                        Slider::new(&mut settings.svgf_history_depth_reject_scale, 0.01..=0.5)
                            .logarithmic(true)
                            .text("History Depth Reject"),
                    );
                });

            CollapsingHeader::new("Camera Lens")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(
                        Slider::new(&mut camera_settings.fov_y_degrees, 20.0..=120.0).text("FOV Y"),
                    );
                    ui.add(Slider::new(&mut camera_settings.aperture, 0.0..=0.25).text("Aperture"));
                    ui.add(
                        Slider::new(&mut camera_settings.focus_distance, 0.1..=1000.0)
                            .logarithmic(true)
                            .text("Focus Distance"),
                    );
                    ui.add(
                        Slider::new(&mut camera_settings.depth_adapt, 0.0..=2.0)
                            .text("Depth Adapt"),
                    );
                    ui.add(
                        Slider::new(&mut camera_settings.near, 0.001..=2.0)
                            .logarithmic(true)
                            .text("Near Plane"),
                    );
                    ui.add(
                        Slider::new(&mut camera_settings.far, 10.0..=20000.0)
                            .logarithmic(true)
                            .text("Far Plane"),
                    );
                });

            CollapsingHeader::new("Controls")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(
                        Slider::new(&mut controller_settings.move_speed, 0.5..=80.0)
                            .text("Move Speed"),
                    );
                    ui.add(
                        Slider::new(&mut controller_settings.sprint_multiplier, 1.0..=10.0)
                            .text("Sprint Mult"),
                    );
                    ui.add(
                        Slider::new(&mut controller_settings.mouse_sensitivity, 0.0002..=0.02)
                            .logarithmic(true)
                            .text("Mouse Sens"),
                    );
                    ui.checkbox(&mut controller_settings.invert_y, "Invert Y");
                });

            CollapsingHeader::new("Stats")
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(format!(
                        "Camera Pos [{:.2}, {:.2}, {:.2}]",
                        camera_state.position.x, camera_state.position.y, camera_state.position.z
                    ));
                    ui.label(format!(
                        "Camera Yaw {:.2} | Pitch {:.2}",
                        camera_state.yaw_degrees, camera_state.pitch_degrees
                    ));
                    ui.label(format!(
                        "Chunks {} | Emissive {} | Frame {}",
                        stats.chunk_count, stats.emissive_count, stats.frame_index
                    ));
                    ui.label(format!(
                        "Chunk map avg probe {:.2} | load {:.2}",
                        stats.chunk_map_avg_probe, stats.chunk_map_load_factor
                    ));
                    if stats.chunk_map_dropped_entries > 0 {
                        ui.colored_label(
                            Color32::RED,
                            format!(
                                "Chunk map dropped entries: {}",
                                stats.chunk_map_dropped_entries
                            ),
                        );
                    } else {
                        ui.label("Chunk map dropped entries: 0");
                    }
                    ui.label(format!(
                        "World sync rejects: {}",
                        stats.world_sync_reject_count
                    ));
                    let reject_reason = if stats.last_world_sync_reject_reason.is_empty() {
                        "none"
                    } else {
                        stats.last_world_sync_reject_reason.as_str()
                    };
                    if stats.world_sync_reject_count > 0 {
                        ui.colored_label(
                            Color32::YELLOW,
                            format!("Last sync reject: {reject_reason}"),
                        );
                    } else {
                        ui.label(format!("Last sync reject: {reject_reason}"));
                    }
                    ui.label(format!(
                        "Motion state: {}",
                        if stats.camera_in_motion {
                            "moving"
                        } else {
                            "stable"
                        }
                    ));
                });

            ui.separator();
            ui.small("WASD move | Space up | Shift down | Ctrl sprint | Esc release pointer");
        });
}

fn capture_pointer(window: &winit::window::Window) -> bool {
    let locked = window
        .set_cursor_grab(CursorGrabMode::Locked)
        .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
        .is_ok();
    if locked {
        window.set_cursor_visible(false);
    }
    locked
}

fn release_pointer(window: &winit::window::Window) {
    let _ = window.set_cursor_grab(CursorGrabMode::None);
    window.set_cursor_visible(true);
}

fn debug_overlay_mode_label(mode: u32) -> &'static str {
    match mode {
        DEBUG_OVERLAY_MODE_NONE => "Disabled",
        DEBUG_OVERLAY_MODE_PROBE => "Probe Stats",
        DEBUG_OVERLAY_MODE_MOTION => "Motion Magnitude",
        DEBUG_OVERLAY_MODE_HISTORY_VALIDITY => "History Validity",
        DEBUG_OVERLAY_MODE_HISTORY_WEIGHT => "History Weight",
        DEBUG_OVERLAY_MODE_REJECT_REASON => "Reject Reason",
        DEBUG_OVERLAY_MODE_CLAMP_DIFF => "Clamp Delta",
        DEBUG_OVERLAY_MODE_TEMPORAL_VARIANCE => "Temporal Variance",
        _ => "Probe Stats",
    }
}
