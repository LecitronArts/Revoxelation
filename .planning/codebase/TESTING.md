# Revoxelation Testing Patterns

## Current test setup
- Tests are unit-style and colocated with implementation under `#[cfg(test)] mod tests`.
- There is currently no `tests/` integration-test directory and no `benches/` directory.
- `Cargo.toml` has no separate `[dev-dependencies]`; tests rely on normal crate dependencies.
- Core command surface is standard Cargo:
  - `cargo test` to run full suite.
  - `cargo test <module_or_test_name>` for targeted iteration.

## Where tests live (path-grounded map)
- World generation/cancellation: `src/world/mod.rs`.
- Frame planning and render diagnostics: `src/renderer/core/frame_plan.rs`, `src/renderer/core/frame_exec.rs`, `src/renderer/core/state.rs`.
- Renderer settings sanitization: `src/renderer/core/renderer.rs`.
- Lifecycle planning/execution: `src/renderer/lifecycle/plan.rs`, `src/renderer/lifecycle/policy.rs`, `src/renderer/lifecycle/executor.rs`, `src/renderer/core/world_ops.rs`.
- Protocol and binding contracts: `src/renderer/protocol/mod.rs`, `src/renderer/protocol/bindings.rs`, `src/renderer/core/bootstrap/pipeline_layouts.rs`.
- Resource and upload logic: `src/renderer/resources/surface.rs`, `src/renderer/resources/context.rs`, `src/renderer/resources/bind_groups.rs`, `src/renderer/world/upload.rs`, `src/renderer/world/payload_builder.rs`, `src/renderer/world/sync.rs`.

## Test naming and style conventions
- Test names are descriptive sentence-like snake_case, documenting intent:
  - `frame_plan_clamps_resolution_to_non_zero` in `src/renderer/core/frame_plan.rs`.
  - `sync_rejection_updates_count_and_reason` in `src/renderer/world/sync.rs`.
  - `svgf_diag_sample_interval_is_clamped_to_supported_range` in `src/renderer/core/renderer.rs`.
- Assertions emphasize exact behavior (`assert_eq!`) and invariant conditions (`assert!`).
- Lightweight helper functions inside test modules reduce repetition:
  - `wait_generation_finished(...)` in `src/world/mod.rs`.
  - `lifecycle_trace(...)` in `src/renderer/core/world_ops.rs`.

## What is being tested today
- Boundary and clamp behavior for user settings and dimensions:
  - `sanitize_renderer_settings` tests in `src/renderer/core/renderer.rs`.
  - non-zero resolution enforcement in `src/renderer/core/frame_plan.rs`.
- Deterministic state transitions and event traces:
  - lifecycle decision/trace tests in `src/renderer/lifecycle/*.rs` and `src/renderer/core/world_ops.rs`.
  - ring-buffer/event querying behavior in `src/renderer/core/state.rs`.
- GPU protocol correctness and layout stability:
  - size/alignment tests with `size_of`/`align_of` in `src/renderer/protocol/mod.rs`.
  - contiguous/unique binding index checks in `src/renderer/protocol/bindings.rs`.
- Safety checks for world upload/sync limits:
  - oversized payload rejection and reason formatting in `src/renderer/world/sync.rs`.
  - metadata and map statistics checks in `src/renderer/world/upload.rs` and `src/renderer/world/payload_builder.rs`.

## Error-path testing patterns
- Invalid/overflow inputs are tested explicitly:
  - overflow branch for remap sizing in `src/renderer/world/sync.rs`.
  - out-of-range setting values in `src/renderer/core/renderer.rs`.
- Reject/success bookkeeping is verified via dedicated state-update helpers:
  - `record_world_sync_rejection_state` and `record_world_sync_success_state` in `src/renderer/world/sync.rs`.
- `expect(...)`/`unwrap(...)` appear in tests for known-good setup, not as fuzzy assertions.

## Practical guidance for adding tests
- Add tests in the same file as the function/struct being changed unless true integration coverage is required.
- Prefer focused tests against pure helpers first (`summarize_*`, `should_*`, `*_slot_*`) before testing larger orchestration paths.
- When changing GPU protocol fields/bindings, always update layout/order tests in:
  - `src/renderer/protocol/mod.rs`
  - `src/renderer/protocol/bindings.rs`
  - `src/renderer/core/bootstrap/pipeline_layouts.rs` (if pipeline layout expectations change)
- When adding new configurable settings, mirror existing clamp tests in `src/renderer/core/renderer.rs`.
- When adding new rejection paths, assert both machine-readable details (`issues`) and human summary strings (`reason`) as done in `src/renderer/world/sync.rs`.
