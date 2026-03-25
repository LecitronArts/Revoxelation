# Roadmap: Revoxelation

## Overview

This roadmap delivers a modular non-Bevy Rust voxel engine with a highly modern GPU-driven rendering architecture. After stabilizing runtime boundaries and streaming, it layers rendering modernization (bindless, meshlet, lighting), then gameplay features (movement, editing, persistence, network contracts).

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Runtime Skeleton and Quality Gates** - Establish deterministic stage execution, stable subsystem boundaries, and mandatory workflow gates.
- [x] **Phase 2: Streaming Lifecycle and Job Queues** - Make player-driven chunk activation work with explicit lifecycle states and bounded background work.
- [x] **Phase 2.5: Vulkan Bootstrap and Render Infrastructure** (INSERTED) - Replace wgpu with raw Vulkan (ash) and establish gpu-allocator-backed staging pipeline and egui-ash integration.
- [x] **Phase 3: Greedy Meshing and Render Delta Sync** - Produce greedy chunk meshes and push only chunk deltas to the renderer.
- [ ] **Phase 4: Rendering Foundation Overhaul** - Fix critical renderer issues, establish real camera/projection, frustum+Hi-Z culling, GpuOnly memory, swapchain lifecycle.
- [ ] **Phase 5: Bindless Architecture and GPU Scene** - Vulkan 1.2 descriptor indexing (hard requirement, no fallback), unified GPU scene buffer, block material/texture system.
- [ ] **Phase 6: Meshlet Pipeline** - Meshlet generation, per-meshlet GPU culling, software mesh shader emulation, optional VK_EXT_mesh_shader hardware path.
- [ ] **Phase 7: Lighting and Shadows** - Directional PBR lighting, cascaded shadow maps, SSAO, voxel AO, sky/atmosphere with day-night cycle.
- [ ] **Phase 8: Movement and Collision Modes** - Deliver fly/gravity movement with stable voxel collision during streaming churn.
- [ ] **Phase 9: Authoritative Block Editing Feedback** - Apply block edits authoritatively and reflect them visually near-immediately.
- [ ] **Phase 10: Chunk Persistence and Recovery** - Persist edited chunks across restart with versioning/integrity and non-blocking saves.
- [ ] **Phase 11: Network-Ready Deterministic Contracts** - Finalize replay-friendly deterministic event contracts for future multiplayer boundaries.

## Phase Details

### Phase 1: Runtime Skeleton and Quality Gates
**Goal**: Developers can run and extend a deterministic ECS runtime with explicit stage ordering and enforce required quality gates from the start.
**Depends on**: Nothing (first phase)
**Requirements**: ECS-01, ECS-02, ECS-03, QUAL-01
**Success Criteria** (what must be TRUE):
  1. Developer can run one frame and observe fixed stage order: input -> simulation -> world update -> meshing sync -> render submit.
  2. Developer can register systems inside world/meshing/collision/persistence boundaries without creating cross-module coupling.
  3. Runtime emits and consumes serializable events for player actions, chunk lifecycle, and block edits.
  4. Phase workflow artifacts explicitly enforce required superpowers gates before work is marked complete.
**Plans**: 5 plans

Plans:
- [x] 01-01: Implement deterministic scheduler stages and system registration boundaries.
- [x] 01-02: Implement serializable domain events and quality-gate enforcement hooks.
- [x] 01-03: Implement boundary-safe runtime system registration and architecture notes.
- [x] 01-04: Implement serializable event contracts and validation paths.
- [x] 01-05: Implement quality-gate enforcement artifacts and final phase checks.

### Phase 2: Streaming Lifecycle and Job Queues
**Goal**: Player movement reliably controls active chunks through explicit lifecycle states while heavy world work stays bounded off the frame thread.
**Depends on**: Phase 1
**Requirements**: STRM-01, STRM-02, STRM-03
**Success Criteria** (what must be TRUE):
  1. Moving the player activates chunks inside load radius and deactivates chunks outside unload radius.
  2. Each chunk transition is observable through explicit lifecycle states with monotonic revision IDs.
  3. Generation work runs through bounded background queues with cancellation/backpressure under high movement churn.
**Plans**: 2 plans

Plans:
- [x] 02-01-PLAN.md — Types, ChunkStateStore, SSE octree traversal, active-set diff (Wave 1)
- [x] 02-02-PLAN.md — ChunkJobQueue, rayon runner, scheduler wiring, integration test (Wave 2)

