# Phase 5: Bindless Architecture and GPU Scene - Research

**Researched:** 2026-03-26
**Method:** Direct codebase analysis (researcher agent MCP timeout fallback)

## 1. Current Architecture Analysis

### Device & Feature Requirements (src/renderer/device.rs)
- **Current Vulkan level:** Vulkan 1.0 — `DeviceCreateInfo` uses `enabled_features` (VkPhysicalDeviceFeatures), no `pNext` chain for Vulkan 1.1/1.2 features.
- **Current required features:** `samplerAnisotropy`, `multiDrawIndirect`, `drawIndirectFirstInstance`.
- **No VkPhysicalDeviceVulkan12Features** queried or enabled. Must add:
  - `descriptorIndexing` (core Vulkan 1.2)
  - `shaderSampledImageArrayNonUniformIndexing`
  - `runtimeDescriptorArray`
  - `descriptorBindingPartiallyBound`
  - `descriptorBindingSampledImageUpdateAfterBind`
  - `descriptorBindingStorageBufferUpdateAfterBind`
  - `drawIndirectCount` (core Vulkan 1.2, for vkCmdDrawIndexedIndirectCount)
- **API version:** Must set VkApplicationInfo.apiVersion to VK_API_VERSION_1_2.
- **ash API:** Use `vk::PhysicalDeviceVulkan12Features` via `pNext` chain on `DeviceCreateInfo`.
- **Feature probe:** Use `instance.get_physical_device_features2()` with `VkPhysicalDeviceVulkan12Features` in pNext.

### Buffer Architecture (src/renderer/chunk_pool.rs)
- **Current: 6 buffers** per ChunkPool:
  1. `vertex_buffer` — GpuOnly, VERTEX_BUFFER
  2. `index_buffer` — GpuOnly, INDEX_BUFFER
  3. `metadata_buffer` — GpuOnly, STORAGE_BUFFER (ChunkDrawMetadata per slot)
  4. `indirect_template_buffer` — GpuOnly, STORAGE_BUFFER | INDIRECT_BUFFER
  5. `draw_slot_buffer` — GpuOnly, STORAGE_BUFFER (draw_index → slot_id mapping)
  6. `dense_indirect_buffer` — GpuOnly, STORAGE_BUFFER | INDIRECT_BUFFER (cull output)
- **Target: 3 buffers:**
  1. `vertex_buffer` (keep)
  2. `index_buffer` (keep)
  3. `scene_buffer` — unified SSBO merging metadata + indirect commands + GpuChunkInstance data
- **MAX_RENDER_CHUNKS = 881** — hardcoded constant, used for all buffer sizes.
- **SlotAllocator** manages chunk_to_slot/slot_to_chunk/free_slots with swap-remove compaction.

### Descriptor Sets (src/renderer/cull_pipeline.rs, mesh_pipeline.rs)
- **Cull pipeline:** 8 bindings on its own descriptor set:
  - binding 0: metadata SSBO
  - binding 1: indirect template SSBO
  - binding 2: draw slots SSBO
  - binding 3: dense indirect SSBO (output)
  - binding 4: frustum planes SSBO
  - binding 5: draw count SSBO
  - binding 6: Hi-Z config SSBO
  - binding 7: Hi-Z pyramid combined image sampler
- **Mesh pipeline:** 1 binding on its own descriptor set:
  - binding 0: metadata SSBO (ChunkDrawMetadata)
- **Both pipelines create their own descriptor_pool, descriptor_set_layout, descriptor_set.**
- **HiZ pipeline:** separate descriptor set (2 bindings: src sampler, dst storage image).

### Shader Analysis
- **chunk_mesh.vert:** Uses `gl_InstanceIndex` to index `chunk_metadata.metadata[]`. After bindless migration, should use `gl_DrawID` to index the scene buffer GpuChunkInstance array.
- **chunk_mesh.frag:** Currently just outputs `v_color` (flat color from block_id hash). Must add texture array sampling.
- **chunk_cull.comp:** Reads metadata, indirect_templates, draw_slots; writes dense_indirect and draw_count. After migration, reads from scene_buffer regions instead.

