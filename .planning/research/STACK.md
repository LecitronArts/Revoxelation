# Revoxelation V1 Stack Research (2026-03-15)

## Scope and Inputs

This stack recommendation is tailored to the current repository state and project direction:

- Source context: `.planning/PROJECT.md` and `.planning/codebase/*.md`
- Current dependency baseline: `Cargo.toml` (`wgpu 0.20`, `winit 0.29`, `egui 0.28`, `hecs 0.10`)
- Product constraints: Rust, desktop-first, non-Bevy ECS, fast edit-to-visual loop, stable architecture before deep optimization
- Ecosystem snapshot date: 2026-03-15 (crates.io + Rust stable channel metadata)

## Executive Recommendation

Use a two-track strategy for roadmap planning:

1. Delivery track (near term): keep the existing rendering stack versions while implementing core V1 systems (streaming, meshing, edits, persistence).
2. Modernization track (scheduled phase): upgrade rendering/window/UI trio together (`wgpu` + `winit` + `egui`) after V1 dataflow boundaries are stable.

Reason: this repo already has substantial renderer integration and custom protocol code; big API migrations now would compete directly with core V1 feature delivery.

## Recommended Stack (Version Bands, Why, Confidence)

Confidence scale:
- High: strong fit with current code and low architectural risk
- Medium: good fit but non-trivial integration or migration cost
- Low: viable but notable uncertainty or maintenance risk

| Area | Recommendation (2026 band) | Why this choice for Revoxelation | Confidence | Practical notes |
|---|---|---|---|---|
| Rust toolchain | `rustc stable 1.94.x` (pin with `rust-toolchain.toml`) | Edition 2024 project; pinning reduces CI and contributor drift | High | Keep MSRV policy explicit in docs; avoid silent compiler drift between dev machines |
| Build workflow | Cargo native + `cargo-nextest 0.9.x` + `cargo-deny 0.19.x` + `cargo-audit 0.22.x` | Matches current Rust-only repo and closes supply-chain/testing gaps called out in concerns | High | Add as CI phase after core V1 loop is stable |
| GPU backend | Target `wgpu 28.x` (modernization phase), keep `0.20` during core V1 buildout | Existing renderer is deep and custom; upgrade is valuable but expensive | Medium | Do not mix feature work with major `wgpu` migration in same phase |
| Window/event loop | Target `winit 0.30.x` with `wgpu 28.x` | Must move in lockstep with renderer stack updates | Medium | Plan as paired migration with render bootstrap updates |
| Debug UI | Target `egui`/`egui-wgpu`/`egui-winit` `0.33.x` together | Current project already uses egui overlay; keep trio aligned by minor version | Medium | Upgrade all three crates together only |
| ECS runtime | `hecs 0.11.x` | Non-Bevy requirement plus current integration in `src/ecs.rs`; minimal disruption path | High | Keep ECS as orchestration layer; avoid storing bulky chunk voxel arrays as components |
| Math | `glam 0.32.x` with `bytemuck` feature | Already used in camera/protocol paths; strong ecosystem compatibility | High | Upgrade opportunistically with rendering stack changes |
| World concurrency | `rayon 1.11.x` + `crossbeam-channel 0.5.x` | Matches existing generation model and supports bounded worker queues for meshing/streaming | High | Prefer explicit queue backpressure and cancellation over ad-hoc thread spawning |
| Concurrent maps | Stay on `dashmap 6.1.x` until `7.x` is stable (non-RC) | Current code uses DashMap heavily; `7.0.0-rc2` indicates transition period | High | Treat `7.x` adoption as deliberate migration task, not incidental dependency bump |
| Error model | `anyhow 1.0.x` at app boundaries + `thiserror 2.0.x` for domain errors | Matches existing typed rejection patterns and top-level orchestration style | High | Keep user-facing failure reasons typed in world sync and persistence paths |
| Logging/telemetry | `tracing 0.1.x` + `tracing-subscriber 0.3.x` (+ optional `tracing-appender 0.2.x`) | Current `log/env_logger` is usable but weak for subsystem correlation | Medium | Migrate incrementally: bridge from `log` first, then add spans on streaming/meshing/edit paths |
| Meshing helper | Evaluate `block-mesh 0.2.x` as reference/prototype only | Useful for quick validation, but crate activity is old | Low | Prefer owning final greedy meshing core in-repo for long-term control |
| Collision queries | Prefer `parry3d 0.26.x` (query-focused) before full `rapier3d` adoption | V1 needs collision and movement, not full rigid-body simulation | Medium | Start with voxel query + swept AABB; add Parry only where it materially lowers bug risk |
| Persistence serialization | `serde 1.0.x` + `bincode 3.0.x` + optional `zstd 0.13.x` | Fast binary path suitable for chunk snapshots and versioned schema | Medium | Enforce explicit chunk schema version + checksum metadata from day one |
| Property/perf tests | `proptest 1.10.x` + `criterion 0.8.x` | Aligns with deterministic world logic and revision-state invariants | High | Use property tests for chunk lifecycle/state machine and edit ordering |