### Phase 2.5: Vulkan Bootstrap and Render Infrastructure (INSERTED)
**Goal**: Replace wgpu dependency with raw Vulkan (ash) and establish the gpu-allocator-backed staging pipeline and egui-ash UI integration that Phase 3 meshing will write into.
**Depends on**: Phase 2
**Requirements**: VK-01, VK-02, VK-03
**Success Criteria** (what must be TRUE):
  1. Window renders a clear frame via ash Vulkan without wgpu dependency.
  2. gpu-allocator manages all device/staging memory; no manual DeviceMemory calls.
  3. egui renders HUD overlay through ash backend.
**Plans**: 2 plans

Plans:
- [x] 02.5-01: Vulkan instance, device, swapchain, render pass, frame loop.
- [x] 02.5-02: gpu-allocator staging buffer pipeline + egui-ash integration.

### Phase 3: Greedy Meshing and Render Delta Sync
**Goal**: Visible voxel surfaces are meshed efficiently and renderer sync updates only affected chunks instead of full-world uploads.
**Depends on**: Phase 2.5
**Requirements**: MESH-01, MESH-02, MESH-03
**Success Criteria** (what must be TRUE):
  1. Visible chunk surfaces render using greedy meshing with incremental updates.
  2. Border changes invalidate neighbor chunks correctly so seams are not visible at chunk edges.
  3. Chunk edits and streaming updates apply through chunk-delta renderer uploads without full-world reupload.
**Plans**: 7 plans executed

Plans:
- [x] 03-01-PLAN.md - Typed chunk payloads, greedy mesh generation, and neighbor-aware invalidation.
- [x] 03-02-PLAN.md - Fixed slot-pool renderer delta sync, feature-gated indirect draw, and visible chunk rendering.
- [x] 03-03-PLAN.md - Deterministic chunk payloads, dense draw bookkeeping, and metadata-driven world placement.
- [x] 03-04-PLAN.md - Compute visibility wiring and dense indirect submission completion.
- [x] 03-05-PLAN.md - Optional validation-layer bootstrap so the default debug renderer path opens even when the layer is absent.
- [x] 03-07-PLAN.md - Gap closure: 7-bit pack_vertex, non-degenerate pack_quad corners, vertex shader face-offset decode, CLOCKWISE front face.

### Phase 4: Rendering Foundation Overhaul
**Goal**: Fix all critical renderer issues, establish a real camera system with MVP projection, GPU-driven frustum+Hi-Z occlusion culling, GpuOnly memory with async staging, and robust swapchain lifecycle management.
**Depends on**: Phase 3
**Requirements**: REND-01, REND-02, REND-03, REND-04, REND-05, REND-06, REND-07
**Success Criteria** (what must be TRUE):
  1. Real FPS camera with MVP projection replaces debug_project; WASD+mouse navigation works.
  2. Window can be resized freely without crashes; minimization is handled gracefully.
  3. Compute culling performs real frustum test (6 planes) and Hi-Z occlusion test; culled chunks produce no pixels.
  4. All chunk buffers use GpuOnly memory with proper staging pipeline; no queue_wait_idle in hot path.
  5. Pipeline cache persists across runs; egui HUD shows GPU statistics.
  6. No OnceLock global state; App struct owns all subsystems via dependency injection.
  7. All submit_frame errors are properly propagated and logged.
**Plans**: 7 plans

Plans:
- [ ] 04-01: Infrastructure fixes + dependency injection refactor (env_logger, App struct, error propagation).
- [ ] 04-02: Real camera system + push constants (FPS camera, MVP, dynamic viewport/scissor).
- [ ] 04-03: Swapchain recreation + window management (resize, OUT_OF_DATE, minimize).
- [ ] 04-04: GpuOnly memory model + async transfer (ring-buffer staging, transfer queue).
- [ ] 04-05: Real frustum culling (AABB vs 6 planes, draw count buffer, IndirectCount).
- [ ] 04-06: Hi-Z occlusion culling (depth pyramid, temporal reprojection, occlusion test in cull shader).
- [ ] 04-07: Pipeline cache + performance counters + shader hot-reload + runtime config.