### Staging Pipeline (src/renderer/staging_ring.rs)
- 32MB CpuToGpu ring buffer, 2 frame regions. Used for all GPU uploads.
- Buffer growth path can reuse `create_allocated_buffer` + `vkCmdCopyBuffer`.

### Instance Creation (src/renderer/instance.rs)
- Must check: current `VkApplicationInfo.apiVersion` — likely `VK_API_VERSION_1_0`. Must bump to `VK_API_VERSION_1_2`.

### Existing Patterns
- All GPU resources via `gpu-allocator`.
- Double-buffered frames (2 in-flight).
- Push constants for camera (80 bytes VERTEX stage).
- GLSL shaders compiled to SPIR-V via build.rs.
- Pipeline cache used for all pipeline creation.

## 2. Vulkan 1.2 Descriptor Indexing Requirements

### Required Features (VkPhysicalDeviceVulkan12Features)
```
descriptorIndexing = VK_TRUE
shaderSampledImageArrayNonUniformIndexing = VK_TRUE
runtimeDescriptorArray = VK_TRUE
descriptorBindingPartiallyBound = VK_TRUE
descriptorBindingSampledImageUpdateAfterBind = VK_TRUE
descriptorBindingStorageBufferUpdateAfterBind = VK_TRUE
drawIndirectCount = VK_TRUE
```

### Descriptor Set Layout Flags
- `VK_DESCRIPTOR_SET_LAYOUT_CREATE_UPDATE_AFTER_BIND_BIT` on set 0.
- Per-binding flags: `PARTIALLY_BOUND | UPDATE_AFTER_BIND` for variable-count bindings.

### Descriptor Pool
- `VK_DESCRIPTOR_POOL_CREATE_UPDATE_AFTER_BIND_BIT` flag.

### GLSL Extensions
```glsl
#extension GL_EXT_nonuniform_qualifier : require
```
- Use `nonuniformEXT()` wrapper for texture array indexing.

## 3. Bindless Set 0 Layout Design

Based on CONTEXT.md decisions:
```
set 0, binding 0: scene SSBO (GpuChunkInstance + indirect commands)
set 0, binding 1: material SSBO (BlockMaterial array)
set 0, binding 2: texture array sampler (2D array, 16×16 per layer)
set 0, binding 3+: reserved for Phase 6/7
```

Additional bindings migrated from cull pipeline:
```
set 0, binding 3: frustum planes SSBO
set 0, binding 4: draw count SSBO
set 0, binding 5: Hi-Z config SSBO
set 0, binding 6: Hi-Z pyramid sampler
set 0, binding 7: dense indirect output SSBO
```

**BindlessTable struct** manages set 0:
- Owns descriptor pool, layout, set.
- Provides `register_buffer()` / `register_texture()` to update bindings.
- Single allocation, shared by all pipelines.
- Both cull and mesh pipelines reference `set_layouts: &[bindless_layout]` in their pipeline_layout.

## 4. GpuChunkInstance Design

Replaces ChunkDrawMetadata. Scene buffer holds:
```rust
#[repr(C)]
struct GpuChunkInstance {
    aabb_min: [f32; 3],
    material_id: u32,        // was slot_id
    aabb_max: [f32; 3],
    lod_level: u32,
    chunk_origin: [f32; 3],
    chunk_scale: f32,
}
// 48 bytes per instance
```

Indirect commands stored in a separate region of scene_buffer (or kept as a separate SSBO within set 0).

Vertex shader: `gl_DrawID` indexes GpuChunkInstance array instead of `gl_InstanceIndex` with metadata.

## 5. Block Material System

```rust
#[repr(C)]
struct BlockMaterial {
    top_texture: u16,
    side_texture: u16,
    bottom_texture: u16,
    flags: u16,
    _padding: u16,  // align to 8 bytes
}
```

- 8 initial block types (dirt, grass, stone, sand, log, planks, leaves, water).
- Material SSBO indexed by block_id.
- Texture array: `VkImage` with `arrayLayers = N`, 16×16 RGBA8 per layer.
- Shader selects texture index from BlockMaterial based on face normal:
  - +Y → top_texture, -Y → bottom_texture, ±X/±Z → side_texture.

