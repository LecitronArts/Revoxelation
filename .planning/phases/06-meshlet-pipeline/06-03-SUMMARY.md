---
phase: 06-meshlet-pipeline
plan: 03
status: COMPLETE
completed: 2026-03-27
---

# Plan 06-03 Summary: Meshlet Draw Pipeline (MSHL-03)

## What Was Done

### Task 1: Create meshlet_draw.vert and meshlet_draw.frag shaders
- **meshlet_draw.vert**: Reads PackedVertex from VB binding 0. Uses `gl_DrawID` (via `GL_ARB_shader_draw_parameters`) to index into `visible_meshlet_buffer` (binding 13) to get `meshlet_id`, then reads `GpuMeshlet.chunk_slot` from binding 10, then reads `GpuChunkInstance` from binding 0. Same position decode logic as chunk_mesh.vert.
- **meshlet_draw.frag**: Identical to chunk_mesh.frag (face-based coloring via material SSBO + texture array).
- **build.rs**: Updated shader_sources from 7 to 9 entries to include both new shaders.
- **bindless.rs**: Updated bindings 10 (meshlet_meta) and 13 (visible_meshlet) to include `VERTEX` stage flag so the vertex shader can read them.

### Task 2: Integrate indirect draw command emission into meshlet_cull.comp
- Extended meshlet_cull.comp to write both visible meshlet indices AND `DrawIndexedIndirectCommand` to binding 14 (meshlet_indirect_buffer) alongside the compaction output.
- Command fields: `indexCount = tri_count * 3`, `instanceCount = 1`, `firstIndex = tri_offset`, `vertexOffset = vtx_offset`, `firstInstance = 0`.
- Commands packed as 5 consecutive uint32 per entry in the SSBO.

### Task 3: Implement MeshletPipeline trait and ComputeIndirectPath (TDD)
- **MeshletPipeline trait**: `record_draw()` method abstracting the meshlet rendering backend.
- **ComputeIndirectPath**: Graphics pipeline using meshlet_draw.vert/frag. Binds meshlet_vertex_buffer as VB (stride 8), meshlet_tri_buffer as IB (INDEX_TYPE_UINT32), calls `vkCmdDrawIndexedIndirectCount` with meshlet_indirect_buffer and meshlet_count_buffer.
- **Renderer** gains `meshlet_pipeline: Option<ComputeIndirectPath>` and `use_meshlet_rendering: bool` (default true).
- **ChunkMeshPipeline** retained as legacy per-chunk path.
- Source-grep test `phase6_meshlet_pipeline_trait_exists` validates trait, method, and struct names.

### Task 4: Wire meshlet draw into submit_frame
- Meshlet rendering is now the default draw path in `submit_frame`.
- When `use_meshlet_rendering=true` and meshlet_pipeline is present, `ComputeIndirectPath::record_draw()` is called instead of legacy per-chunk draw.
- Barriers updated: meshlet cull COMPUTE_WRITE -> INDIRECT_READ + VERTEX_INPUT + VERTEX_SHADER for visible_meshlet_buffer, meshlet_count_buffer, and meshlet_indirect_buffer.
- Legacy per-chunk path available via `use_meshlet_rendering=false` runtime toggle.
- `submit_frame_sequence` updated to reflect new steps.

## Files Modified
- `shaders/meshlet_draw.vert` (new)
- `shaders/meshlet_draw.frag` (new)
- `shaders/meshlet_cull.comp` (extended with indirect draw emission)
- `src/renderer/mesh_pipeline.rs` (MeshletPipeline trait + ComputeIndirectPath + retained ChunkMeshPipeline)
- `src/renderer/submit.rs` (meshlet draw wiring, updated barriers and sequence)
- `src/renderer/mod.rs` (meshlet_pipeline field, use_meshlet_rendering toggle, cleanup)
- `src/renderer/bindless.rs` (bindings 10, 13 stage flags updated)
- `build.rs` (2 new shader entries)
- `tests/phase6_meshlet.rs` (new test: phase6_meshlet_pipeline_trait_exists)

## Verification
- `cargo build` -- OK
- `cargo test --test phase6_meshlet` -- 13/13 tests pass
- `cargo clippy --all-targets` -- zero warnings

## Architecture Notes
- **Rendering flow**: meshlet_cull.comp -> visible_meshlet_buffer + meshlet_indirect_buffer -> vkCmdDrawIndexedIndirectCount
- **Vertex shader chain**: gl_DrawID -> visible_meshlet_buffer[gl_DrawID] -> meshlet_id -> GpuMeshlet.chunk_slot -> GpuChunkInstance
- **Index type**: u32 (widened from u8 during MeshletPool::record_upload in Plan 01)
- **No firstInstance abuse**: firstInstance=0, gl_DrawID auto-provided via VK_KHR_shader_draw_parameters (core Vulkan 1.1)
