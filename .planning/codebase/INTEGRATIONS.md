# Revoxelation Integrations

## Integration Overview
- This project currently integrates only local subsystems: windowing, Vulkan GPU setup, runtime scheduling, chunk streaming, meshing, and serialized event models.
- There is no external network API, SaaS integration, or database integration in the live source tree.
- Most boundaries are in-process integrations between `src/app.rs`, `src/runtime/**`, `src/streaming/**`, `src/meshing/**`, and `src/renderer/*.rs`.

## 1) Windowing/Event Loop <-> App Logic
- Provider: `winit`.
- Event loop creation: `EventLoop::new` in `src/app.rs`.
- Window creation: `WindowBuilder::new` in `src/app.rs`.
- OS redraw flow: `Event::AboutToWait` requests a redraw, and `WindowEvent::RedrawRequested` advances one runtime frame.
- Raw-handle bridge: `raw-window-handle` traits extract display/window handles for Vulkan surface creation.

## 2) App <-> Renderer Bootstrap
- Integration call: `Renderer::new(display_handle, window_handle, extent)` in `src/app.rs`.
- Renderer subsystem wiring is done immediately afterward in `src/app.rs`:
  - `ChunkPool::new`
  - `ChunkMeshPipeline::new`
  - `ChunkCullPipeline::new`
  - `EguiAshBackend::new`
- Renderer ownership is then moved into global process state through `install_renderer`.

## 3) Renderer <-> Vulkan (`ash`)
- Instance bootstrap: `src/renderer/instance.rs`.
- Surface creation: `ash_window::create_surface(...)` in `src/renderer/mod.rs`.
- Physical-device selection and required feature gate: `src/renderer/device.rs`.
- Swapchain/image-view/render-pass/framebuffer creation: `src/renderer/swapchain.rs`.
- Command-buffer and sync primitive allocation: `src/renderer/frame.rs` and `src/renderer/mod.rs`.
- Queue submission and presentation: `submit_frame` in `src/renderer/mod.rs`.

## 4) Build System <-> Shader Assets
- Shader sources live in `shaders/chunk_mesh.vert`, `shaders/chunk_mesh.frag`, and `shaders/chunk_cull.comp`.
- `build.rs` compiles those sources to SPIR-V with `shaderc`.
- `src/renderer/mesh_pipeline.rs` and `src/renderer/cull_pipeline.rs` consume the compiled SPIR-V through `include_bytes!`.
- Runtime and build-time shader source lists stay aligned through `renderer::shader_source_files()` and `build.rs`.

## 5) Runtime Scheduler <-> Streaming
- `run_frame` in `src/runtime/scheduler.rs` drives the `WorldUpdate` stage.
- Active-set decisions come from `diff_active_set` in `src/streaming/sse.rs`.
- Background generation jobs are queued through `ChunkJobQueue` and spawned via `spawn_chunk_job` in `src/streaming/job_runner.rs`.
- Job results flow back over `std::sync::mpsc` into `MeshSync`.

## 6) Streaming/Meshing <-> Renderer
- `ChunkJobOutcome::Generated` stores typed `ChunkVoxels` into `MeshingState`.
- `build_greedy_mesh` in `src/meshing/greedy.rs` turns dirty chunk payloads plus halo neighbors into `PackedMesh`.
- Scheduler converts finished meshes into `RenderDelta::Upsert` / `RenderDelta::Remove`.
- `RenderSubmit` drains those deltas into `Renderer::enqueue_chunk_delta`, and `submit_frame` applies them to the slot-backed chunk pool before drawing.

## 7) Egui Data <-> Custom Vulkan Backend
- App-side backend creation happens in `src/app.rs`.
- Backend implementation lives in `src/renderer/egui_backend.rs`.
- Font texture uploads use `StagingBuffer::copy_to_image`.
- Scratch mesh uploads allocate temporary Vulkan buffers through the same renderer allocation helpers.
- Current integration scope is backend plumbing; the app currently passes empty `TexturesDelta` and primitive lists on frame submit.

## 8) Runtime Commands/Events <-> Serialization
- Runtime command models live in `src/runtime/events/command.rs`.
- Runtime event models live in `src/runtime/events/event.rs`.
- Sequence metadata lives in `src/runtime/events/sequence.rs`.
- `serde` derives and tagged enums provide the serialization contract, and `tests/phase1_events.rs` verifies round-trip stability.

## 9) Observability Integration
- Stage begin/end tracing is emitted with `log::info!` from `src/runtime/scheduler.rs`.
- `RuntimeHudOverlay` in `src/runtime/observability/hud.rs` summarizes frame-stage execution for UI/debug consumption.
- `FrameExecution` snapshots package stage order, trace entries, overlay state, and event-bus snapshots for tests and diagnostics.

## 10) External Integrations Status
- No HTTP or websocket integrations detected in `src/**`.
- No database client integration detected in `Cargo.toml` or `src/**`.
- No auth provider integration detected in current runtime code paths.
