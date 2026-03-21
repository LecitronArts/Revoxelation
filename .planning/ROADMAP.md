# Roadmap: Revoxelation

## Overview

This roadmap delivers a modular non-Bevy Rust voxel engine foundation by stabilizing runtime boundaries first, then layering streaming, meshing, movement, editing, persistence, and network-ready contracts so each phase ends in a verifiable user-visible capability.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Runtime Skeleton and Quality Gates** - Establish deterministic stage execution, stable subsystem boundaries, and mandatory workflow gates.
- [x] **Phase 2: Streaming Lifecycle and Job Queues** - Make player-driven chunk activation work with explicit lifecycle states and bounded background work.
- [x] **Phase 2.5: Vulkan Bootstrap and Render Infrastructure** (INSERTED) - Replace wgpu with raw Vulkan (ash) and establish gpu-allocator-backed staging pipeline and egui-ash integration.
- [ ] **Phase 3: Greedy Meshing and Render Delta Sync** - Produce greedy chunk meshes and push only chunk deltas to the renderer.
- [ ] **Phase 4: Movement and Collision Modes** - Deliver fly/gravity movement with stable voxel collision during streaming churn.
- [ ] **Phase 5: Authoritative Block Editing Feedback** - Apply block edits authoritatively and reflect them visually near-immediately.
- [ ] **Phase 6: Chunk Persistence and Recovery** - Persist edited chunks across restart with versioning/integrity and non-blocking saves.
- [ ] **Phase 7: Network-Ready Deterministic Contracts** - Finalize replay-friendly deterministic event contracts for future multiplayer boundaries.

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
- [ ] 02-01-PLAN.md — Types, ChunkStateStore, SSE octree traversal, active-set diff (Wave 1)
- [ ] 02-02-PLAN.md — ChunkJobQueue, rayon runner, scheduler wiring, integration test (Wave 2)

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
**Plans**: 4 plans

Plans:
- [x] 03-01-PLAN.md - Typed chunk payloads, greedy mesh generation, and neighbor-aware invalidation.
- [x] 03-02-PLAN.md - Fixed slot-pool renderer delta sync, feature-gated indirect draw, and visible chunk rendering.
- [x] 03-03-PLAN.md - Deterministic chunk payloads, dense draw bookkeeping, and metadata-driven world placement.
- [ ] 03-04-PLAN.md - Compute visibility wiring and dense indirect submission completion.

### Phase 4: Movement and Collision Modes
**Goal**: Players can navigate the world with reliable fly/gravity modes and collision behavior that remains stable during chunk streaming transitions.
**Depends on**: Phase 2
**Requirements**: MOVE-01, MOVE-02, MOVE-03
**Success Criteria** (what must be TRUE):
  1. Player can toggle fly mode and gravity mode at runtime and movement behavior changes immediately.
  2. Gravity movement uses chunk-backed voxel collision that prevents normal wall clipping.
  3. Crossing chunk boundaries during movement remains stable without control loss from streaming churn.
**Plans**: 2 plans

Plans:
- [ ] 04-01: Implement runtime movement mode switching and control handling.
- [ ] 04-02: Implement chunk-backed collision queries and boundary-stable movement updates.

### Phase 5: Authoritative Block Editing Feedback
**Goal**: Block edits mutate authoritative world state first and produce near-immediate visible feedback through remesh/render invalidation.
**Depends on**: Phase 3, Phase 4
**Requirements**: EDIT-01, EDIT-02, EDIT-03
**Success Criteria** (what must be TRUE):
  1. Player can place and destroy blocks in loaded chunks.
  2. After an edit, the visual world reflects the change near-immediately.
  3. Edited block state is authoritative before async remesh/persistence side effects run.
**Plans**: 2 plans

Plans:
- [ ] 05-01: Implement authoritative block edit command path.
- [ ] 05-02: Implement remesh/render invalidation prioritization for edit feedback.

### Phase 6: Chunk Persistence and Recovery
**Goal**: Edited chunks survive restart through versioned, integrity-checked persistence without stalling normal gameplay frames.
**Depends on**: Phase 5
**Requirements**: SAVE-01, SAVE-02, SAVE-03
**Success Criteria** (what must be TRUE):
  1. Restarting the engine restores previously modified chunks.
  2. Saved chunk payloads include schema version and integrity metadata, and invalid data is detected.
  3. Save/load operations run without stalling frame loop under normal gameplay conditions.
**Plans**: 2 plans

Plans:
- [ ] 06-01: Implement versioned chunk schema with integrity metadata and recovery checks.
- [ ] 06-02: Integrate asynchronous persistence pipeline into activation/unload/checkpoint flow.

### Phase 7: Network-Ready Deterministic Contracts
**Goal**: Engine exposes deterministic replay-friendly contracts for movement, chunk lifecycle, and block edits that are ready for future network transport.
**Depends on**: Phase 6
**Requirements**: NINT-01, NINT-02
**Success Criteria** (what must be TRUE):
  1. Movement, chunk lifecycle, and block edit boundaries expose serializable network-ready contracts.
  2. Replaying captured single-player contract streams yields deterministic world/chunk outcomes.
**Plans**: 1 plan

Plans:
- [ ] 07-01: Define/validate deterministic replay-friendly event contracts and integration checks.

## Progress

**Execution Order:**
Phases execute in numeric order: 2 -> 2.5 -> 3 -> 4

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Runtime Skeleton and Quality Gates | 5/5 | Complete | 2026-03-15 |
| 2. Streaming Lifecycle and Job Queues | 2/2 | Complete    | 2026-03-21 |
| 2.5. Vulkan Bootstrap and Render Infrastructure | 2/2 | Complete    | 2026-03-21 |
| 3. Greedy Meshing and Render Delta Sync | 3/4 | In Progress | - |
| 4. Movement and Collision Modes | 0/2 | Not started | - |
| 5. Authoritative Block Editing Feedback | 0/2 | Not started | - |
| 6. Chunk Persistence and Recovery | 0/2 | Not started | - |
| 7. Network-Ready Deterministic Contracts | 0/1 | Not started | - |
