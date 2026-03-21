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
    install_renderer, mesh_pipeline::ChunkMeshPipeline,
};

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

    let mut frame_index = 0_u64;
    event_loop
        .run(move |event, elwt| match event {
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
                WindowEvent::CloseRequested => elwt.exit(),
                WindowEvent::RedrawRequested => {
                    let _ = crate::runtime::run_frame(frame_index);
                    frame_index = frame_index.saturating_add(1);
                }
                _ => {}
            },
            _ => {}
        })
        .map_err(|err| anyhow!("winit event loop failed: {err}"))
}
