# Plan 06-05 Summary: DAG LOD Generation and GPU LOD Selection

**Status:** COMPLETE
**Date:** 2026-03-28

## What was done

### Task 1: 2-Level DAG LOD Generation (TDD)
- Added `build_lod1()` to `src/meshing/packing.rs`:
  - Groups ~4 adjacent LOD0 meshlets spatially by bounding sphere center
  - Merges group triangles into shared vertex space with boundary vertex detection
  - Simplifies via `meshopt::simplify_with_locks()` (target: 4x reduction)
  - Skips ineffective simplification (<10% reduction)
  - Computes `parent_error` via `meshopt::simplify_scale()` for SSE-based LOD selection
  - Re-splits simplified geometry into LOD1 meshlets via `build_meshlets()`
  - Records `parent_error`, `group_id`, `lod_level` on all meshlets
- Updated `MeshletDescriptor` fields: `lod_level: u8`, `group_id: u32`, `parent_error: f32`
- Small meshes (<4 LOD0 meshlets) skip LOD1 generation
- All 5 LOD tests pass (`phase6_lod_dag_two_levels`, `phase6_lod1_fewer_triangles`, etc.)

### Task 2: GPU LOD Selection and Alpha Dither
- Updated `shaders/meshlet_cull.comp`:
  - GpuMeshlet struct: `pad0` -> `parent_error` (float), `pad1` -> `group_id` (uint)
  - Push constants extended from 32 to 40 bytes (+`sse_threshold`, +`screen_height`)
  - Added `lod_cull()`: SSE-based LOD selection (projected_error vs sse_threshold)
  - LOD0 meshlets culled when parent should render; LOD1 culled when children should render
- Updated `shaders/meshlet.task`:
  - Mirrored all changes from meshlet_cull.comp (LOD selection, push constants, GpuMeshlet fields)
  - LOD culling runs before backface/frustum/hiz culling
- Updated `shaders/meshlet_draw.vert`:
  - Added `v_lod_transition` output (flat float, location 3)
  - Computes LOD transition factor from `parent_error` and camera distance
- Updated `shaders/meshlet_draw.frag`:
  - Added Bayer 8x8 dither matrix (64 entries)
  - Alpha dither: `discard` fragments where alpha < dither threshold
  - Smooth LOD transitions without geometry skirts
- Updated `shaders/meshlet.mesh`:
  - GpuMeshlet struct updated with `parent_error`/`group_id`
  - Push constant offset changed from 32 to 48 (16-byte alignment for mat4)
  - Added `v_lod_transition` per-vertex output with LOD transition computation
- Updated `src/renderer/cull_pipeline.rs`:
  - `MeshletCullPushConstants` extended from 32 to 40 bytes
- Updated `src/renderer/mesh_pipeline.rs`:
  - `MeshShaderPushConstants` updated with `_pad_align` for 16-byte alignment
  - Push constant ranges updated: task 0..40, mesh 48..128
  - Default `sse_threshold: 2.0` and `screen_height` from extent

### Task 3: Skirt Removal and GpuMeshlet Upload
- Disabled border skirt emission in `src/meshing/greedy.rs` (commented out skirt loop)
- LOD DAG alpha dithering replaces geometry skirts for LOD transitions
- Updated `src/renderer/chunk_pool.rs`:
  - `GpuMeshlet` struct: `pad0: u32` -> `parent_error: f32`, `pad1: u32` -> `group_id: u32`
  - `MeshletPool::record_upload` now populates `parent_error` and `group_id` from `MeshletDescriptor`
  - `lod_level` now uses `desc.lod_level` instead of `key.lod_level`
- Updated phase3 skirt tests to expect 0 skirt quads

### Task 4: Statistics and HUD
- Extended `GpuPerfCounters` with `lod0_meshlets`, `lod1_meshlets`, `sse_threshold`
- Added `sse_threshold: f32` field to `Renderer` struct (default 2.0)
- Updated `submit.rs` to pass `sse_threshold` and `screen_height` in push constants
- Added egui "Meshlet Culling" window with:
  - Backface/frustum/hiz culling toggles (checkboxes)
  - Meshlet rendering toggle
  - SSE threshold slider (0.1..16.0 px)
- Extended Debug window with LOD0/LOD1 meshlet counts and cull rate

## Files modified
- `src/meshing/packing.rs` - LOD DAG generation (build_lod1, group_meshlets_spatially)
- `src/meshing/greedy.rs` - Disabled skirt emission
- `src/renderer/cull_pipeline.rs` - Extended push constants to 40 bytes
- `src/renderer/chunk_pool.rs` - GpuMeshlet parent_error/group_id fields
- `src/renderer/mesh_pipeline.rs` - Updated MeshShaderPushConstants alignment
- `src/renderer/submit.rs` - SSE threshold and screen_height wiring
- `src/renderer/mod.rs` - Added sse_threshold field
- `src/renderer/perf_counters.rs` - LOD statistics fields
- `src/app.rs` - Meshlet culling HUD panel and LOD stats
- `shaders/meshlet_cull.comp` - LOD selection in compute cull
- `shaders/meshlet.task` - LOD selection in task shader
- `shaders/meshlet_draw.vert` - LOD transition output
- `shaders/meshlet_draw.frag` - Bayer 8x8 alpha dither
- `shaders/meshlet.mesh` - LOD transition and updated struct
- `tests/phase3_meshing.rs` - Updated skirt test expectations
- `tests/phase6_meshlet.rs` - LOD tests (pre-existing, all pass)

## Verification
- `cargo test --test phase6_meshlet`: 23/23 passed
- `cargo test --test phase3_meshing`: 13/13 passed
- `cargo build`: clean (0 warnings)
- `cargo clippy --all-targets`: clean (0 warnings)
