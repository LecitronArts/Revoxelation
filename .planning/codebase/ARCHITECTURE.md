# Revoxelation Architecture

## 1) System Shape
- The app is a single-process desktop renderer with a real-time event loop in `src/app.rs`.
- Startup entry is `src/main.rs` (`main` -> `app::run`).
- The runtime is split into four top-level concerns declared in `src/main.rs`: `app`, `ecs`, `renderer`, `world`.
- Rendering is compute-first (`wgpu` compute passes), with UI composited afterward via egui in `src/renderer/core/frame_exec.rs`.

## 2) Primary Entry Points
- Process entry: `src/main.rs`.
- App orchestration entry: `src/app.rs` (`run()` creates window, world, logic scheduler, renderer).
- Renderer constructor: `src/renderer/core/renderer.rs` (`Renderer::new`) delegated to bootstrap in `src/renderer/core/bootstrap/mod.rs`.
- World generation kickoff: `src/world/mod.rs` (`VoxelWorld::spawn_generation`).

## 3) Layering Model
- Layer A (Platform/UI): window/input/event loop + egui controls in `src/app.rs`.
- Layer B (Simulation/Control): camera/input ECS logic in `src/ecs.rs`.
- Layer C (World Data): chunk storage + async procedural generation in `src/world/mod.rs`.
- Layer D (Renderer Orchestration): frame execution + lifecycle + world sync in `src/renderer/core/*.rs`.
- Layer E (GPU Contracts/Resources): protocol structs and bind-group contracts in `src/renderer/protocol/*.rs` and `src/renderer/resources/*.rs`.
- Layer F (GPU Programs): WGSL kernels in `src/shaders/trace.wgsl`, `src/shaders/reistir.wgsl`, `src/shaders/svgf.wgsl`.

## 4) Core Architecture Patterns
- Orchestrator + service modules: `src/renderer/core/world_ops.rs` delegates to specialized modules (`world/sync`, `world/upload`, `lifecycle/*`).
- Policy -> Plan -> Executor lifecycle pipeline:
- Policy: `src/renderer/lifecycle/policy.rs`
- Plan: `src/renderer/lifecycle/plan.rs`
- Execution hooks: `src/renderer/lifecycle/executor.rs`
- Protocol-driven binding contracts: binding indices are centralized in `src/renderer/protocol/bindings.rs` and consumed by pipeline layouts/bind-group builders in `src/renderer/core/bootstrap/pipeline_layouts.rs` and `src/renderer/resources/bind_groups.rs`.
- Ping-pong temporal resources: frame history slots and ReSTIR buffers in `src/renderer/resources/restir_storage.rs` and helpers in `src/renderer/protocol/mod.rs`.
- Versioned resource dependencies: `ResourceVersionState` in `src/renderer/resources/context.rs` tracks world/surface generations.

## 5) Data Flow: Startup to First Frame
1. `src/main.rs` initializes logging and calls `app::run`.
2. `src/app.rs` creates `VoxelWorld`, starts generation, creates `LogicScheduler`, and blocks on `Renderer::new`.
3. `Renderer::new` in `src/renderer/core/renderer.rs` calls bootstrap in `src/renderer/core/bootstrap/mod.rs`.
4. Bootstrap stages:
- GPU/device/surface setup in `src/renderer/core/bootstrap/device_setup.rs`.
- Initial resources/uniforms in `src/renderer/core/bootstrap/resource_setup.rs`.
- Pipeline layouts/shaders/pipelines in `src/renderer/core/bootstrap/pipeline_setup.rs`.
- Initial bind groups from resource context in `src/renderer/resources/bind_groups.rs`.
5. Bootstrap ends by calling `sync_world` once (`src/renderer/core/bootstrap/mod.rs`).

## 6) Data Flow: World Sync Pipeline
1. Triggered from `src/app.rs` when `world.take_dirty()` is true.
2. `Renderer::sync_world` -> `src/renderer/core/world_ops.rs`.
3. Payload build: `prepare_world_sync` in `src/renderer/world/sync.rs` calls `build_payload` in `src/renderer/world/payload_builder.rs`.
4. Validation gate: storage-size checks in `validate_world_sync_payload` (`src/renderer/world/sync.rs`).
5. Upload plan + GPU resource creation in `src/renderer/world/upload.rs` (`prepare_world_upload`, `execute_world_upload`).
6. Runtime metadata update and resource swap in `apply_world_upload` (`src/renderer/core/world_ops.rs`) and `RendererResourceContext::apply_world_upload` (`src/renderer/resources/context.rs`).
7. Lifecycle event emitted (`SyncSucceeded` or `SyncRejected`) and executed by `src/renderer/lifecycle/executor.rs`.

## 7) Data Flow: Per-Frame Render Pipeline
1. `src/app.rs` gathers camera + UI settings and calls `Renderer::render`.
2. `src/renderer/core/frame_exec.rs` builds `FramePlan` (`src/renderer/core/frame_plan.rs`) and `FrameContext`.
3. CPU writes `CameraGpu` + `TracerUniform` + SVGF uniforms to GPU buffers (`src/renderer/core/frame_exec.rs`).
4. Compute passes are recorded in order:
- Trace pass (`src/renderer/passes/trace.rs`)
- ReSTIR pass (`src/renderer/passes/reistir.rs`)
- SVGF init/atrous/resolve (`src/renderer/passes/svgf.rs`)
5. Output texture is copied to the swapchain texture, then egui is rendered over it in `src/renderer/core/frame_exec.rs`.
6. Frame bridge advances and diagnostics events are pushed to the ring in `src/renderer/core/state.rs`.

## 8) Dependency Direction (Practical Rule)
- `app` depends on `ecs`, `world`, `renderer` (`src/app.rs`).
- `renderer/core` orchestrates but relies on `renderer/world`, `renderer/resources`, `renderer/lifecycle`, `renderer/passes`.
- `renderer/protocol` is a low-level shared contract layer consumed by both bootstrap/layout and runtime upload/bind-group code.
- WGSL shaders are leaf artifacts loaded by bootstrap (`src/renderer/core/bootstrap/shader_modules.rs`).
