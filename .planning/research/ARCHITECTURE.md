# Revoxelation V1 Architecture Recommendation

## Scope

This document recommends how to add V1 gameplay foundation features to the current non-Bevy Rust codebase:

- chunk streaming
- greedy meshing
- collision-capable movement
- block edits with fast visual feedback
- persistence for modified chunks
- ECS-centered integration and scheduling

It assumes the existing renderer/world architecture described in `.planning/codebase/*.md` remains the baseline (renderer bootstrap, world sync pipeline, `hecs` runtime, `wgpu` backend).

## Architecture Goals

1. Keep frame loop deterministic and responsive while heavy work runs off-thread.
2. Make chunk lifecycle explicit (streaming, generation, meshing, persistence).
3. Support fast edit-to-visual latency without full-world reupload.
4. Keep ECS as orchestration layer, not storage for bulk chunk voxel data.
5. Preserve clear module boundaries so future networking can attach to stable events.

## Recommended System Decomposition

### 1) Runtime Orchestration (ECS + Stage Scheduler)

Responsibility:
- Own frame stage order and system execution.
- Own player intent/movement state and gameplay events.
- Trigger world streaming updates from player position.

Recommended module surface:
- `src/ecs.rs` (or split into `src/ecs/*`)
  - `stages.rs`: fixed stage list and runner
  - `components.rs`: player/camera/movement data
  - `events.rs`: typed domain events
  - `systems/*`: movement, streaming trigger, interaction trigger

Boundary rule:
- ECS systems issue commands/events; they do not directly mutate renderer internals.

### 2) World Domain Core

Responsibility:
- Canonical chunk voxel data and metadata.
- Chunk state machine and dirty flags.
- Spatial queries for collision/raycast.

Recommended module surface:
- `src/world/mod.rs` (entry)
- `src/world/chunk.rs`: `Chunk`, `ChunkCoord`, block storage
- `src/world/index.rs`: active chunk map / lookup API
- `src/world/state.rs`: chunk lifecycle state
- `src/world/query.rs`: voxel sampling, AABB sweep helpers, raycast

Boundary rule:
- World domain owns chunk truth; meshing and renderer consume snapshots/deltas.

### 3) Streaming + Job System

Responsibility:
- Compute desired active chunk set around player.
- Schedule load/generate/unload and meshing jobs.
- Return completed work back to main thread safely.

Recommended module surface:
- `src/world/streaming.rs`
- `src/world/jobs.rs`
  - generation queue
  - meshing queue
  - completion channels

Boundary rule:
- Background workers never touch GPU APIs directly; they produce CPU outputs only.

### 4) Greedy Meshing Pipeline

Responsibility:
- Convert chunk voxel data (plus neighbor visibility context) into mesh sections.
- Produce compact CPU mesh payloads keyed by chunk coord + revision.

Recommended module surface:
- `src/world/meshing.rs`
  - `build_mesh(chunk, neighbors) -> ChunkMesh`
  - mesh revision/version tagging

Boundary rule:
- Mesher reads immutable voxel snapshots and writes immutable mesh outputs.

### 5) Renderer Integration Layer

Responsibility:
- Accept chunk mesh deltas and update GPU resources incrementally.
- Keep render protocol stable and avoid full world rebuild for local edits.

Recommended module surface extension:
- `src/renderer/world/sync.rs`:
  - keep full sync path for bootstrap/fallback
  - add incremental `WorldDelta` apply path
- `src/renderer/world/upload.rs`:
  - per-chunk mesh upload/free
- `src/renderer/core/world_ops.rs`:
  - lifecycle event dispatch for chunk delta application

Boundary rule:
- Renderer consumes prepared deltas; it does not run world generation or meshing logic.

### 6) Interaction + Edit Pipeline

Responsibility:
- Convert player action to block edit commands.
- Apply edit atomically to world chunk data.
- Trigger dependent chunk remesh and persistence dirtying.

Recommended module surface:
- `src/world/edits.rs`
  - `EditCommand` (`Place`, `Break`)
  - `apply_edit(world, cmd) -> EditResult`

Boundary rule:
- Edit application is main-thread authoritative; worker threads only process derived tasks.

### 7) Persistence

Responsibility:
- Save/load changed chunk data and metadata.
- Integrate with streaming lifecycle (load on activation, flush on unload/checkpoint).

Recommended module surface:
- `src/world/persistence.rs`
  - chunk serialization format
  - dirty chunk journal/index
  - async write queue and crash-safe temp-to-final commit

Boundary rule:
- Persistence stores world state only (voxels + minimal metadata), not renderer resources.

## Core Chunk Lifecycle State Machine

Recommended states:

- `Absent`
- `QueuedLoadOrGen`
- `Populating`
- `ResidentDirtyMesh`
- `ResidentMeshed`
- `QueuedUnload`
- `Persisting`

State transitions are event-driven and revisioned:
- chunk voxel revision increments on successful edit or generation completion
- mesh revision tracks the voxel revision it was built from
- renderer uploads only if `mesh_revision > gpu_revision`

## ECS Stage Order (Fixed Pipeline)

Recommended per-frame stage order:

1. `InputStage`
2. `IntentStage` (movement mode toggles, interaction intent)
3. `PhysicsStage` (collision + integration)
4. `StreamingPlanStage` (compute desired chunk set)
5. `MainThreadWorldApplyStage` (apply completed generation/mesh jobs)
6. `InteractionStage` (raycast, place/break)
7. `PostEditStage` (mark remesh + persistence dirty)
8. `RenderSyncStage` (submit world deltas to renderer)
9. `MaintenanceStage` (persistence flush budget, diagnostics)

