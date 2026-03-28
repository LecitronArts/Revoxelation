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

- [x] **FIX-01**: Hi-Z pyramid is recreated on swapchain resize — no GPU crash or validation errors from stale depth image references.
- [x] **FIX-02**: egui scratch buffers respect double-buffered frame lifetimes — no GPU use-after-free.
- [x] **FIX-03**: Camera position is passed to streaming active-set computation — chunks follow the player, not stuck at origin.
- [x] **FIX-04**: dense_indirect_shadow access is bounds-checked — no OOB panic on malformed draw_index.
- [x] **FIX-05**: All unsafe impl Send have documented SAFETY invariants explaining raw pointer ownership and thread safety.
- [x] **FIX-06**: draw_cmd_as_bytes replaced with safe bytemuck cast or field-by-field write — no manual from_raw_parts.
- [x] **FIX-07**: Staging ring exhaustion degrades gracefully — partial batch with deferred deltas instead of frame failure.
- [x] **FIX-08**: All clippy warnings resolved — zero warnings from cargo clippy --all-targets.
- [x] **FIX-09**: Drop implementations log resource cleanup failures instead of silently discarding errors.

### Meshlet Pipeline

- [x] **MSHL-01**: Greedy mesh output is split into meshlets with precomputed bounding spheres and orientation cones.
- [x] **MSHL-02**: Per-meshlet GPU culling (backface, frustum, Hi-Z) runs in compute shader with toggleable modes.
- [x] **MSHL-03**: Software mesh shader emulation via compute+indirect produces correct rendering.
- [x] **MSHL-04**: VK_EXT_mesh_shader hardware path works on supported GPUs with automatic fallback.
- [x] **MSHL-05**: LOD transitions between meshlet groups are seamless with no visible seams.

### Critical Bug Fixes (Phase 06.1)

- [x] **CRIT-01**: Depth attachment store_op changed from DONT_CARE to STORE — Hi-Z pyramid reads valid depth data on all GPU vendors.
- [x] **CRIT-02**: scene_buffer grow copies data per-region with correct src/dst offsets — no rendering corruption after capacity growth.
- [x] **CRIT-03**: Mesh shader push constants split into two calls matching pipeline layout ranges — no Vulkan spec violation.
- [x] **CRIT-04**: MeshletPool tracks removals and reclaims GPU buffer space — active counts decrement on chunk unload, no unbounded growth.
- [x] **CRIT-05**: SSE distance calculation uses world-space chunk coordinates (key × chunk_edge × lod_scale) — streaming loads correct chunks.
- [x] **CRIT-06**: deactivate_chunk handles Queued/Loading states — chunks transition to Inactive and can be re-activated.
- [x] **CRIT-07**: Hi-Z pass 0 correctly handles equal src/dst resolution — no incorrect 2×2 sampling of out-of-bounds texels.

### High-Priority Bug Fixes (Phase 06.1)

- [x] **HIGH-01**: egui descriptor set uses UPDATE_AFTER_BIND or per-frame sets — no use-after-free on font texture update.
- [x] **HIGH-02**: destroy_allocated_buffer/image destroys resource before freeing allocation — correct Vulkan destruction order.
- [x] **HIGH-03**: ChunkStateStore removes entries on Inactive transition — no unbounded HashMap growth during exploration.
- [x] **HIGH-04**: cancel_flags entries cleaned up on Queued deactivation — no Arc<AtomicBool> leaks.
- [x] **HIGH-05**: Dirty mesh records removed when payload absent — no unbounded dirty HashMap growth.
- [x] **HIGH-06**: Job queue eviction compares new task SSE against evicted — no incorrect eviction of higher-priority tasks.
- [x] **HIGH-07**: Bindless descriptor stageFlags include TASK_SHADER_BIT_EXT and MESH_SHADER_BIT_EXT — mesh shader path has valid descriptor access.

### Medium Bug Fixes and Hardening (Phase 06.1)

- [x] **MED-01**: Camera near-plane extraction uses Vulkan z∈[0,w] formula (row2 only) — correct frustum plane derivation.
- [x] **MED-02**: Pipeline barriers include TASK_SHADER_BIT_EXT/MESH_SHADER_BIT_EXT in dstStageMask — correct synchronization for mesh shader path.
- [x] **MED-03**: transition_image_layout catch-all uses conservative access masks and logs warning — no silent zero-synchronization.
- [x] **MED-04**: StagingBuffer::write returns Result and fails on unmapped memory — no silent data loss.
- [x] **MED-05**: max_draw_count uses meshlet_pool capacity instead of hardcoded constant — correct after future grow.
- [x] **MED-06**: PrioritizedTask computes real SSE at enqueue time — job queue priority sorting is effective.
- [x] **MED-07**: Dirty queue uses HashSet for O(1) dedup instead of O(n) VecDeque scan.
- [x] **MED-08**: run_mesh_sync limits job results processed per frame — no frame stall on bulk completion.
- [x] **MED-09**: cancel_flags use Acquire/Release ordering — correct cross-thread visibility on ARM.
- [x] **MED-10**: seed_input_commands placeholder removed or guarded by cfg(test) — no per-frame dummy events in production.
- [x] **MED-11**: eprintln! diagnostics replaced with log::debug! — respects log level configuration.
- [ ] **MED-12**: Octree parent clamp replaced with skip-link — no incorrect topology from coordinate saturation.

### Rendering Polish and Optimization (Phase 06.1)

