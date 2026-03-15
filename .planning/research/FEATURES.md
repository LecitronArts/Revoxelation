# Revoxelation V1 Feature Set

Aligned to `.planning/PROJECT.md` (updated 2026-03-15): a non-Bevy Rust voxel engine focused on a modular, editable, flyable world foundation for prototyping.

## Complexity Scale

- `S`: small (localized change, low coordination)
- `M`: medium (cross-module integration)
- `L`: large (multi-system coordination and state flow)
- `XL`: very large (architecture-shaping and high verification cost)

## Table Stakes

These are required for V1 to be considered usable for gameplay prototyping.

| Feature | Complexity | Depends On | Notes |
|---|---|---|---|
| ECS-driven runtime loop with clear system boundaries | `L` | App lifecycle, hecs integration | Foundation for deterministic staging and maintainability. |
| Player-centered chunk streaming (load/unload around movement) | `L` | ECS runtime loop, world/chunk model | Must keep active world coherent while moving. |
| Background job queues for chunk generation/meshing | `L` | Chunk streaming, scheduler boundaries | Keeps frame loop responsive under world churn. |
| Greedy meshing pipeline integrated with render sync | `XL` | Chunk streaming, job queues, renderer protocol/upload path | Converts voxel data to efficient renderable geometry. |
| Collision-capable movement with fly mode and gravity mode | `L` | ECS runtime loop, chunk spatial queries | Enables both debugging traversal and gameplay-like motion. |
| Block placement/destruction with near-immediate visual feedback | `XL` | Movement/raycast interaction, meshing pipeline, render sync | Core interaction quality target for the project. |
| Chunk persistence (save/load of modified world state) | `L` | Chunk model stability, edit events, streaming lifecycle | Required so edits survive restart. |
| Network-ready boundary interfaces/events (no multiplayer impl) | `M` | ECS events, edit/movement actions, chunk lifecycle events | Stabilizes future expansion path without building netcode now. |

## Differentiators

These distinguish Revoxelation from a basic voxel sandbox prototype.

| Feature | Complexity | Depends On | Why It Differentiates |
|---|---|---|---|
| Non-Bevy, explicit modular engine boundaries | `L` | ECS/runtime architecture discipline | Prioritizes control and long-term extensibility over framework lock-in. |
| Mixed scheduling model (fixed stages + event-driven/async subsystems) | `L` | Runtime loop, background jobs, event contracts | Balances predictability with responsiveness to heavy world workloads. |
| Fast edit-to-visual loop as a first-class quality target | `XL` | Block edit pipeline, meshing, renderer upload/sync | Emphasizes iteration speed for gameplay experimentation. |
| Future-ready interface contracts without scope creep | `M` | Stable event surface for world/player/edits | Allows later multiplayer/tooling integration with lower refactor risk. |

## Anti-Features

These are intentionally excluded from V1 to protect delivery focus.

| Anti-Feature | Why Excluded | Blocked By / Dependency Context |
|---|---|---|
| Full multiplayer replication/synchronization | Out of scope for V1; interfaces only | Depends on validated single-player authority model and stable network protocol design. |
| Migration to Bevy ECS/engine stack | Conflicts with project direction | Would invalidate current architecture goals and existing module boundaries. |
| Mobile/Web deployment targets | Deferred beyond V1 desktop focus | Depends on platform abstraction hardening and GPU/path compatibility work. |
| Premature deep performance tuning | Deferred until architecture closure | Depends on stable system boundaries and representative profiling baselines. |

## Dependency Backbone (Execution Order)

Use this ordering when planning implementation phases:

1. ECS runtime loop and system boundaries.
2. Chunk streaming lifecycle around player position.
3. Background job queues for generation and meshing.
4. Greedy meshing plus renderer synchronization.
5. Movement/collision modes (fly + gravity) using streamed chunk queries.
6. Block placement/destruction with immediate mesh/render invalidation path.
7. Persistence integration into chunk load/unload and edit events.
8. Network-ready boundary events finalized after edit/movement/chunk flows stabilize.