This keeps collision/edit outcomes visible before render sync in the same frame when possible.

## Data Flow

### A) Player Movement -> Chunk Streaming

1. ECS movement updates player transform.
2. Streaming planner computes target chunk radius around player chunk coord.
3. Diff against active chunk set:
   - enqueue missing chunks for load/generation
   - mark distant chunks for unload/persist
4. Worker completions are drained on main thread and applied to world index.
5. Applied chunks are marked `DirtyMesh` and meshing jobs are enqueued.

### B) Chunk Data -> Greedy Mesh -> GPU

1. Meshing job reads chunk voxel snapshot + neighbor border snapshots.
2. Greedy mesher emits opaque/transparent face groups and bounds.
3. Main thread receives `ChunkMeshReady` with `chunk_coord` + `mesh_revision`.
4. Renderer delta sync uploads/replaces chunk GPU buffers only for changed chunks.
5. Renderer lifecycle emits success/rejection diagnostics per chunk delta batch.

### C) Collision Queries

1. Physics system builds swept AABB from velocity + dt.
2. Query layer samples overlapping voxels from resident chunks.
3. Resolve axis-by-axis penetration and update grounded state.
4. If required chunks are missing, use conservative behavior:
   - no fall-through into unknown space
   - optional movement clamp at stream boundary

### D) Block Edit -> Visual + Persistence Feedback

1. Interaction system raycasts from camera/player to hit voxel face.
2. Emits `EditCommand` (`Place`/`Break`) with authoritative target.
3. World edit applies change, increments voxel revision.
4. Mark edited chunk and border-neighbor chunks `DirtyMesh` when edge touched.
5. Enqueue high-priority remesh jobs for touched chunks.
6. Mark edited chunks `DirtyPersistence`.
7. On mesh completion, renderer applies chunk deltas; edit becomes visible.

### E) Persistence With Streaming

1. On chunk activation: attempt disk load first; fallback to generation.
2. On edit: mark chunk dirty in persistence journal.
3. On unload/checkpoint interval: serialize dirty chunks via async writer.
4. On successful write: clear dirty flag and update persisted revision.
5. On shutdown: drain persistence queue with bounded timeout and final report.

## Event Contracts (Network-Ready but Local-Only)

Use typed domain events now so multiplayer can map onto them later.

Recommended events:

- `ChunkActivated { coord, source: Load|Generate }`
- `ChunkMeshed { coord, mesh_revision }`
- `ChunkUnloaded { coord }`
- `BlockEdited { coord, local_pos, old_block, new_block, actor }`
- `PlayerMoved { entity, from, to, grounded }`
- `PersistenceFlushed { coord, revision }`

Rule:
- keep events deterministic and serializable; avoid embedding renderer types.

## Build Order (Suggested Implementation Sequence)

### Phase 1: ECS Runtime and Stage Skeleton
- Introduce explicit fixed stage runner around existing `hecs` usage.
- Add event bus and frame-local command buffers.
- Keep behavior parity initially.

Exit criteria:
- stable per-frame stage execution with tests for ordering.

### Phase 2: Chunk Lifecycle + Streaming Planner
- Implement active-set diff around player-centric radius.
- Add chunk lifecycle states and revision metadata.
- Wire generation/load enqueue/dequeue mechanics.

Exit criteria:
- moving player triggers deterministic load/unload events.

### Phase 3: Background Job Infrastructure
- Separate generation and meshing queues with priorities.
- Add bounded worker pools and completion channels.
- Add main-thread apply stage with budget controls.

Exit criteria:
- no frame-thread blocking on generation/meshing.

### Phase 4: Greedy Meshing + Incremental Renderer Delta Path
- Implement chunk greedy mesher using neighbor context.
- Add renderer chunk-delta upload path alongside existing full sync.
- Make incremental path default after validation.

Exit criteria:
- world updates no longer require full payload rebuild for local changes.

### Phase 5: Collision + Movement Modes
- Add gravity/fly movement modes in ECS physics systems.
- Implement chunk-backed voxel collision queries and sweep resolution.
- Add stream-boundary-safe fallback behavior.

Exit criteria:
- predictable grounded movement without tunneling in loaded terrain.

### Phase 6: Block Edit Pipeline
- Add raycast targeting and authoritative edit apply.
- Trigger neighbor-aware remesh invalidation.
- Prioritize edit-caused remesh jobs for near-immediate feedback.

Exit criteria:
- place/break updates visible quickly and consistently.

### Phase 7: Persistence Integration
- Add chunk disk format and dirty journal.
- Integrate load-on-activate and flush-on-unload/checkpoint.
- Add startup/shutdown integrity checks.

Exit criteria:
- edited chunks persist across restart.

### Phase 8: Stabilization + Contract Hardening
- Harden domain event schemas for future networking.
- Add integration tests for end-to-end flows:
  - move -> stream -> mesh -> render
  - edit -> remesh -> render -> persist

Exit criteria:
- architecture stable and ready for post-v1 optimization passes.

## Verification Targets Per Subsystem

- Streaming: no orphan chunk states; bounded queue growth.
- Meshing: mesh revision always matches voxel revision source.
- Collision: deterministic results for fixed seed + input replay.
- Edits: same-frame or next-frame visual confirmation under normal load.
- Persistence: no dirty chunk loss on clean shutdown.
- ECS integration: stage ordering invariants covered by tests.

## Final Recommendation

Adopt a staged ECS-orchestrated architecture where world data, background jobs, meshing, renderer sync, and persistence are separate but event-coupled subsystems. Keep world state authoritative on main thread, run heavy transforms in worker queues, and move renderer sync to chunk deltas to make block edits and streaming responsive enough for V1 gameplay prototyping.
