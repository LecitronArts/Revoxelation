# Revoxelation Tech Stack

## Snapshot
- Project type: native desktop Rust voxel runtime with a fixed-stage frame loop and an `ash`/Vulkan renderer.
- Primary crate: `revoxelation` in `Cargo.toml`.
- Rust edition: `2024`.
- Entrypoints: `src/main.rs` calls `revoxelation::app::run`, and `src/app.rs` owns runtime startup.

## Language and Runtime
- Language: Rust across `src/lib.rs`, `src/app.rs`, `src/runtime/**`, `src/streaming/**`, `src/meshing/**`, and `src/renderer/**`.
- Shader language: GLSL sources in `shaders/chunk_mesh.vert`, `shaders/chunk_mesh.frag`, and `shaders/chunk_cull.comp`.
- Shader build flow: `build.rs` compiles GLSL into SPIR-V with `shaderc`, and renderer pipelines load the compiled bytes with `include_bytes!(concat!(env!("OUT_DIR"), ...))`.
- Runtime model: single-process desktop app with a `winit` event loop and a five-stage frame scheduler.

## Build and Dependency Layer
- Build tool: Cargo (`Cargo.toml`, `Cargo.lock`, `build.rs`).
- GPU and platform crates:
  - `ash`, `ash-window`, `gpu-allocator`, `raw-window-handle` for Vulkan instance/device/surface/memory management.
  - `winit` for window creation and OS event dispatch.
- Runtime and data crates:
  - `dashmap`, `rayon`, `noise`, `rand` for chunk state tracking, background generation, and procedural payload production.
  - `serde` and `serde_json` (tests) for runtime command/event serialization checks.
- Utility crates:
  - `anyhow` for fallible setup and bootstrap paths.
  - `bytemuck` for POD GPU-facing structs and byte casting.
  - `glam` for math support where needed.
  - `egui` for UI data structures consumed by the custom Vulkan backend.
  - `log` for runtime stage tracing.
- Dependency note: `hecs` remains declared in `Cargo.toml`, but the current source tree does not wire a live ECS runtime around it.

## Rendering Stack
- Vulkan bootstrap: `src/renderer/instance.rs`, `src/renderer/device.rs`, `src/renderer/swapchain.rs`, and `src/renderer/frame.rs`.
- Renderer owner: `src/renderer/mod.rs` (`Renderer`) holds Vulkan objects, synchronization primitives, allocator state, and optional rendering subsystems.
- Chunk rendering path:
  - slot-backed GPU buffers in `src/renderer/chunk_pool.rs`
  - graphics pipeline in `src/renderer/mesh_pipeline.rs`
  - compute culling pipeline in `src/renderer/cull_pipeline.rs`
  - frame submission and presentation in `src/renderer/mod.rs`
- Upload helpers: `StagingBuffer` plus buffer/image allocation helpers in `src/renderer/mod.rs`.
- UI backend: `src/renderer/egui_backend.rs` manages font texture uploads and scratch GPU buffers for egui meshes.

## Runtime, Streaming, and Meshing Stack
- App orchestration: `src/app.rs` builds the window, creates renderer subsystems, installs global renderer state, and drives redraws.
- Frame scheduler: `src/runtime/scheduler.rs` executes `Input -> Simulation -> WorldUpdate -> MeshSync -> RenderSubmit`.
- Runtime support: `src/runtime/boundaries/**`, `src/runtime/events/**`, `src/runtime/trace.rs`, and `src/runtime/observability/**`.
- Streaming subsystem: `src/streaming/octree.rs`, `src/streaming/sse.rs`, `src/streaming/state_store.rs`, `src/streaming/job_queue.rs`, `src/streaming/job_runner.rs`, and `src/streaming/types.rs`.
- Meshing subsystem: `src/meshing/greedy.rs`, `src/meshing/invalidation.rs`, and `src/meshing/packing.rs`.
- Render bridge: `RenderDelta` values produced by runtime/meshing are drained into the renderer and applied to the chunk pool before draw submission.

## Testing and Quality Signals
- Integration tests live under `tests/`:
  - `tests/phase1_*.rs` for stage order, boundaries, observability, events, and quality gates
  - `tests/phase2_streaming.rs` for world-update/mesh-sync round trips
  - `tests/phase25_vulkan.rs` for Vulkan API compile checks
  - `tests/phase3_meshing.rs` for typed voxel payloads, greedy meshing, slot reuse, and Vulkan feature gating
- Inline unit tests live in `src/streaming/*.rs`, `src/runtime/scheduler.rs`, and `src/runtime/boundaries/*.rs`.

## Not Present in Current Stack
- No Bevy or external game engine integration is present.
- No network server, database client, or web framework is present in the current runtime.
