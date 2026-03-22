# Revoxelation Directory Structure

## 1) Top-Level Layout
- `Cargo.toml`: crate metadata and dependency graph.
- `Cargo.lock`: resolved dependency versions.
- `build.rs`: GLSL-to-SPIR-V build step for Vulkan shaders.
- `src/`: runtime source code exported by `src/lib.rs`.
- `shaders/`: Vulkan shader sources compiled at build time.
- `tests/`: integration and regression tests organized by phase.
- `.planning/codebase/`: generated codebase mapping docs.
- `target/`: Cargo build output.

## 2) Source Tree (Path-Grounded)
- `src/lib.rs`: library root exporting `app`, `runtime`, `streaming`, `meshing`, and `renderer`.
- `src/main.rs`: process entry; forwards startup to `revoxelation::app::run`.
- `src/app.rs`: `winit` app shell, raw-handle extraction, renderer installation, and redraw loop.
- `src/runtime/`: stage runner, domain boundaries, events, observability, and trace helpers.
- `src/streaming/`: chunk lifecycle contracts, octree traversal, SSE logic, job queue, and background job runner.
- `src/meshing/`: dirty tracking, greedy meshing, packed mesh formats.
- `src/renderer/`: Vulkan bootstrap, memory helpers, pipelines, chunk slot pool, egui backend, and frame submission.

## 3) Runtime Subtree Breakdown
- `src/runtime/mod.rs`: runtime public surface and re-exports.
- `src/runtime/stages.rs`: stage enum and canonical `STAGE_ORDER`.
- `src/runtime/scheduler.rs`: global runtime state plus `run_frame`.
- `src/runtime/trace.rs`: stage transition trace records.
- `src/runtime/observability/hud.rs`: HUD-friendly overlay summary types.
- `src/runtime/events/`: runtime commands, events, event bus, sequencing, and validation.
- `src/runtime/boundaries/`: domain registration boundaries for world, meshing, collision, and persistence.
- `src/runtime/systems/placeholders.rs`: placeholder systems used by boundary/quality-gate tests.

## 4) Streaming and Meshing Breakdown
- `src/streaming/types.rs`: canonical chunk keys, voxel payload type, lifecycle states, LOD config, and job result types.
- `src/streaming/state_store.rs`: `DashMap`-backed chunk state registry.
- `src/streaming/octree.rs`: octree layout and active-set traversal helpers.
- `src/streaming/sse.rs`: screen-space-error selection logic.
- `src/streaming/job_queue.rs`: bounded priority queue and cancellation handling.
- `src/streaming/job_runner.rs`: rayon-backed chunk job execution.
- `src/meshing/invalidation.rs`: dirty records, face-neighbor invalidation, and finer-neighbor mask tracking.
- `src/meshing/greedy.rs`: neighbor-aware greedy quad extraction and skirt generation.
- `src/meshing/packing.rs`: packed vertex/index encoding helpers.

## 5) Renderer Breakdown
- `src/renderer/mod.rs`: `Renderer`, `RenderDelta`, `StagingBuffer`, global renderer state, and frame submission.
- `src/renderer/instance.rs`: Vulkan instance and debug messenger setup.
- `src/renderer/device.rs`: physical-device selection, feature/extension gating, logical device creation.
- `src/renderer/swapchain.rs`: swapchain, image views, render pass, and framebuffer setup.
- `src/renderer/frame.rs`: per-frame command buffer, semaphore, and fence allocation.
- `src/renderer/chunk_pool.rs`: fixed-capacity GPU slot allocator and chunk draw buffers.
- `src/renderer/mesh_pipeline.rs`: chunk mesh graphics pipeline and indirect draw call.
- `src/renderer/cull_pipeline.rs`: compute culling pipeline.
- `src/renderer/egui_backend.rs`: custom egui upload/backend plumbing on Vulkan resources.

## 6) Shader and Test Assets
- `shaders/chunk_mesh.vert`: chunk mesh vertex shader.
- `shaders/chunk_mesh.frag`: chunk mesh fragment shader.
- `shaders/chunk_cull.comp`: compute shader for chunk visibility/cull work.
- `tests/phase1_events.rs`: serde/event contract tests.
- `tests/phase1_stage_order.rs`: stage order and stage metadata tests.
- `tests/phase1_registration_boundaries.rs`: domain-boundary registration tests.
- `tests/phase1_observability.rs`: trace and overlay observability tests.
- `tests/phase1_quality_gates.rs`: quality-gate smoke tests.
- `tests/phase2_streaming.rs`: streaming round-trip tests over `run_frame`.
- `tests/phase25_vulkan.rs`: compile-check tests for Vulkan-facing renderer types.
- `tests/phase3_meshing.rs`: typed voxel, greedy mesh, and renderer slot-pool regression tests.

## 7) Structural Boundaries and Responsibilities
- `src/app.rs` owns OS/window concerns and should remain the only place that talks directly to `winit`.
- `src/runtime/**` owns orchestration and policy, but not raw Vulkan objects.
- `src/streaming/**` owns chunk selection and async generation state, but not mesh packing or GPU uploads.
- `src/meshing/**` owns geometry derivation from typed voxel payloads, but not scheduling policy or swapchain details.
- `src/renderer/*.rs` owns GPU resources and draw submission, but not chunk activation policy.

## 8) Practical Navigation Recipes
- To trace startup: `src/main.rs` -> `src/app.rs` -> `src/renderer/mod.rs::Renderer::new`.
- To trace one frame: `src/app.rs` -> `src/runtime/scheduler.rs::run_frame` -> `src/renderer/mod.rs::submit_frame`.
- To inspect chunk activation policy: `src/runtime/scheduler.rs` + `src/streaming/sse.rs` + `src/streaming/octree.rs`.
- To inspect mesh generation: `src/runtime/scheduler.rs` + `src/meshing/invalidation.rs` + `src/meshing/greedy.rs`.
- To inspect draw-buffer layout and slot reuse: `src/renderer/chunk_pool.rs` + `tests/phase3_meshing.rs`.
