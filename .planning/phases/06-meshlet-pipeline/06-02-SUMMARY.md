# Plan 06-02 Summary: Per-Meshlet GPU Culling Pipeline (MSHL-02)

**Status:** COMPLETE
**Date:** 2026-03-27

## Objective

Create meshlet_cull.comp with per-meshlet backface+frustum+Hi-Z culling using
subgroup ballot compaction. Build MeshletCullPipeline in Rust. Wire two-level
cascade dispatch (chunk->meshlet) in submit_frame. Validate subgroup features.

## Tasks Completed

### Task 1: Subgroup feature validation (TDD)
- Added `SubgroupInfo` struct to `DeviceContext` with subgroup_size, supported_operations, has_ballot, has_basic
- Query `VkPhysicalDeviceSubgroupProperties` via `get_physical_device_properties2` at device creation
- Logs subgroup info; warns if BALLOT_BIT or BASIC_BIT missing (no hard fail)
- Test: `phase6_subgroup_feature_check` (source grep for SubgroupProperties + BALLOT)

### Task 2: meshlet_cull.comp shader
- Created `shaders/meshlet_cull.comp` with local_size_x=64
- GpuMeshlet GLSL struct matches Rust layout exactly (64 bytes, 16 uint32)
- Three independent culling modes, each gated by push constant u32 toggle:
  - **Backface:** orientation cone test (`dot(cone_axis, to_camera) < cone_cutoff`)
  - **Frustum:** bounding sphere vs 6 planes
  - **Hi-Z:** bounding sphere projection to screen rect, sample depth pyramid at mip level
- Subgroup ballot compaction (D-06): `subgroupBallot` -> `subgroupBallotBitCount` -> `subgroupElect` + `atomicAdd` -> `subgroupBroadcastFirst` -> write `visible_meshlets[]`
- Reads: binding 10 (meshlet meta), binding 4 (frustum planes), binding 6 (Hi-Z config), binding 7 (Hi-Z pyramid)
- Writes: binding 13 (visible meshlet output), binding 15 (meshlet count)
- Updated `build.rs` to target SPIR-V 1.5 / Vulkan 1.2 for subgroup operations

### Task 3: MeshletCullPipeline + two-level cascade
- `MeshletCullPipeline` struct in `cull_pipeline.rs` with `new()`, `record_dispatch()`, `destroy()`
- `MeshletCullPushConstants` (32 bytes): total_meshlet_count, enable_backface, enable_frustum, enable_hiz, camera_pos, _pad
- `record_dispatch`: reset count buffer via `vkCmdFillBuffer`, barrier, bind pipeline+descriptors+push constants, dispatch
- Updated `submit_frame` with two-level cascade:
  1. chunk_cull dispatch (Level 1, existing)
  2. COMPUTE->COMPUTE barrier (chunk writes -> meshlet reads)
  3. meshlet_cull dispatch (Level 2, new) with count buffer reset
  4. COMPUTE->INDIRECT barrier on visible_meshlet_buffer + meshlet_count_buffer
  5. Existing COMPUTE->DRAW_INDIRECT barrier for chunk indirect draw
- Renderer gains `meshlet_cull_pipeline: Option<MeshletCullPipeline>` and 3 culling toggle bools
- Pipeline created in `app.rs` alongside ChunkCullPipeline
- Updated `submit_frame_sequence()` to reflect new stages

## Artifacts
- `shaders/meshlet_cull.comp` - 3-mode GPU culling shader with subgroup ballot
- `src/renderer/cull_pipeline.rs` - MeshletCullPipeline + MeshletCullPushConstants
- `src/renderer/submit.rs` - Two-level cascade dispatch
- `src/renderer/device.rs` - SubgroupInfo + subgroup validation
- `src/renderer/mod.rs` - meshlet_cull_pipeline field + culling toggles
- `src/app.rs` - Pipeline creation
- `build.rs` - SPIR-V 1.5 target + meshlet_cull.comp entry
- `tests/phase6_meshlet.rs` - phase6_subgroup_feature_check

## Verification
- `cargo test` - 172 tests, all passing
- `cargo build` - success (shader compiles with subgroup ops)
- `cargo clippy --all-targets` - zero warnings

## Key Design Decisions
- Two-level cascade is IMPLICIT: meshlet_cull dispatches over ALL meshlets; per-meshlet frustum test naturally eliminates invisible chunks' meshlets (no chunk_visibility_mask SSBO)
- Subgroup ballot: one atomicAdd per subgroup (not per thread) for compaction
- SPIR-V 1.5 target required for subgroup operations (project already requires Vulkan 1.2)
- Camera position passed via push constants (not SSBO extension)
- Degenerate cones (cone_cutoff < -1.0) skip backface test
