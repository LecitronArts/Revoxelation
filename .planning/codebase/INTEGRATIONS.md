# Revoxelation Integrations

## Integration Overview
- This project integrates local subsystems (windowing, GPU compute, UI overlay, world generation, and CPU/GPU data protocols).
- There is no external network API, SaaS, or database integration in current source files.
- Most integrations are in-process boundaries between modules in `src/app.rs`, `src/renderer/**`, and `src/world/mod.rs`.

## 1) Windowing/Event Loop <-> App Logic
- Provider: `winit`.
- Event loop is created in `src/app.rs` with `EventLoop::new`.
- Window creation uses `WindowBuilder::new` in `src/app.rs`.
- Device and input events are handled in the event loop match in `src/app.rs`.
- Pointer capture/release bridges app state and OS cursor APIs via `capture_pointer` and `release_pointer` in `src/app.rs`.
- Integration outcome: realtime controls feed camera/controller state managed by `LogicScheduler` in `src/ecs.rs`.

## 2) App <-> Renderer Bootstrap
- Integration call: `Renderer::new(window.clone(), world.clone())` in `src/app.rs`.
- `pollster::block_on` bridges async renderer bootstrap into the synchronous event-loop setup in `src/app.rs`.
- Renderer bootstrap fan-in lives in `src/renderer/core/bootstrap/mod.rs`.
- Device/surface initialization is performed in `src/renderer/core/bootstrap/device_setup.rs`.

## 3) Renderer <-> GPU (wgpu)
- `wgpu::Instance` and `create_surface` integration in `src/renderer/core/bootstrap/device_setup.rs`.
- Adapter/device negotiation (`request_adapter`, `request_device`) in the same file.
- Surface configuration and present mode selection (`Mailbox` fallback to `Fifo`) in `src/renderer/core/bootstrap/device_setup.rs`.
- Compute pipeline creation integrates pipeline layouts with shader modules in:
- `src/renderer/core/bootstrap/pipeline_layouts.rs`
- `src/renderer/core/bootstrap/shader_modules.rs`
- `src/renderer/core/bootstrap/compute_pipelines.rs`
- Frame command encoding/submission and surface present integration happen in `src/renderer/core/frame_exec.rs`.

## 4) Shader Assets <-> Rust Runtime
- WGSL files are source-integrated at compile time via `include_str!` in `src/renderer/core/bootstrap/shader_modules.rs`.
- Shader files:
- `src/shaders/trace.wgsl`
- `src/shaders/reistir.wgsl`
- `src/shaders/svgf.wgsl`
- Runtime token replacement (`__TRACE_STORAGE_FORMAT__`, `__SVGF_STORAGE_FORMAT__`) links chosen surface format to shader source in `src/renderer/core/bootstrap/shader_modules.rs`.

## 5) Egui UI <-> Winit <-> Wgpu
- UI definition and controls are built in `src/app.rs`.
- Input translation from window events to egui uses `egui_winit::State` in `src/app.rs`.
- GPU renderer is created through `egui_wgpu::Renderer::new` in `src/renderer/core/bootstrap/mod.rs`.
- Per-frame texture uploads/buffer updates/render pass are integrated in `src/renderer/core/frame_exec.rs`.
- Integration outcome: debug/control panel overlays on top of compute-rendered output texture.

## 6) World Generation <-> Renderer World Sync
- World generation subsystem: `VoxelWorld` in `src/world/mod.rs`.
- Integration trigger: app checks `world.take_dirty()` and calls `renderer.sync_world(&world)` in `src/app.rs`.
- Sync planning and validation (including max storage binding checks) are in `src/renderer/world/sync.rs`.
- GPU payload building is in `src/renderer/world/payload_builder.rs`.
- GPU resource upload (`create_buffer_init`, 3D importance texture upload) is in `src/renderer/world/upload.rs`.
- Renderer state application after upload flows through `src/renderer/core/world_ops.rs`.

## 7) CPU Protocol <-> GPU Bindings Contract
- Shared struct layouts are defined in `src/renderer/protocol/types.rs` (`#[repr(C, align(16))]`, `bytemuck::Pod`).
- Binding indices are centralized in `src/renderer/protocol/bindings.rs`.
- Bind group construction consumes those constants in `src/renderer/resources/bind_groups.rs`.
- Layout generation also consumes the same constants in `src/renderer/core/bootstrap/pipeline_layouts.rs`.
- Integration outcome: layout/binding drift is guarded by tests in both protocol and bootstrap modules.

## 8) Renderer Lifecycle Integrations
- Lifecycle planning/execution modules coordinate resize/reconfigure/sync transitions:
- `src/renderer/lifecycle/plan.rs`
- `src/renderer/lifecycle/executor.rs`
- Integration entry points are called from `src/renderer/core/world_ops.rs`.
- App-side triggers include window resize and surface errors in `src/app.rs`.

## 9) Observability and Error Integration
- Logging initialization starts in `src/main.rs` (`env_logger::init`).
- Runtime logs and warnings are emitted from `src/app.rs`, `src/renderer/core/world_ops.rs`, and `src/renderer/world/payload_builder.rs`.
- Fallible integration points use `anyhow::Result` in setup/bootstrap paths (`src/app.rs`, `src/renderer/core/bootstrap/device_setup.rs`).

## 10) External Integrations Status
- No HTTP/webhook integrations detected in `src/**`.
- No database connector integration detected in `Cargo.toml` or `src/**`.
- No auth provider integration detected in current code paths.
