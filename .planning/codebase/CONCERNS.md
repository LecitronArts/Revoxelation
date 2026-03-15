# CONCERNS

## Scope
- Focus: technical debt, reliability, security, performance, and fragility risks observed in the current codebase.
- Source scope: Rust host code and WGSL shaders under `src/`.

## High-Risk Concerns
1. Busy render loop can pin CPU/GPU and battery usage.
- Evidence: `ControlFlow::Poll` plus unconditional redraw request in `Event::AboutToWait` in `src/app.rs`.
- Risk: high idle power usage, poor laptop thermals, unstable frame pacing on weaker systems.
- Note: this is a design choice today, but it is operationally expensive.

2. World sync path rebuilds and reuploads everything instead of diffing.
- Evidence: full payload rebuild in `src/renderer/world/payload_builder.rs`, then full GPU resource recreation in `src/renderer/world/upload.rs` and apply in `src/renderer/core/world_ops.rs`.
- Risk: spikes/stutter when world changes, poor scaling as chunk count grows.

3. Chunk-map degradation is non-fatal and can silently drop world entries.
- Evidence: dropped-entry warning then continue in `src/renderer/world/payload_builder.rs` (`chunk_map_dropped_entries`).
- Risk: rendering holes/inconsistent world sampling instead of a hard failure path.

4. Unbounded generation thread creation pattern under repeated regen.
- Evidence: each `spawn_generation` starts a new thread in `src/world/mod.rs` (`std::thread::spawn`) and uses rayon inside it.
- Risk: thread churn and contention during repeated generation requests.

## Reliability / Fragility
1. Runtime `assert!` guards in pass prepare paths can crash the app if invariants break.
- Evidence: `assert!` in `src/renderer/passes/trace.rs`, `src/renderer/passes/reistir.rs`, `src/renderer/passes/svgf.rs`.
- Risk: hard process abort in production rather than recoverable error propagation.

2. Lifecycle execution result is ignored.
- Evidence: `let _ = execute_renderer_lifecycle(...)` in `src/renderer/core/world_ops.rs`.
- Risk: lifecycle anomalies are harder to diagnose and cannot be surfaced to caller policy.

3. SVGF diagnostics readback failures are effectively silent.
- Evidence: channel send/poll failures are consumed in `src/renderer/core/frame_exec.rs` without explicit logging.
- Risk: false confidence in diagnostics; degraded observability when GPU readback breaks.

4. Shader storage format selection has permissive fallback behavior.
- Evidence: fallback branch in `storage_format_token` in `src/renderer/core/bootstrap/shader_modules.rs`.
- Risk: future format expansion could silently choose an unintended shader token.

5. Integrity check exists but is not enforced.
- Evidence: `_chunk_coord_match` computed then unused in `src/renderer/world/payload_builder.rs`.
- Risk: latent data consistency bug can pass unnoticed if chunk metadata diverges.

## Performance Debt
1. Large CPU allocations during resource rebuilds.
- Evidence: zero-filled vectors used to initialize storage buffers in `src/renderer/resources/restir_storage.rs`.
- Risk: allocation spikes and extra memory bandwidth on resize/rebuild paths.

2. 3D importance texture upload uses full CPU staging copy.
- Evidence: full `Vec<u8>` staging and nested copy loops in `src/renderer/world/upload.rs`.
- Risk: costly world-sync uploads as light importance data grows.

3. Per-frame `device.poll` for diagnostics readback.
- Evidence: `device.poll(wgpu::Maintain::Poll)` in `src/renderer/core/frame_exec.rs`.
- Risk: avoidable CPU overhead and potential frame jitter on some drivers.

## Security Notes
1. Runtime attack surface is currently low.
- Evidence: no network/socket/input parsing subsystem in `src/`; app is local GPU rendering.

2. Supply-chain and hardening process is thin.
- Evidence: dependencies in `Cargo.toml` but no visible CI/security automation (`.github` absent in repo tree).
- Risk: delayed detection of vulnerable crate versions or regressions.

## Maintainability / Technical Debt
1. High-complexity, monolithic files increase change risk.
- Evidence: `src/shaders/trace.wgsl` (~1390 lines), `src/shaders/svgf.wgsl` (~801 lines), `src/renderer/core/frame_exec.rs` (~1115 lines), `src/app.rs` (~556 lines).
- Risk: fragile edits, slower reviews, higher regression probability.

2. Host/shader contract is manually mirrored across many files.
- Evidence: protocol and binding definitions in `src/renderer/protocol/*.rs`, layouts in `src/renderer/core/bootstrap/pipeline_layouts.rs`, usage across WGSL files in `src/shaders/*.wgsl`.
- Risk: subtle drift bugs despite unit tests when adding/changing fields.

3. Unused or partially integrated code paths remain in-tree.
- Evidence: `#![allow(dead_code)]` in `src/renderer/light_sampler.rs` and `src/renderer/reservoir.rs`.
- Risk: stale logic diverges from production path and confuses future refactors.

## Practical Mitigation Priorities
1. Add a frame pacing mode (event-driven or target FPS sleep) in `src/app.rs`.
2. Introduce incremental world upload and chunk-delta updates in `src/renderer/world/*`.
3. Convert pass `assert!` checks to recoverable diagnostics/errors in `src/renderer/passes/*`.
4. Add integration smoke tests for renderer bootstrap + one frame render path around `src/renderer/core/*`.
5. Add CI with `cargo test`, `cargo clippy`, and dependency audit to protect `Cargo.toml` updates.
