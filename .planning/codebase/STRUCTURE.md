# Revoxelation Directory Structure

## 1) Top-Level Layout
- `Cargo.toml`: crate metadata and dependency graph.
- `Cargo.lock`: resolved dependency versions.
- `src/`: all runtime source code.
- `.planning/codebase/`: generated mapping docs (including this file).
- `target/`: Cargo build output.

## 2) Source Tree (Path-Grounded)
- `src/main.rs`: binary module declarations and process entry.
- `src/app.rs`: application loop, input routing, egui control panel, render calls.
- `src/ecs.rs`: camera/input ECS scheduler and camera state projection.
- `src/world/mod.rs`: chunk model, procedural generation, dirty-state signaling.
- `src/renderer/mod.rs`: renderer module boundary and public re-exports.
- `src/shaders/trace.wgsl`: primary tracer compute kernel.
- `src/shaders/reistir.wgsl`: ReSTIR spatial reuse compute kernel.
- `src/shaders/svgf.wgsl`: SVGF denoise/resolve/diagnostics kernels.

## 3) Renderer Subtree Breakdown
- `src/renderer/core/`: orchestration layer.
- `src/renderer/core/bootstrap/`: initialization pipeline (device/resources/layouts/shaders/pipelines).
- `src/renderer/core/frame_exec.rs`: per-frame command encoding and submission.
- `src/renderer/core/frame_plan.rs`: frame slot/pass scheduling decisions.
- `src/renderer/core/world_ops.rs`: world sync, resize, lifecycle application.
- `src/renderer/core/state.rs`: renderer state bags, settings, diagnostics/event ring.
- `src/renderer/core/renderer.rs`: external-facing renderer methods.

- `src/renderer/lifecycle/`: lifecycle decision mechanics.
- `src/renderer/lifecycle/policy.rs`: event policy decisions.
- `src/renderer/lifecycle/plan.rs`: event -> lifecycle plan mapping.
- `src/renderer/lifecycle/executor.rs`: hook-based lifecycle execution.

- `src/renderer/world/`: CPU world-to-GPU pipeline.
- `src/renderer/world/payload_builder.rs`: builds GPU payload from `VoxelWorld`.
- `src/renderer/world/sync.rs`: validates payload and rejection reporting.
- `src/renderer/world/upload.rs`: allocates/upload GPU buffers and textures.

- `src/renderer/resources/`: long-lived GPU resource containers/utilities.
- `src/renderer/resources/context.rs`: world/surface resource ownership + version tracking.
- `src/renderer/resources/surface.rs`: surface-size dependent resources and SVGF uniform helpers.
- `src/renderer/resources/restir_storage.rs`: ReSTIR ping-pong + frame bridge.
- `src/renderer/resources/bind_groups.rs`: trace/SVGF bind-group construction.

- `src/renderer/protocol/`: shader ABI contract.
- `src/renderer/protocol/types.rs`: POD structs shared with WGSL layouts.
- `src/renderer/protocol/bindings.rs`: binding slot constants and ordering.
- `src/renderer/protocol/mod.rs`: history-slot helpers and protocol exports.

- `src/renderer/passes/`: pass-level dispatch units.
- `src/renderer/passes/trace.rs`: trace pass implementation.
- `src/renderer/passes/reistir.rs`: ReSTIR pass implementation.
- `src/renderer/passes/svgf.rs`: SVGF sequence implementation.
- `src/renderer/passes/mod.rs`: pass traits and dispatch grid utility.

- `src/renderer/camera.rs`: CPU camera -> `CameraGpu` projection.
- `src/renderer/light_sampler.rs`: emissive CDF + importance map + remap utilities.
- `src/renderer/reservoir.rs`: CPU-side reservoir sampling helpers.

## 4) Structural Boundaries and Responsibilities
- UI/event code is isolated to `src/app.rs`; it does not own low-level GPU objects directly.
- Simulation controls are isolated in `src/ecs.rs`; renderer reads a `PhysicalCamera` snapshot.
- World generation/storage is isolated in `src/world/mod.rs`; renderer consumes snapshots via sync APIs.
- GPU object ownership is centralized under `Renderer` (`src/renderer/core/state.rs`) and `RendererResourceContext` (`src/renderer/resources/context.rs`).
- Contract-sensitive data layout lives in `src/renderer/protocol/*` to avoid duplicated binding/index definitions.

## 5) Practical Navigation Recipes
- To trace startup: `src/main.rs` -> `src/app.rs` -> `src/renderer/core/renderer.rs` -> `src/renderer/core/bootstrap/mod.rs`.
- To inspect world sync failures: `src/renderer/core/world_ops.rs` + `src/renderer/world/sync.rs`.
- To inspect frame scheduling: `src/renderer/core/frame_plan.rs` then `src/renderer/core/frame_exec.rs`.
- To inspect binding mismatches: `src/renderer/protocol/bindings.rs` + `src/renderer/core/bootstrap/pipeline_layouts.rs` + `src/renderer/resources/bind_groups.rs`.
- To inspect temporal resource behavior: `src/renderer/resources/restir_storage.rs` + history helpers in `src/renderer/protocol/mod.rs`.

## 6) Test Placement Structure
- Tests are colocated with implementation using `#[cfg(test)]` across `src/world/mod.rs` and many `src/renderer/**` files.
- Protocol consistency tests are concentrated in `src/renderer/protocol/mod.rs`, `src/renderer/protocol/bindings.rs`, and `src/renderer/core/bootstrap/pipeline_layouts.rs`.
- Lifecycle behavior tests are concentrated in `src/renderer/lifecycle/*.rs` and `src/renderer/core/world_ops.rs`.
