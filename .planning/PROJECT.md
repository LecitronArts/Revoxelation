# Revoxelation

## What This Is

Revoxelation is a Rust voxel runtime project built around a custom staged architecture rather than a game-engine framework.  
The current codebase already contains a live `ash`/Vulkan renderer bootstrap, chunk streaming state machine, greedy meshing path, and render-delta bridge; the active scope is to turn that foundation into a modular, editable, flyable voxel-world core.

## Core Value

Build a cleanly extensible Rust voxel runtime where world streaming, meshing, and future block edits remain predictable, testable, and fast to iterate on.

## Requirements

### Validated

- [x] Rust desktop application scaffold with `winit` startup and redraw loop exists (`src/main.rs`, `src/app.rs`)
- [x] `ash`/Vulkan renderer bootstrap exists with instance/device/surface/swapchain setup (`src/renderer/mod.rs`, `src/renderer/instance.rs`, `src/renderer/device.rs`, `src/renderer/swapchain.rs`)
- [x] GPU memory and frame primitives are in place through `gpu-allocator`, command pools, fences, and semaphores (`src/renderer/mod.rs`, `src/renderer/frame.rs`)
- [x] Chunk rendering path exists with slot-backed buffers, compute culling, mesh pipeline, and indirect draw submission (`src/renderer/chunk_pool.rs`, `src/renderer/cull_pipeline.rs`, `src/renderer/mesh_pipeline.rs`)
- [x] Chunk streaming lifecycle and bounded background job execution exist (`src/runtime/scheduler.rs`, `src/streaming/**`)
- [x] Greedy meshing, border invalidation, and packed mesh formats exist (`src/meshing/**`)
- [x] Runtime command/event contracts and stage-trace observability exist (`src/runtime/events/**`, `src/runtime/trace.rs`, `src/runtime/observability/**`)

### Active

- [ ] Player movement and collision modes suitable for gameplay prototyping
- [ ] Real block placement/destruction with near-immediate visual feedback
- [ ] Chunk persistence (save/load) so modified world state survives restart
- [ ] Better renderer lifecycle handling for resize/reconfigure/error surfacing
- [ ] Stronger UI/debug tooling on top of the current egui backend scaffolding
- [ ] Network-ready interfaces and deterministic contracts only (no multiplayer implementation in v1)

### Out of Scope

- Full multiplayer synchronization and replication; v1 only preserves interfaces
- Migration to Bevy ECS or a third-party engine stack
- Mobile/Web target support in the current milestone
- Deep performance tuning before architecture and gameplay loops stabilize

## Context

The repository is now a brownfield Rust codebase with meaningful runtime infrastructure already present: a fixed five-stage frame runner, typed chunk payloads, background streaming jobs, and a Vulkan renderer that can cull and draw chunk meshes through indirect commands.  
The main remaining work is not "pick a renderer" anymore; it is finishing the gameplay/runtime layers that sit on top of the renderer and tightening the runtime lifecycle around the current Vulkan path.

Quality and delivery process must still use the Superpowers quality gates during subsequent phases, including planning before multi-step work, systematic debugging when blocked, verification before completion claims, and code-review gates before integration closure.

## Constraints

- **Engine Direction**: Rust custom runtime boundaries; do not collapse into Bevy or another engine framework
- **Rendering**: `ash`/Vulkan is the current and intended v1 renderer backend
- **Platforms**: Windows + Linux desktop first
- **Runtime Architecture**: fixed stage runner (`Input`, `Simulation`, `WorldUpdate`, `MeshSync`, `RenderSubmit`)
- **Heavy Work Placement**: chunk generation runs through background job queues; renderer consumes queued deltas on frame submit
- **Geometry Pipeline**: typed `ChunkVoxels` -> greedy meshing -> packed mesh -> slot-backed indirect draw
- **Shader Workflow**: authoritative GLSL sources compile to SPIR-V in `build.rs`
- **Performance Strategy**: correctness and stable architecture first; optimize once the core loop is complete
- **Quality Workflow**: Superpowers skills/gates remain mandatory

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Custom Rust runtime boundaries instead of Bevy | Preserve control over voxel-specific frame stages, streaming, and renderer integration | Active |
| `ash`/Vulkan as renderer backend | Current source already depends on raw Vulkan setup, feature gating, allocator-backed memory, and indirect draw flow | Active |
| Fixed five-stage scheduler | Keeps streaming/meshing/render handoff explicit and testable | Active |
| Typed chunk payloads before meshing | Prevents opaque byte blobs from leaking across streaming and meshing boundaries | Active |
| Greedy meshing with render-delta bridge | Keeps meshing and renderer ownership separated while supporting incremental GPU updates | Active |
| Multiplayer deferred to interface-only work | Reduces scope while still protecting future expansion seams | Active |
| Persistence, collision, and block editing remain follow-on phases | Renderer groundwork exists; gameplay/runtime capabilities are now the main missing pieces | Active |

---
*Last updated: 2026-03-22 after Vulkan/current-architecture doc refresh*
