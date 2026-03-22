# Revoxelation Testing Patterns

## Current test setup
- The repository uses both integration tests under `tests/` and inline unit tests under `src/**`.
- `Cargo.toml` includes `serde_json` as a dev dependency for serialization-contract tests.
- Core command surface is standard Cargo:
  - `cargo test` to run the full suite
  - `cargo test --test phase25_vulkan` for Vulkan compile checks
  - `cargo test --test phase2_streaming` for scheduler/streaming round trips
  - `cargo test --test phase3_meshing` for meshing and render-bridge regressions

## Where tests live (path-grounded map)
- Phase 1 runtime contracts:
  - `tests/phase1_events.rs`
  - `tests/phase1_stage_order.rs`
  - `tests/phase1_registration_boundaries.rs`
  - `tests/phase1_observability.rs`
  - `tests/phase1_quality_gates.rs`
- Phase 2 streaming:
  - `tests/phase2_streaming.rs`
- Phase 2.5 Vulkan bootstrap:
  - `tests/phase25_vulkan.rs`
- Phase 3 meshing and render-delta integration:
  - `tests/phase3_meshing.rs`
- Inline unit tests:
  - `src/streaming/state_store.rs`
  - `src/streaming/sse.rs`
  - `src/streaming/octree.rs`
  - `src/streaming/job_queue.rs`
  - `src/streaming/job_runner.rs`
  - `src/runtime/scheduler.rs`
  - `src/runtime/boundaries/world.rs`
  - `src/runtime/boundaries/meshing.rs`

## Test naming and style conventions
- Integration tests are grouped by phase/regression scope rather than by module name.
- Individual test names are descriptive snake_case sentences such as:
  - `full_round_trip`
  - `renderer_module_types_compile`
  - `mesh_01_chunk_voxels_contract_and_packed_layout`
- Assertions prefer exact contract checks (`assert_eq!`) and explicit invariants (`assert!`) over loose smoke checks.

## What is being tested today
- Runtime orchestration contracts:
  - stage order and trace sequencing
  - event and command serialization round trips
  - boundary registration isolation
  - overlay/observability summaries
- Streaming behavior:
  - active-set and queue behavior
  - job-runner cancellation/round-trip flow
  - state-transition contracts in `ChunkStateStore`
- Vulkan surface/API exposure:
  - compile-only existence of renderer/bootstrap types and functions
  - required-device-feature error strings
- Meshing and render bridge:
  - typed voxel payload contract
  - packed vertex layout
  - greedy meshing with halo neighbors and skirts
  - chunk slot reuse and render-delta cleanup behavior

## Error-path and stateful-test patterns
- Runtime and streaming tests frequently assert exact state names, transition results, or emitted deltas instead of only checking for "no panic".
- Serialization tests verify both encode and decode paths using `serde_json`.
- Because scheduler state uses `OnceLock`, integration tests reserve distinct frame-index ranges to avoid collisions across files.
- Compile-check tests intentionally avoid creating a live Vulkan instance so they can run in environments without a GPU-backed display surface.

## Practical guidance for adding tests
- Add integration tests under `tests/` when the behavior crosses runtime/streaming/renderer boundaries.
- Add inline tests for pure helpers and state machines inside `src/streaming/**`, `src/runtime/**`, or `src/meshing/**`.
- When changing Vulkan bootstrap signatures or renderer public types, update `tests/phase25_vulkan.rs`.
- When changing chunk payload, dirty tracking, or slot-allocation contracts, update `tests/phase3_meshing.rs`.
- When changing stage order, command/event schemas, or boundary rules, update the relevant `tests/phase1_*.rs` files.
