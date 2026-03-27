# Requirements: Revoxelation

**Defined:** 2026-03-15
**Core Value:** Build a cleanly extensible Rust ECS voxel engine (non-Bevy) with a highly modern GPU-driven rendering architecture, where world interaction, especially block edits, is reflected immediately and predictably.

## v1 Requirements

### Runtime and ECS

- [x] **ECS-01**: Developer can run a deterministic mixed scheduler with explicit fixed stages for input, simulation, world update, meshing sync, and render submit.
- [x] **ECS-02**: Developer can register systems through stable boundaries (world, meshing, collision, persistence) without introducing cross-module coupling.
- [x] **ECS-03**: Engine can emit and consume serializable domain events for player actions, chunk lifecycle, and block edits.

### World Streaming

- [x] **STRM-01**: Player movement drives chunk activation/deactivation within configured load and unload radii.
- [x] **STRM-02**: Chunk load/unload transitions are represented by explicit lifecycle states and revision IDs.
- [x] **STRM-03**: Heavy chunk generation work executes through bounded background queues with cancellation/backpressure behavior.

### Meshing and Render Sync

- [x] **MESH-01**: Engine can generate greedy meshes for visible chunk surfaces and update them incrementally.
- [x] **MESH-02**: Chunk-border updates correctly invalidate neighbor meshes to avoid visible seams.
- [x] **MESH-03**: Renderer integration supports chunk-delta updates so chunk edits do not require full world reupload.

### Rendering Foundation

- [x] **REND-01**: Real FPS camera with MVP projection replaces debug_project; supports WASD+mouse navigation.
- [x] **REND-02**: Swapchain recreates correctly on window resize; minimization handled gracefully.
- [ ] **REND-03**: GPU-driven frustum culling via compute shader tests chunk AABB against 6 frustum planes.
- [x] **REND-04**: Hi-Z occlusion culling uses depth pyramid to reject occluded chunks before draw.
- [x] **REND-05**: All chunk GPU buffers use GpuOnly memory with proper staging pipeline; no queue_wait_idle in hot path.
- [x] **REND-06**: OnceLock global state replaced with App-struct dependency injection for testability.
- [x] **REND-07**: Pipeline cache persists across runs; egui HUD displays GPU performance statistics.

### Bindless and GPU Scene

- [x] **BIND-01**: Vulkan 1.2 is a hard requirement; device creation fails gracefully with clear error message if GPU does not support required features.
- [x] **BIND-02**: Single bindless descriptor set binds all GPU resources; no per-chunk descriptor updates needed.
- [x] **BIND-03**: Unified GPU scene buffer reduces per-chunk buffer count; rendering output unchanged.
- [x] **BIND-04**: Block material system supports distinct textures per block_id via texture array + bindless sampling.
- [x] **BIND-05**: Chunk render capacity grows dynamically beyond fixed limit; IndirectCount eliminates CPU-side draw count.

### Bug Fixes and Safety Hardening

- [ ] **FIX-01**: Hi-Z pyramid is recreated on swapchain resize — no GPU crash or validation errors from stale depth image references.
- [x] **FIX-02**: egui scratch buffers respect double-buffered frame lifetimes — no GPU use-after-free.
- [ ] **FIX-03**: Camera position is passed to streaming active-set computation — chunks follow the player, not stuck at origin.
- [ ] **FIX-04**: dense_indirect_shadow access is bounds-checked — no OOB panic on malformed draw_index.
- [ ] **FIX-05**: All unsafe impl Send have documented SAFETY invariants explaining raw pointer ownership and thread safety.
- [ ] **FIX-06**: draw_cmd_as_bytes replaced with safe bytemuck cast or field-by-field write — no manual from_raw_parts.
- [ ] **FIX-07**: Staging ring exhaustion degrades gracefully — partial batch with deferred deltas instead of frame failure.
- [ ] **FIX-08**: All clippy warnings resolved — zero warnings from cargo clippy --all-targets.
- [ ] **FIX-09**: Drop implementations log resource cleanup failures instead of silently discarding errors.

### Meshlet Pipeline

- [ ] **MSHL-01**: Greedy mesh output is split into meshlets with precomputed bounding spheres and orientation cones.
- [ ] **MSHL-02**: Per-meshlet GPU culling (backface, frustum, Hi-Z) runs in compute shader with toggleable modes.
- [ ] **MSHL-03**: Software mesh shader emulation via compute+indirect produces correct rendering.
- [ ] **MSHL-04**: VK_EXT_mesh_shader hardware path works on supported GPUs with automatic fallback.
- [ ] **MSHL-05**: LOD transitions between meshlet groups are seamless with no visible seams.

### Lighting and Shadows

- [ ] **LGHT-01**: Blocks display PBR lighting with diffuse and specular response under directional light.
- [ ] **LGHT-02**: Blocks cast correct shadows via cascaded shadow maps; cascade transitions are flicker-free.
- [ ] **LGHT-03**: SSAO produces visible darkening at block edges and corners with acceptable performance.
- [ ] **LGHT-04**: Voxel AO provides per-vertex ambient occlusion computed during meshing.
- [ ] **LGHT-05**: Sky color and light direction change with day-night cycle; distance fog fades far objects.

### Movement and Collision

