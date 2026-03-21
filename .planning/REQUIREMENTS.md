# Requirements: Revoxelation

**Defined:** 2026-03-15  
**Core Value:** Build a cleanly extensible Rust ECS voxel engine (non-Bevy) where world interaction, especially block edits, is reflected immediately and predictably.

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

- [ ] **MESH-01**: Engine can generate greedy meshes for visible chunk surfaces and update them incrementally.
- [ ] **MESH-02**: Chunk-border updates correctly invalidate neighbor meshes to avoid visible seams.
- [ ] **MESH-03**: Renderer integration supports chunk-delta updates so chunk edits do not require full world reupload.

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
| MESH-01 | Phase 3 | Pending |
| MESH-02 | Phase 3 | Pending |
| MESH-03 | Phase 3 | Pending |
| MOVE-01 | Phase 4 | Pending |
| MOVE-02 | Phase 4 | Pending |
| MOVE-03 | Phase 4 | Pending |
| EDIT-01 | Phase 5 | Pending |
| EDIT-02 | Phase 5 | Pending |
| EDIT-03 | Phase 5 | Pending |
| SAVE-01 | Phase 6 | Pending |
| SAVE-02 | Phase 6 | Pending |
| SAVE-03 | Phase 6 | Pending |
| NINT-01 | Phase 7 | Pending |
| NINT-02 | Phase 7 | Pending |
| QUAL-01 | Phase 1 | Complete |

**Coverage:**
- v1 requirements: 21 total
- Mapped to phases: 21
- Unmapped: 0

---
*Requirements defined: 2026-03-15*  
*Last updated: 2026-03-15 after roadmap creation*