## Recommended Additions by Roadmap Phase

| Roadmap focus | Stack additions that should happen in that phase |
|---|---|
| Streaming core + job queues | `crossbeam-channel`, `thiserror`, `tracing` baseline |
| Greedy meshing + render sync | optional `block-mesh` prototyping, then in-repo mesher; start perf probes |
| Movement + collision | `parry3d` only if custom swept-AABB path becomes costly/fragile |
| Edit latency hardening | richer `tracing` spans/events, property tests for event ordering |
| Persistence | `serde` + `bincode` + checksum/compression policy |
| Stabilization | `cargo-nextest`, `cargo-deny`, `cargo-audit`, CI enforcement |
| Modernization phase | coordinated `wgpu` + `winit` + `egui` family upgrade |

## What to Avoid (Actionable)

1. Avoid introducing `bevy_ecs` for V1.
- Reason: directly conflicts with project direction and existing custom renderer/world boundaries.

2. Avoid older/stale ECS alternatives (`legion`, `specs`) for new core work.
- Reason: lower momentum and no practical advantage over current `hecs` path for this repo.

3. Avoid adopting release-candidate dependencies in core paths (for now).
- Example: `dashmap 7.0.0-rc2`.
- Reason: unnecessary risk during architecture-shaping phases.

4. Avoid adding a full async runtime (Tokio) to the frame loop unless a clear blocking need appears.
- Reason: current model is frame-loop + worker queues; runtime mixing adds lifecycle complexity quickly.

5. Avoid full-world rebuild/reupload as the long-term renderer sync model.
- Reason: already identified as major scaling/perf risk in codebase concerns.

6. Avoid JSON for hot-path chunk persistence.
- Reason: binary formats are better for throughput, file size, and deterministic schema handling.

7. Avoid a big-bang rendering stack migration while core V1 features are still landing.
- Reason: high regression risk and diluted delivery focus.

## Practical Migration Notes from Current `Cargo.toml`

Current baseline (repo):
- `wgpu 0.20`, `winit 0.29`, `egui 0.28`, `egui-wgpu 0.28`, `egui-winit 0.28`, `hecs 0.10`, `dashmap 6.1`

Planned sequence:
1. Keep rendering trio pinned during streaming/meshing/edit/persistence architecture work.
2. Upgrade low-risk crates early (`hecs`, `glam`, `rayon`, `bytemuck`, `anyhow`) with targeted tests.
3. Schedule a dedicated render-stack migration phase for `wgpu/winit/egui` with no other feature scope.
4. Land frame pacing and incremental world sync before aggressive rendering optimizations.

## Confidence Summary

- High confidence: `hecs`, `rayon`, `glam`, error model split (`anyhow` + `thiserror`), CI hardening tools.
- Medium confidence: render stack timing (`wgpu/winit/egui` upgrade sequencing), persistence codec choice (`bincode 3`), `parry3d` adoption threshold.
- Low confidence: using `block-mesh` as a long-term dependency (good for prototyping, weak for long-term ownership confidence).

## Research Snapshot Sources

- Rust stable channel manifest: `https://static.rust-lang.org/dist/channel-rust-stable.toml`
- Crate registry metadata API:
  - `https://crates.io/api/v1/crates/wgpu`
  - `https://crates.io/api/v1/crates/winit`
  - `https://crates.io/api/v1/crates/egui`
  - `https://crates.io/api/v1/crates/hecs`
  - `https://crates.io/api/v1/crates/dashmap`
  - `https://crates.io/api/v1/crates/rayon`
  - `https://crates.io/api/v1/crates/tracing`
  - `https://crates.io/api/v1/crates/parry3d`
  - `https://crates.io/api/v1/crates/rapier3d`
  - `https://crates.io/api/v1/crates/block-mesh`
  - `https://crates.io/api/v1/crates/bincode`
  - `https://crates.io/api/v1/crates/zstd`

