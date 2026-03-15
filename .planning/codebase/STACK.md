# Revoxelation Tech Stack

## Snapshot
- Project type: native desktop Rust application focused on GPU compute voxel path tracing.
- Primary crate: `revoxelation` in `Cargo.toml`.
- Rust edition: `2024` in `Cargo.toml`.
- Entrypoint: `src/main.rs` initializes logging and delegates to `app::run`.

## Language and Runtime
- Language: Rust (`src/main.rs`, `src/app.rs`, `src/renderer/**`, `src/world/mod.rs`).
- Shader language: WGSL in `src/shaders/trace.wgsl`, `src/shaders/reistir.wgsl`, `src/shaders/svgf.wgsl`.
- Runtime model: single-process desktop app with event loop in `src/app.rs`.
- Async bootstrap is resolved synchronously via `pollster::block_on` in `src/app.rs`.

## Build and Dependency Layer
- Build tool: Cargo (`Cargo.toml`, `Cargo.lock`).
- Core crates declared in `Cargo.toml`:
- `wgpu` + `winit` for GPU and window/event integration (`src/renderer/core/bootstrap/device_setup.rs`, `src/app.rs`).
- `egui`, `egui-wgpu`, `egui-winit` for in-app debug/control UI (`src/app.rs`, `src/renderer/core/bootstrap/mod.rs`, `src/renderer/core/frame_exec.rs`).
- `hecs` for ECS-style camera logic (`src/ecs.rs`).
- `glam` for math vectors and camera transforms (`src/ecs.rs`, `src/renderer/camera.rs`, `src/renderer/core/frame_exec.rs`).
- `dashmap`, `rayon`, `noise`, `rand` for concurrent procedural world generation (`src/world/mod.rs`).
- `bytemuck` for safe byte casting/POD layout over GPU buffers (`src/renderer/protocol/types.rs`, `src/renderer/world/upload.rs`).
- `anyhow` for fallible app/renderer setup (`src/app.rs`, `src/renderer/core/bootstrap/device_setup.rs`).
- `log` + `env_logger` for diagnostics (`src/main.rs`, `src/app.rs`, `src/renderer/core/world_ops.rs`).

## Rendering Stack
- GPU API abstraction: `wgpu` device/surface/pipeline lifecycle in `src/renderer/core/bootstrap/device_setup.rs`.
- Compute pipelines are built in `src/renderer/core/bootstrap/compute_pipelines.rs`.
- Pipeline layouts come from protocol binding constants in `src/renderer/core/bootstrap/pipeline_layouts.rs`.
- Shader modules are created from WGSL sources in `src/renderer/core/bootstrap/shader_modules.rs`.
- Per-frame command recording and dispatch occur in `src/renderer/core/frame_exec.rs`.
- Pass modules are split into trace/ReSTIR/SVGF in `src/renderer/passes/trace.rs`, `src/renderer/passes/reistir.rs`, `src/renderer/passes/svgf.rs`.

## World/Data Stack
- World storage: chunk map in `DashMap<ChunkCoord, Arc<Chunk>>` at `src/world/mod.rs`.
- World generation worker uses `std::thread::spawn` + Rayon parallel iteration in `src/world/mod.rs`.
- Noise-based terrain/cave synthesis uses `OpenSimplex` in `src/world/mod.rs`.
- CPU world payload construction happens in `src/renderer/world/payload_builder.rs`.
- Upload planning and GPU buffer/texture creation happen in `src/renderer/world/upload.rs`.

## UI and Input Stack
- Window/event handling: `winit` in `src/app.rs`.
- UI state and controls: `egui` panel in `src/app.rs`.
- Egui platform bridge: `egui_winit::State` in `src/app.rs`.
- Egui GPU rendering: `egui_wgpu::Renderer` setup in `src/renderer/core/bootstrap/mod.rs` and render usage in `src/renderer/core/frame_exec.rs`.

## Protocol and Memory Layout Stack
- Shared CPU-side protocol structs live in `src/renderer/protocol/types.rs`.
- Binding slot constants live in `src/renderer/protocol/bindings.rs`.
- Frame history slot helpers live in `src/renderer/protocol/mod.rs`.
- Bind group construction that enforces protocol order is in `src/renderer/resources/bind_groups.rs`.

## Testing and Quality Signals
- The codebase uses inline module tests (`#[cfg(test)]`) heavily in:
- `src/world/mod.rs`
- `src/renderer/core/renderer.rs`
- `src/renderer/core/world_ops.rs`
- `src/renderer/core/frame_exec.rs`
- `src/renderer/protocol/mod.rs`
- `src/renderer/core/bootstrap/pipeline_layouts.rs`

## Not Present in Current Stack
- No web framework/server runtime found in `src/**`.
- No external database client crates in `Cargo.toml`.
- No HTTP client/server crate usage found in `src/**`.