### Texture Loading
- Load PNG from `assets/textures/` using `image` crate.
- Upload to 2D array texture via staging ring.
- Initial textures: programmatically generated simple patterns (checkerboard, solid+noise).

## 6. Dynamic Capacity Growth

### Current Fixed Capacity
- `MAX_RENDER_CHUNKS = 881` — all 6 buffers sized to this.
- SlotAllocator `free_slots` exhaustion returns error.

### Growth Strategy
- Initial capacity: 1024 slots.
- Trigger: `active_chunks > capacity * 0.9`.
- Growth: 2× (Vec-style doubling).
- Flow:
  1. Allocate new buffer (2× size) via `create_allocated_buffer`.
  2. Record `vkCmdCopyBuffer` from old to new.
  3. Fence wait (between frames, not mid-recording).
  4. Destroy old buffer via `destroy_allocated_buffer`.
  5. Update descriptor set bindings to point to new buffer.
  6. Update SlotAllocator internal capacity.

### IndirectCount
- Replace `vkCmdDrawIndexedIndirect` with `vkCmdDrawIndexedIndirectCount`.
- Draw count from cull shader's atomicAdd output (draw_count_buffer).
- CPU passes `max_draw_count` as safety upper bound.
- `drawIndirectCount` feature required (Vulkan 1.2 core).
- In ash: use `khr::draw_indirect_count::Device` loader for the function pointer, or since Vulkan 1.2, it may be available as core `cmd_draw_indexed_indirect_count`.

## 7. Migration Risks & Ordering

### Plan Dependency Chain
1. **Plan 01** (Vulkan 1.2 upgrade) — must come first, enables all other plans.
2. **Plan 02** (BindlessTable) — creates set 0, migrates both pipelines.
3. **Plan 03** (Scene buffer) — merges 4 buffers into 1, uses gl_DrawID.
4. **Plan 04** (Materials + textures) — adds texture array, material SSBO.
5. **Plan 05** (Dynamic capacity + IndirectCount) — removes fixed limit.

### Key Risks
- **Breaking rendering during migration:** Plan 02 must atomically switch both pipelines to bindless set 0. A partial migration (one pipeline on old set, one on new) will crash.
- **gl_InstanceIndex → gl_DrawID:** When using IndirectCount with compacted draws, gl_InstanceIndex no longer maps to slot_id. Must use `firstInstance` field in indirect command to pass slot_id, or switch to `gl_DrawID` + scene buffer indexing.
- **Descriptor set lifetime:** Bindless set 0 must persist for the entire app lifetime. Don't destroy/recreate on swapchain resize.
- **Texture array size:** Fixed at creation. Must support at least 256 layers for future expansion. Use `maxImageArrayLayers` limit check.

## 8. Validation Architecture

### Test Strategy by Requirement
- **BIND-01 (Vulkan 1.2 hard-require):** Unit test that device creation fails gracefully on mock device without features. Integration test on real GPU verifying features are enabled.
- **BIND-02 (Single bindless set):** Verify no per-chunk descriptor updates. Check that cull and mesh pipelines share set 0. Render test: same output as before migration.
- **BIND-03 (Buffer reduction):** Count active buffers in ChunkPool. Verify 3 instead of 6. Render comparison test.
- **BIND-04 (Distinct textures):** Visual test: different block_ids render different colors/textures. Verify BlockMaterial SSBO contents match expected texture indices.
- **BIND-05 (Dynamic capacity + IndirectCount):** Load >881 chunks, verify no crash. Check that vkCmdDrawIndexedIndirectCount is used instead of vkCmdDrawIndexedIndirect.

### Regression Safety
- Existing culling behavior must be preserved (frustum + Hi-Z).
- Push constants (camera) unchanged.
- Swapchain recreation path must update bindless set if buffer handles change (only on grow).

---

## RESEARCH COMPLETE

*Phase: 05-bindless-architecture-and-gpu-scene*
*Researched: 2026-03-26 via direct codebase analysis*
