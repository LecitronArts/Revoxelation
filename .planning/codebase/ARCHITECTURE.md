# Revoxelation Architecture

## 1) System Shape
- The app is a single-process desktop runtime with a `winit` event loop in `src/app.rs`.
- Startup entry is `src/main.rs` (`main` -> `revoxelation::app::run`).
- The live codebase is split into five top-level library concerns declared in `src/lib.rs`: `app`, `runtime`, `streaming`, `meshing`, and `renderer`.
- Rendering is Vulkan-first: `ash` owns instance/device/swapchain setup, chunk visibility is culled by a compute pipeline, and chunk meshes are submitted through indexed indirect draws.

## 2) Primary Entry Points
- Process entry: `src/main.rs`.
- App orchestration entry: `src/app.rs` (`run()` creates the window, derives raw handles, builds renderer subsystems, and starts the event loop).
- Frame scheduler entry: `src/runtime/scheduler.rs` (`run_frame`).
- Renderer constructor: `src/renderer/mod.rs` (`Renderer::new`).

## 3) Layering Model
- Layer A (Platform): `src/app.rs` owns window creation, redraw requests, and OS event routing.
- Layer B (Runtime orchestration): `src/runtime/**` owns stage order, event/command models, domain boundaries, and trace/overlay reporting.
- Layer C (Streaming): `src/streaming/**` owns chunk keys, chunk lifecycle state, octree traversal, SSE decisions, queue prioritization, and background job dispatch.
- Layer D (Meshing): `src/meshing/**` owns dirty tracking, neighbor invalidation, greedy quad generation, and packed vertex/index payloads.
- Layer E (Renderer/Vulkan): `src/renderer/*.rs` owns Vulkan bootstrap, GPU memory, chunk slot buffers, pipelines, and presentation.

## 4) Core Architecture Patterns
- Fixed-stage frame runner: `STAGE_ORDER` in `src/runtime/stages.rs` locks the runtime to `Input -> Simulation -> WorldUpdate -> MeshSync -> RenderSubmit`.
- Domain boundary registry: `src/runtime/boundaries/**` keeps world/meshing/collision/persistence registrations explicit and rejects cross-domain misuse.
- Global runtime state: `src/runtime/scheduler.rs` stores streaming and meshing state in `OnceLock<Mutex<...>>` so the stage runner can operate without per-frame object wiring.
- Global renderer state: `src/renderer/mod.rs` installs the active `Renderer` into a process-wide `OnceLock<Mutex<Renderer>>`.
- Slot-based rendering: `src/renderer/chunk_pool.rs` maps each active `ChunkKey` to a reusable GPU slot holding vertices, indices, metadata, and indirect draw commands.
- Build-time shader compilation: `build.rs` is part of the architecture boundary, not just tooling. It compiles the authoritative GLSL sources into SPIR-V consumed by the Vulkan pipelines.

## 5) Data Flow: Startup to First Frame
1. `src/main.rs` calls `revoxelation::app::run`.
2. `src/app.rs` creates the `winit` event loop and window, then extracts raw display/window handles.
3. `Renderer::new` in `src/renderer/mod.rs` loads the Vulkan entry, creates the instance/surface, selects a physical device, builds a logical device, command pool, allocator, swapchain context, and frame sync primitives.
4. `src/app.rs` then attaches optional renderer subsystems:
   - `ChunkPool::new`
   - `ChunkMeshPipeline::new`
   - `ChunkCullPipeline::new`
   - `EguiAshBackend::new`
5. The renderer is installed globally with `install_renderer`.
6. The event loop requests redraws, and each redraw calls `runtime::run_frame(frame_index)`.

## 6) Data Flow: Streaming to Meshing to Render Deltas
1. `run_frame` enters `WorldUpdate` and asks `diff_active_set` in `src/streaming/sse.rs` which chunks should activate or deactivate.
2. Newly needed chunks are inserted into `ChunkStateStore`, queued in `ChunkJobQueue`, and dispatched through `spawn_chunk_job` in `src/streaming/job_runner.rs`.
3. `MeshSync` drains completed `ChunkJobResult` values from the scheduler receiver.
4. Generated chunk payloads are stored in `MeshingState`, marked dirty, and neighbor invalidation is propagated.
5. Dirty chunks are greedily meshed by `build_greedy_mesh` in `src/meshing/greedy.rs`.
6. Finished meshes are converted into `RenderDelta::Upsert` or `RenderDelta::Remove` items and buffered for render submission.

## 7) Data Flow: Render Submit and Presentation
1. During `RenderSubmit`, runtime drains pending `RenderDelta` items into the global renderer.
2. `submit_frame` in `src/renderer/mod.rs` acquires the next swapchain image, resets per-frame synchronization, and begins command recording.
3. Pending chunk deltas are applied to the slot-backed chunk pool before any draw work.
4. If present, `ChunkCullPipeline` dispatches compute work to update indirect draw visibility and a buffer barrier promotes the results for indirect draw reads.
5. A Vulkan render pass begins over the swapchain image.
6. If present, `ChunkMeshPipeline` binds vertex/index buffers from the chunk pool and issues `cmd_draw_indexed_indirect`.
7. If present, `EguiAshBackend` runs its current paint/upload hook.
8. The command buffer is submitted to the graphics queue and the rendered image is presented on the swapchain.

## 8) Dependency Direction (Practical Rule)
- `app` depends on `runtime` and `renderer`.
- `runtime` depends on `streaming`, `meshing`, and the public renderer API surface.
- `meshing` depends on shared streaming contracts (`ChunkKey`, `ChunkVoxels`) but does not own GPU objects.
- `renderer` depends on meshing output types (`PackedMesh`) and streaming keys (`ChunkKey`) but does not own chunk-selection policy.
- Shader sources in `shaders/` are leaf artifacts compiled by `build.rs` and loaded by renderer pipeline modules.
