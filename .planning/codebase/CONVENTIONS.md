# Revoxelation Coding Conventions

## Scope and language profile
- The codebase is Rust-only and centered in `src/` with binary entry in `src/main.rs`.
- The crate currently uses Rust `edition = "2024"` in `Cargo.toml`.
- Dependencies suggest a real-time renderer architecture (`wgpu`, `winit`, `egui`, `anyhow`, `log`).

## Module and file organization
- Top-level modules are declared in `src/main.rs` as `mod app; mod ecs; mod renderer; mod world;`.
- Public renderer surface is organized via `src/renderer/mod.rs`:
  - public submodules: `camera`, `core`, `lifecycle`, `protocol`, `resources`, `world`
  - internal-only modules: `light_sampler`, `passes`, `reservoir`
- Re-export pattern is used to keep public API stable:
  - `src/renderer/mod.rs` re-exports `Renderer`, `RendererSettings`, diagnostics/event types.
  - `src/renderer/core/renderer.rs` also re-exports from `state` for local consistency.

## Naming conventions
- Types use `PascalCase`: `RendererSettings`, `VoxelWorld`, `WorldSyncRejection`, `FramePlan`.
- Functions and fields use `snake_case`: `prepare_world_sync`, `generation_snapshot`, `svgf_passes`.
- Constants use `SCREAMING_SNAKE_CASE`: `SVGF_MAX_ATROUS_PASSES`, `DEBUG_OVERLAY_MODE_MAX`.
- GPU transfer structs are consistently suffixed with `Gpu` in `src/renderer/protocol/mod.rs` and `src/renderer/protocol/types.rs` (`CameraGpu`, `TracerUniform`, `SvgfDiagStatsGpu`).
- Acronyms are stylized intentionally by domain type names (`ReSTIRPass`, `SvgfPass`), while methods stay snake_case.

## API visibility and ownership conventions
- `pub(crate)` is preferred for internal cross-module sharing (`src/renderer/core/state.rs`).
- `pub(super)` is used for bootstrap internals (`src/renderer/core/bootstrap/device_setup.rs`).
- Plain `pub` is reserved for external-facing runtime surface (`Renderer`, `RendererSettings`, world types).

## Error handling conventions
- Application/bootstrap paths use `anyhow::Result` for ergonomic propagation:
  - `src/app.rs` -> `pub fn run() -> Result<()>`
  - `src/renderer/core/renderer.rs` -> `Renderer::new(...) -> Result<Self>`
  - `src/renderer/core/bootstrap/device_setup.rs` uses `.context(...)` and `bail!(...)`.
- Domain failures use typed errors instead of `anyhow`:
  - `src/renderer/world/sync.rs` returns `Result<PreparedWorldSync, WorldSyncRejection>`.
  - Rejection bundles structured detail (`issues: Vec<String>`) plus a short user-facing `reason`.
- Runtime resilience is favored over panics:
  - surface errors are matched and recovered in `src/app.rs` (`Lost/Outdated` -> `reconfigure`, `OutOfMemory` -> exit).
  - input/state counters use saturating arithmetic (`saturating_add`) in `src/renderer/world/sync.rs`.
- Assertions are used as invariants:
  - `assert!` for required runtime preconditions in pass `prepare()` methods (`src/renderer/passes/*.rs`).
  - `debug_assert!` for development-only sanity checks (`src/renderer/core/frame_exec.rs`, `src/renderer/core/bootstrap/device_setup.rs`).
- `unwrap()/expect()` are concentrated in tests and known invariants, not general runtime control flow.

## Data validation and bounds strategy
- Settings sanitization is centralized and explicit in `src/renderer/core/renderer.rs` (`sanitize_renderer_settings` with `.clamp(...)`).
- Zero/invalid dimensions are clamped before resource use:
  - `src/renderer/core/frame_plan.rs` enforces minimum resolution `[1, 1]`.
  - `src/renderer/core/bootstrap/device_setup.rs` and `src/renderer/core/world_ops.rs` clamp render extents.
- Buffer-size safety checks are explicit and descriptive in `src/renderer/world/sync.rs` (`check_storage_slice_limit`).

## Practical conventions to follow for new code
- Put pure logic next to its tests in the same file under `#[cfg(test)] mod tests`.
- Prefer small, composable helper functions for diagnostics/state transitions, then test them directly.
- Use typed error structs when callers need machine-readable failure details; use `anyhow` for top-level orchestration.
- Keep new public renderer API surfaced through `src/renderer/mod.rs` re-exports instead of deep module paths.
- For GPU protocol changes, update both constants/layout logic and related tests in `src/renderer/protocol/mod.rs` and `src/renderer/protocol/bindings.rs`.
