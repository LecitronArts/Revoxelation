# Revoxelation

## What This Is

Revoxelation is a Rust voxel engine project targeting a non-Bevy ECS architecture for game prototyping.  
The current codebase already includes a substantial `wgpu` renderer, world generation/storage, and ECS-adjacent runtime pieces; this initialization defines the next focused scope toward a modular, editable, flyable voxel world foundation.

## Core Value

Build a cleanly extensible Rust ECS voxel engine (non-Bevy) where world interaction, especially block edits, is reflected immediately and predictably.

## Requirements

### Validated

- ✅ Rust desktop application scaffold with `winit` loop and logging is in place (`src/main.rs`, `src/app.rs`) — existing
- ✅ `wgpu` renderer bootstrap and multi-pass GPU pipeline architecture already exist (`src/renderer/core/bootstrap/*`, `src/renderer/passes/*`) — existing
- ✅ World/chunk data model with procedural generation and concurrency primitives already exists (`src/world/mod.rs`) — existing
- ✅ Shared host/shader protocol layer and GPU upload pipeline exist (`src/renderer/protocol/*`, `src/renderer/world/*`) — existing
- ✅ `hecs` is already integrated in the codebase for ECS-oriented runtime state (`src/ecs.rs`) — existing

### Active

- [ ] V1 chunk streaming loop centered on player movement with clear ECS/system boundaries
- [ ] Greedy meshing pipeline for chunk surfaces, integrated with render sync
- [ ] Collision-capable player movement with fly mode + gravity mode suitable for gameplay prototyping
- [ ] Block placement/destruction with near-immediate visual feedback after edit
- [ ] Chunk persistence (save/load) so modified world state survives restart
- [ ] Network-ready boundary interfaces/events only (no multiplayer implementation in v1)

### Out of Scope

- Full multiplayer synchronization and replication — deferred; v1 only reserves interfaces
- Migration to Bevy ECS or Bevy engine stack — explicitly excluded by project direction
- Mobile/Web target support — deferred to later milestones; v1 prioritizes desktop
- Aggressive low-level performance tuning before architecture closure — postponed until core loop is stable

## Context

The repository is a brownfield Rust project with significant renderer/world infrastructure already present, including compute-oriented shader passes and a structured renderer module tree.  
The new initialization aligns this existing base with a clearer GSD-driven product direction: a reusable voxel-engine core for future game prototypes, emphasizing modular architecture, predictable system boundaries, and fast iteration.

Quality and delivery process must explicitly use Superpowers quality gates during subsequent phases, including planning, TDD before implementation, systematic debugging when blocked, verification before completion claims, and code-review gates before integration closure.

## Constraints

- **Engine Direction**: Rust + non-Bevy ECS architecture — maintain custom engine boundaries and avoid Bevy dependency
- **ECS Choice**: Use `hecs` for v1 execution model — optimize for delivery velocity and clear system decomposition
- **Rendering**: `wgpu` backend for v1 — consistent cross-platform desktop GPU abstraction
- **Platforms**: Windows + Linux first — no mobile/web commitments in this cycle
- **Runtime Architecture**: Mixed scheduling model — fixed main stages plus event-driven/local async subsystems
- **Heavy Work Placement**: Chunk generation/meshing done via background job queues — keep frame loop responsive
- **Performance Strategy**: Stability and correctness first; optimize after core architectural closure
- **Quality Workflow**: Superpowers skills/gates are mandatory and cannot be skipped for speed

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Non-Bevy Rust ECS engine | Full control over boundaries and data flow for voxel-specific architecture | — Pending |
| `hecs` as v1 ECS base | Faster delivery than full custom ECS while preserving non-Bevy direction | — Pending |
| `wgpu` as rendering backend | Strong Rust ecosystem support and cross-platform desktop target fit | — Pending |
| V1 excludes multiplayer implementation (interfaces only) | Reduces scope risk while preserving future expansion path | — Pending |
| Mixed scheduler (stages + events) | Balances debuggability and modular extensibility | — Pending |
| Async job queues for chunk/mesh heavy work | Reduces frame stalls and supports streaming workloads | — Pending |
| Block edit feedback must be near-immediate | Core interaction quality requirement for voxel gameplay iteration | — Pending |
| Superpowers quality gates are mandatory | Improves execution quality and verification discipline | — Pending |

---
*Last updated: 2026-03-15 after initialization*
