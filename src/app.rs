use anyhow::{Context, Result, anyhow};
use ash::vk;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::{
    event::{Event, WindowEvent},
    event_loop::EventLoop,
    window::WindowBuilder,
};

use crate::renderer::{
    Renderer, chunk_pool::ChunkPool, cull_pipeline::ChunkCullPipeline, egui_backend::EguiAshBackend,
    install_renderer, mesh_pipeline::ChunkMeshPipeline, renderer_state,
};

/// Pending egui output to be consumed by submit_frame.
pub struct PendingEguiOutput {
    pub textures_delta: egui::TexturesDelta,
    pub clipped_primitives: Vec<egui::ClippedPrimitive>,
    pub screen_size: [f32; 2],
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
    install_renderer(renderer)?;

    let egui_ctx = egui::Context::default();
    let mut frame_index = 0_u64;

    event_loop
        .run(move |event, elwt| match event {
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::RedrawRequested => {
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

                    let full_output = egui_ctx.run(raw_input, |ctx| {
                        egui::Window::new("Debug").show(ctx, |ui| {
                            ui.label(format!("Frame: {}", frame_index));
                        });
                    });

                    let clipped_primitives =
                        egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
                    let textures_delta = full_output.textures_delta;

                    // Store egui output for submit_frame to consume.
                    if let Some(state) = renderer_state() {
                        if let Ok(mut renderer) = state.lock() {
                            renderer.pending_egui_output = Some(PendingEguiOutput {
                                textures_delta,
                                clipped_primitives,
                                screen_size,
                            });
                        }
                    }

                    let _ = crate::runtime::run_frame(frame_index);
                    frame_index = frame_index.saturating_add(1);
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|err| anyhow!("winit event loop failed: {err}"))
}
