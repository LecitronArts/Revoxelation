# Revoxelation V1 Research Summary

## Recommended Stack Highlights
- Keep delivery and modernization split: keep current renderer stack pinned during core V1 feature delivery, then run a dedicated migration phase for `wgpu` + `winit` + `egui` together.
- Keep non-Bevy ECS direction with `hecs` as orchestration layer, not bulk voxel storage.
- Standardize world concurrency on bounded worker queues (`rayon` + `crossbeam-channel`) with cancellation and backpressure.
- Use typed domain errors (`thiserror`) inside subsystems and `anyhow` at app boundaries.
- Adopt `tracing` for chunk lifecycle, meshing, edit, and persistence observability.
- Persist chunks with versioned binary schema (`serde` + `bincode`), optional compression, and checksum metadata.
- Add stabilization tooling in CI (`cargo-nextest`, `cargo-deny`, `cargo-audit`) after core dataflow is stable.

## Table-Stakes Features for V1
- ECS-driven runtime loop with explicit stage boundaries.
- Player-centered chunk streaming with deterministic load/unload lifecycle.
- Background generation and meshing job queues to keep frame thread responsive.
- Greedy meshing integrated with incremental render synchronization.
- Collision-capable movement supporting fly and gravity modes.
- Block place/destroy path with near-immediate visual feedback.
- Chunk persistence for edited world state across restarts.
- Network-ready event/contracts at boundaries (without implementing multiplayer).

## Key Architecture Decisions
- Keep world state authoritative on main thread; workers do CPU transforms only.
- Use explicit chunk lifecycle states and monotonic revision IDs for generation, meshing, upload, and persistence.
- Use fixed ECS stage order so collision and edits can feed render sync predictably within frame budgets.
- Separate world domain, streaming/jobs, meshing, renderer sync, edit pipeline, and persistence into narrow modules.
- Move renderer integration to chunk-delta apply path; keep full sync only as bootstrap/fallback.
- Apply block edits locally and authoritatively first, then enqueue remesh/persistence side effects.
- Keep event contracts deterministic and serializable so later networking can map onto stable local behavior.

## Top Pitfalls and Mitigations
- Streaming thrash at high movement speeds: use hysteresis radii, prioritized/cancelable jobs, and per-frame mutation budgets.
- Cross-thread chunk races: enforce state machine transitions with revision checks before mesh/upload commit.
- Chunk-border meshing seams: use neighbor-aware visibility queries and multi-chunk invalidation on border edits.
- Remesh storms from frequent edits: coalesce edits, track dirty regions, and budget remesh/upload work per frame.
- Collision mismatches and tunneling: use one canonical voxel source, swept AABB, and delta-time clamp/substeps.
- Persistence corruption or drift: atomic temp-write-rename, revision/checksum metadata, and explicit schema migration tests.
- Renderer instability under churn: frame-stable world snapshots, explicit sync points, and staged GPU buffer swaps.

## Suggested Phase Ordering Hints
1. Establish ECS stage skeleton and explicit event/command boundaries.
2. Implement chunk lifecycle plus streaming planner and active-set diff.
3. Add bounded background job infrastructure for generation and meshing.
4. Implement greedy meshing and incremental renderer chunk-delta sync.
5. Add movement/collision on chunk-backed spatial queries.
6. Add block edit pipeline with high-priority remesh invalidation.
7. Integrate persistence into activation/unload/checkpoint flow.
8. Harden event contracts and add end-to-end integration tests.
9. Run dedicated renderer stack modernization phase after V1 architecture is stable.

## Roadmap Use Note
- Keep phase scope tight: avoid combining renderer API migration with core gameplay foundation phases.