### Phase 5: Bindless Architecture and GPU Scene
**Goal**: Leverage Vulkan 1.2 descriptor indexing (hard requirement) to eliminate per-material descriptor set switching, build a unified GPU scene buffer, and establish a block material/texture system. No Vulkan 1.0 fallback — simplifies code paths significantly.
**Depends on**: Phase 4
**Requirements**: BIND-01, BIND-02, BIND-03, BIND-04, BIND-05
**Success Criteria** (what must be TRUE):
  1. Vulkan 1.2 features are hard-required; device creation fails gracefully with clear error if unsupported.
  2. Single bindless descriptor set binds all resources; no per-chunk descriptor updates needed.
  3. Unified GPU scene buffer reduces buffer count from 6 to 3; rendering output unchanged.
  4. Different block_ids display distinct textures via texture array + bindless sampling.
  5. Chunk capacity grows dynamically beyond fixed 881 limit; IndirectCount eliminates CPU-side draw count.
**Plans**: 5 plans

Plans:
- [ ] 05-01: Vulkan 1.2 device upgrade + descriptor indexing (hard requirement, no fallback).
- [ ] 05-02: Bindless descriptor set + global resource table (BindlessTable, shared set 0).
- [ ] 05-03: Unified GPU scene buffer (GpuChunkInstance SSBO, gl_DrawID indexing).
- [ ] 05-04: Block material system + texture array (BlockMaterial, 2D array texture, bindless sampling).
- [ ] 05-05: Dynamic capacity + IndirectCount (runtime grow, vkCmdDrawIndexedIndirectCount).

### Phase 6: Meshlet Pipeline
**Goal**: Split greedy mesh output into meshlets (64 verts / 124 tris clusters), implement per-meshlet GPU culling (backface+frustum+Hi-Z), and optionally leverage VK_EXT_mesh_shader hardware path with compute+indirect fallback.
**Depends on**: Phase 5
**Requirements**: MSHL-01, MSHL-02, MSHL-03, MSHL-04, MSHL-05
**Success Criteria** (what must be TRUE):
  1. Greedy mesh output is split into meshlets with precomputed bounding spheres and orientation cones.
  2. Per-meshlet GPU culling (backface, frustum, Hi-Z) runs in compute shader with independently toggleable modes.
  3. Software mesh shader emulation via compute+indirect produces correct rendering matching per-chunk path.
  4. VK_EXT_mesh_shader hardware path works on supported GPUs with automatic fallback.
  5. LOD transitions between meshlet groups are seamless with no visible seams.
**Plans**: 5 plans

Plans:
- [ ] 06-01: Meshlet generation (split PackedMesh, bounding sphere, orientation cone, GpuMeshlet SSBO).
- [ ] 06-02: Meshlet GPU culling (meshlet_cull.comp, backface+frustum+Hi-Z, atomicAdd compact list).
- [ ] 06-03: Software task/mesh shader emulation (compute+indirect draw, gl_DrawID meshlet decode).
- [ ] 06-04: VK_EXT_mesh_shader hardware path (task+mesh shaders, feature flag fallback).
- [ ] 06-05: Cluster LOD transitions (meshlet LOD groups, SSE selection, skirt+alpha dither).

### Phase 7: Lighting and Shadows
**Goal**: Establish a complete real-time lighting system with directional PBR, cascaded shadow maps, SSAO, voxel AO, and sky/atmosphere rendering with day-night cycle. Transform the visual quality from flat-colored blocks to a scene with depth and atmosphere.
**Depends on**: Phase 5 (materials/textures), Phase 6 (meshlet normals)
**Requirements**: LGHT-01, LGHT-02, LGHT-03, LGHT-04, LGHT-05
**Success Criteria** (what must be TRUE):
  1. Blocks display PBR lighting with diffuse and specular response under directional light.
  2. Blocks cast correct shadows via 4-cascade CSM; cascade transitions are flicker-free.
  3. SSAO produces visible darkening at block edges and corners; performance < 1ms at 1080p.
  4. Voxel AO provides per-vertex ambient occlusion computed during meshing.
  5. Sky color and light direction change with day-night cycle; distance fog fades far objects.
**Plans**: 5 plans

Plans:
- [ ] 07-01: Directional light + PBR lighting model (Lambertian+GGX, face normals, metallic/roughness).
- [ ] 07-02: Cascaded shadow maps (4 cascades, depth-only pass, PCF soft shadows).
- [ ] 07-03: Screen-space ambient occlusion (SSAO compute, bilateral blur, compositing).
- [ ] 07-04: Voxel ambient occlusion + GI probes (per-vertex AO in meshing, optional irradiance probes).
- [ ] 07-05: Sky/atmosphere + day-night cycle (Preetham model, sun rotation, distance fog).

