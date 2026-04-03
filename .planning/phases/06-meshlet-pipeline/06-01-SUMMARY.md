---
phase: 06-meshlet-pipeline
plan: 01
type: summary
status: COMPLETE
completed: 2026-03-27
---

# Plan 06-01 Summary: Meshlet Pipeline Foundation (MSHL-01)

## Objective
Add meshopt dependency, define meshlet types, implement meshoptimizer splitting,
create MeshletPool with GPU SSBOs, extend BindlessTable, and update the render
delta pipeline to carry MeshletMesh.

## Tasks Completed

### Task 1: meshopt dependency + MeshletDescriptor/MeshletMesh types
- Added `meshopt = "0.2"` to Cargo.toml `[dependencies]`
- Defined `MeshletDescriptor` (12 fields: offsets, counts, bounding sphere, orientation cone)
- Defined `MeshletMesh` (meshlets, vertices, triangles, aabb_min, aabb_max)
- Added `MeshletMesh::flat_indices()` and `to_packed_mesh()` for legacy compatibility
- Re-exported from `src/meshing/mod.rs`

### Task 2: build_meshlets_from_packed()
- Implemented `unpack_position()` to extract 7-bit x/y/z from PackedVertex word0
- Implemented `build_meshlets_from_packed()`:
  - Constructs `VertexDataAdapter` from unpacked f32 positions (12 bytes stride)
  - Calls `meshopt::build_meshlets()` with max_vertices=64, max_triangles=124, cone_weight=0.5
  - Computes per-meshlet bounding sphere + orientation cone via `meshopt::compute_meshlet_bounds()`
  - Re-indexes PackedVertex data in meshlet-local order
  - Copies u8 triangle indices and AABB from input PackedMesh

### Task 3: GpuMeshlet + MeshletPool
- Defined `GpuMeshlet` (64 bytes, `#[repr(C)]`, Pod+Zeroable):
  - center[3], radius, cone_axis[3], cone_cutoff, vertex/triangle offsets+counts, chunk_slot, lod_level, pad[2]
- Created `MeshletPool` with 6 GPU SSBOs:
  - `meshlet_meta_buffer` (binding 10): GpuMeshlet[]
  - `meshlet_vertex_buffer` (binding 11): PackedVertex[]
  - `meshlet_tri_buffer` (binding 12): u32[] (widened from u8)
  - `visible_meshlet_buffer` (binding 13): u32[] (cull output)
  - `meshlet_indirect_buffer` (binding 14): indirect commands
  - `meshlet_count_buffer` (binding 15): u32 visible count
- Initial capacity: 65536 meshlets, 4M vertices, ~24M triangle indices
- `record_upload()` widens u8 triangle indices to u32, uploads via staging ring
- `record_remove()` clears chunk meshlet range tracking
- Per-chunk range tracked via `HashMap<ChunkKey, (u32, u32)>`
- Changed `RenderDelta::Upsert` payload from `PackedMesh` to `MeshletMesh`
- Added `MeshletPool` to `Renderer` struct with cleanup in `Drop`

### Task 4: BindlessTable extension + MeshSync wiring
- Extended `BindlessTable` BINDING_COUNT from 10 to 16
- Bindings 10-15: STORAGE_BUFFER with COMPUTE stage (binding 11 also VERTEX)
- Descriptor pool: 14 STORAGE_BUFFER + 2 COMBINED_IMAGE_SAMPLER descriptors
- Added `register_meshlet_buffers()` method
- Wired MeshSync: `build_greedy_mesh() -> PackedMesh -> build_meshlets_from_packed() -> MeshletMesh`
- Scheduler produces `RenderDelta::Upsert { key, mesh: MeshletMesh }`
- Legacy `ChunkPool::record_uploads()` uses `MeshletMesh::to_packed_mesh()` bridge
- Extended `GpuPerfCounters` with: `total_meshlets`, `visible_meshlets`, `meshlet_cull_rate`, `meshlet_ssbo_bytes`

## Files Modified
| File | Change |
|------|--------|
| `Cargo.toml` | Added `meshopt = "0.2"` dependency |
| `src/meshing/packing.rs` | MeshletDescriptor, MeshletMesh, build_meshlets_from_packed() |
| `src/meshing/mod.rs` | Re-exports for new types and function |
| `src/renderer/chunk_pool.rs` | GpuMeshlet, MeshletPool with 6 SSBOs |
| `src/renderer/bindless.rs` | 16 bindings, register_meshlet_buffers() |
| `src/renderer/mod.rs` | RenderDelta::Upsert carries MeshletMesh, MeshletPool field |
| `src/renderer/perf_counters.rs` | Meshlet statistics fields |
| `src/runtime/scheduler.rs` | MeshSync produces MeshletMesh via meshlet splitting |
| `tests/phase6_meshlet.rs` | 11 tests covering all tasks |

## Verification
- `cargo test --test phase6_meshlet`: 11/11 passed
- `cargo test`: all tests passed
- `cargo build`: success
- `cargo clippy --all-targets`: zero warnings

## Requirement Coverage
- **MSHL-01**: Fully satisfied. Greedy mesh output is split into meshlets via
  meshoptimizer with precomputed bounding spheres and orientation cones.
  MeshletMesh replaces PackedMesh in the pipeline. MeshletPool manages
  meshlet-granular GPU storage with 6 SSBOs registered in BindlessTable.