- [ ] **MOVE-01**: Player can switch between fly mode and gravity mode during runtime.
- [ ] **MOVE-02**: Player collision uses chunk-backed voxel queries and prevents wall clipping in normal movement scenarios.
- [ ] **MOVE-03**: Movement/collision logic remains stable under streaming churn (entering/leaving chunk boundaries).

### Block Editing

- [ ] **EDIT-01**: Player can place and destroy blocks in loaded chunks.
- [ ] **EDIT-02**: A block edit triggers mesh/render invalidation and produces near-immediate visible feedback.
- [ ] **EDIT-03**: Block edits are applied to authoritative world state before async side effects (meshing/persistence) are dispatched.

### Persistence

- [ ] **SAVE-01**: Modified chunks are persisted and restored across restart.
- [ ] **SAVE-02**: Persistence uses versioned chunk schema with integrity metadata (for corruption detection/migration control).
- [ ] **SAVE-03**: Save operations do not stall frame loop during normal gameplay.

### Future Networking Interfaces

- [ ] **NINT-01**: Engine exposes network-ready event contracts for movement, chunk lifecycle, and block edits.
- [ ] **NINT-02**: Event contracts are deterministic and replay-friendly in single-player runtime.

### Quality Gates

- [x] **QUAL-01**: Planning and implementation workflow enforces superpowers gates (`writing-plans`, `test-driven-development`, `systematic-debugging`, `verification-before-completion`, `requesting-code-review`, `receiving-code-review`, `finishing-a-development-branch`).

## v2 Requirements

### Multiplayer

- **NET-01**: Basic client/server state synchronization for player movement and block edits.
- **NET-02**: Interest management for chunk replication.

### Performance and Scale

- **PERF-01**: Formal frame-time budget targets and profiling baselines for representative scenes.
- **PERF-02**: Advanced chunk/mesh compression and memory optimization pass.

### Tooling

- **TOOL-01**: In-engine debug tooling for chunk lifecycle visualization and event tracing UI.
- **TOOL-02**: Replay tooling for event stream diagnostics.

## Out of Scope

| Feature | Reason |
|---------|--------|
| Full multiplayer implementation in v1 | Interfaces only in v1; replication is deferred to v2 |
| Bevy migration | Conflicts with explicit non-Bevy architecture direction |
| Mobile/Web deployment | Desktop-first (Windows/Linux) scope for current milestone |
| Premature deep optimization | Stability and architecture closure are prioritized first |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| ECS-01 | Phase 1 | Complete |
| ECS-02 | Phase 1 | Complete |
| ECS-03 | Phase 1 | Complete |
| STRM-01 | Phase 2 | Complete |
| STRM-02 | Phase 2 | Complete |
| STRM-03 | Phase 2 | Complete |
| MESH-01 | Phase 3 | Complete |
| MESH-02 | Phase 3 | Complete |
| MESH-03 | Phase 3 | Complete |
| REND-01 | Phase 4 | Pending |
| REND-02 | Phase 4 | Complete |
| REND-03 | Phase 4 | Pending |
| REND-04 | Phase 4 | Complete |
| REND-05 | Phase 4 | Complete |
| REND-06 | Phase 4 | Pending |
| REND-07 | Phase 4 | Complete |
| BIND-01 | Phase 5 | Complete |
| BIND-02 | Phase 5 | Complete |
| BIND-03 | Phase 5 | Complete |
| BIND-04 | Phase 5 | Complete |
| BIND-05 | Phase 5 | Complete |
| FIX-01 | Phase 05.1 | Pending |
| FIX-02 | Phase 05.1 | Complete |
| FIX-03 | Phase 05.1 | Pending |
| FIX-04 | Phase 05.1 | Pending |
| FIX-05 | Phase 05.1 | Pending |
| FIX-06 | Phase 05.1 | Pending |
| FIX-07 | Phase 05.1 | Pending |
| FIX-08 | Phase 05.1 | Pending |
| FIX-09 | Phase 05.1 | Pending |
| MSHL-01 | Phase 6 | Pending |
| MSHL-02 | Phase 6 | Pending |
| MSHL-03 | Phase 6 | Pending |
| MSHL-04 | Phase 6 | Pending |
| MSHL-05 | Phase 6 | Pending |
| LGHT-01 | Phase 7 | Pending |
| LGHT-02 | Phase 7 | Pending |
| LGHT-03 | Phase 7 | Pending |
| LGHT-04 | Phase 7 | Pending |
| LGHT-05 | Phase 7 | Pending |
| MOVE-01 | Phase 8 | Pending |
| MOVE-02 | Phase 8 | Pending |
| MOVE-03 | Phase 8 | Pending |
| EDIT-01 | Phase 9 | Pending |
| EDIT-02 | Phase 9 | Pending |
| EDIT-03 | Phase 9 | Pending |
| SAVE-01 | Phase 10 | Pending |
| SAVE-02 | Phase 10 | Pending |
| SAVE-03 | Phase 10 | Pending |
| NINT-01 | Phase 11 | Pending |
| NINT-02 | Phase 11 | Pending |
| QUAL-01 | Phase 1 | Complete |

**Coverage:**
- v1 requirements: 52 total
- Mapped to phases: 52
- Unmapped: 0

---
*Requirements defined: 2026-03-15*
*Last updated: 2026-03-27 — added FIX-01 through FIX-09 for Phase 05.1 bug fixes and safety hardening*