- [ ] **POLISH-01**: All shader hardcoded constants (screen_height=1080, SSE threshold=2.0, distance clamp=0.001) are parameterized via push constants or UBO — no magic numbers in shader source.
- [x] **POLISH-02**: Texture array uses mipmap chain with anisotropic filtering sampler; block textures are sharp at distance without shimmer.
- [x] **POLISH-03**: MSAA (4× minimum) or equivalent post-process AA eliminates jagged block edges.
- [ ] **POLISH-04**: Shared shader include system eliminates duplicated code between chunk_mesh/meshlet_draw/meshlet.mesh shaders.
- [x] **POLISH-05**: All runtime unwrap()/panic!() in non-test code replaced with Result propagation or graceful error logging.
- [x] **POLISH-06**: GPU performance counters read actual culled meshlet/chunk counts from GPU (async readback, 1-2 frame latency) — HUD shows real numbers.
- [ ] **POLISH-07**: Shader compilation uses performance optimization level; SPIR-V output is optimized.
- [ ] **POLISH-08**: Chunk streaming transitions use distance-based fade-in instead of instant pop-in.
- [ ] **POLISH-09**: Camera movement uses delta-time smoothing and configurable sensitivity — no jitter or speed variance across frame rates.

### Architecture Refactoring (Phase 06.1)

- [ ] **REFAC-01**: Renderer struct split into sub-structs (VulkanCore, PipelineSet, PoolManager) — no 38-field God Object.
- [ ] **REFAC-02**: submit_frame decomposed into named sub-functions matching frame sequence steps — no 484-line monolith.
- [ ] **REFAC-03**: create/recreate_swapchain_context merged into single parameterized function — no 60% code duplication.
- [ ] **REFAC-04**: Staging copy pattern extracted into helper function — no 10× repetition in chunk_pool.rs.
- [ ] **REFAC-05**: Binding IDs defined as named constants (BINDING_SCENE, BINDING_FRUSTUM, etc.) — no magic numbers.
- [ ] **REFAC-06**: hecs dependency removed from Cargo.toml — unused ECS crate cleaned up.
- [ ] **REFAC-07**: Dead code removed (ChunkDrawMetadata, commented skirt code, unused window_extent writes).
- [ ] **REFAC-08**: app::run() frame logic extracted into App::tick() method — event loop body ≤50 lines.

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
| FIX-01 | Phase 05.1 | Complete |
| FIX-02 | Phase 05.1 | Complete |
| FIX-03 | Phase 05.1 | Complete |
| FIX-04 | Phase 05.1 | Complete |
| FIX-05 | Phase 05.1 | Complete |
| FIX-06 | Phase 05.1 | Complete |
| FIX-07 | Phase 05.1 | Complete |
| FIX-08 | Phase 05.1 | Complete |
| FIX-09 | Phase 05.1 | Complete |
| MSHL-01 | Phase 6 | Complete |
| MSHL-02 | Phase 6 | Complete |
| MSHL-03 | Phase 6 | Complete |
| MSHL-04 | Phase 6 | Complete |
| MSHL-05 | Phase 6 | Complete |
| POLISH-01 | Phase 06.1 | Pending |
| POLISH-02 | Phase 06.1 | Complete |
| POLISH-03 | Phase 06.1 | Complete |
| POLISH-04 | Phase 06.1 | Pending |
| POLISH-05 | Phase 06.1 | Complete |
| POLISH-06 | Phase 06.1 | Complete |
| POLISH-07 | Phase 06.1 | Pending |
| POLISH-08 | Phase 06.1 | Pending |
| POLISH-09 | Phase 06.1 | Pending |
| CRIT-01 | Phase 06.1 | Complete |
| CRIT-02 | Phase 06.1 | Complete |
| CRIT-03 | Phase 06.1 | Complete |
| CRIT-04 | Phase 06.1 | Complete |
| CRIT-05 | Phase 06.1 | Complete |
| CRIT-06 | Phase 06.1 | Complete |
| CRIT-07 | Phase 06.1 | Complete |
| HIGH-01 | Phase 06.1 | Complete |
| HIGH-02 | Phase 06.1 | Complete |
| HIGH-03 | Phase 06.1 | Complete |
| HIGH-04 | Phase 06.1 | Complete |
| HIGH-05 | Phase 06.1 | Complete |
| HIGH-06 | Phase 06.1 | Complete |
| HIGH-07 | Phase 06.1 | Complete |
| MED-01 | Phase 06.1 | Complete |
| MED-02 | Phase 06.1 | Complete |
| MED-03 | Phase 06.1 | Complete |
| MED-04 | Phase 06.1 | Complete |
| MED-05 | Phase 06.1 | Complete |
| MED-06 | Phase 06.1 | Complete |
| MED-07 | Phase 06.1 | Complete |
| MED-08 | Phase 06.1 | Complete |
| MED-09 | Phase 06.1 | Complete |
| MED-10 | Phase 06.1 | Complete |
| MED-11 | Phase 06.1 | Complete |
| MED-12 | Phase 06.1 | Pending |
| REFAC-01 | Phase 06.1 | Pending |
| REFAC-02 | Phase 06.1 | Pending |
| REFAC-03 | Phase 06.1 | Pending |
| REFAC-04 | Phase 06.1 | Pending |
| REFAC-05 | Phase 06.1 | Pending |
| REFAC-06 | Phase 06.1 | Pending |
| REFAC-07 | Phase 06.1 | Pending |
| REFAC-08 | Phase 06.1 | Pending |
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
- v1 requirements: 96 total
- Mapped to phases: 96
- Unmapped: 0

---
*Requirements defined: 2026-03-15*
*Last updated: 2026-03-28 — added CRIT-01~07, HIGH-01~07, MED-01~12, REFAC-01~08 for Phase 06.1 deep review fixes*