### Phase 8: Movement and Collision Modes
**Goal**: Players can navigate the world with reliable fly/gravity modes and collision behavior that remains stable during chunk streaming transitions.
**Depends on**: Phase 4 (camera system)
**Requirements**: MOVE-01, MOVE-02, MOVE-03
**Success Criteria** (what must be TRUE):
  1. Player can toggle fly mode and gravity mode at runtime and movement behavior changes immediately.
  2. Gravity movement uses chunk-backed voxel collision that prevents normal wall clipping.
  3. Crossing chunk boundaries during movement remains stable without control loss from streaming churn.
**Plans**: 2 plans

Plans:
- [ ] 08-01: Implement runtime movement mode switching and control handling.
- [ ] 08-02: Implement chunk-backed collision queries and boundary-stable movement updates.

### Phase 9: Authoritative Block Editing Feedback
**Goal**: Block edits mutate authoritative world state first and produce near-immediate visible feedback through remesh/render invalidation.
**Depends on**: Phase 7 (lighting), Phase 8 (movement)
**Requirements**: EDIT-01, EDIT-02, EDIT-03
**Success Criteria** (what must be TRUE):
  1. Player can place and destroy blocks in loaded chunks.
  2. After an edit, the visual world reflects the change near-immediately.
  3. Edited block state is authoritative before async remesh/persistence side effects run.
**Plans**: 2 plans

Plans:
- [ ] 09-01: Implement authoritative block edit command path.
- [ ] 09-02: Implement remesh/render invalidation prioritization for edit feedback.

### Phase 10: Chunk Persistence and Recovery
**Goal**: Edited chunks survive restart through versioned, integrity-checked persistence without stalling normal gameplay frames.
**Depends on**: Phase 9
**Requirements**: SAVE-01, SAVE-02, SAVE-03
**Success Criteria** (what must be TRUE):
  1. Restarting the engine restores previously modified chunks.
  2. Saved chunk payloads include schema version and integrity metadata, and invalid data is detected.
  3. Save/load operations run without stalling frame loop under normal gameplay conditions.
**Plans**: 2 plans

Plans:
- [ ] 10-01: Implement versioned chunk schema with integrity metadata and recovery checks.
- [ ] 10-02: Integrate asynchronous persistence pipeline into activation/unload/checkpoint flow.

### Phase 11: Network-Ready Deterministic Contracts
**Goal**: Engine exposes deterministic replay-friendly contracts for movement, chunk lifecycle, and block edits that are ready for future network transport.
**Depends on**: Phase 10
**Requirements**: NINT-01, NINT-02
**Success Criteria** (what must be TRUE):
  1. Movement, chunk lifecycle, and block edit boundaries expose serializable network-ready contracts.
  2. Replaying captured single-player contract streams yields deterministic world/chunk outcomes.
**Plans**: 1 plan

Plans:
- [ ] 11-01: Define/validate deterministic replay-friendly event contracts and integration checks.

## Progress

**Execution Order:**
Phases execute in numeric order: 2 -> 2.5 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8 -> 9 -> 10 -> 11

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Runtime Skeleton and Quality Gates | 5/5 | Complete | 2026-03-15 |
| 2. Streaming Lifecycle and Job Queues | 2/2 | Complete | 2026-03-21 |
| 2.5. Vulkan Bootstrap and Render Infrastructure | 2/2 | Complete | 2026-03-21 |
| 3. Greedy Meshing and Render Delta Sync | 7/7 | Complete | 2026-03-22 |
| 4. Rendering Foundation Overhaul | 0/7 | Not started | - |
| 5. Bindless Architecture and GPU Scene | 0/5 | Not started | - |
| 6. Meshlet Pipeline | 0/5 | Not started | - |
| 7. Lighting and Shadows | 0/5 | Not started | - |
| 8. Movement and Collision Modes | 0/2 | Not started | - |
| 9. Authoritative Block Editing Feedback | 0/2 | Not started | - |
| 10. Chunk Persistence and Recovery | 0/2 | Not started | - |
| 11. Network-Ready Deterministic Contracts | 0/1 | Not started | - |
