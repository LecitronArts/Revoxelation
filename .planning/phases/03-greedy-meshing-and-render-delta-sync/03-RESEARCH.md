# Phase 3: Greedy Meshing and Render Delta Sync - Research

**Researched:** 2026-03-22
**Domain:** Rust/Vulkan voxel meshing, chunk-delta render sync, and indirect draw orchestration
**Confidence:** MEDIUM

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### 顶点格式

- **格式：** 压缩打包，每顶点 **2× u32**（8 字节/顶点）
  - `u32_0`：xyz 格坐标（各 6 bit，限 64³ 区块内，共 18 bit）+ 面朝向（6 种，6 bit）+ 预留 8 bit
  - `u32_1`：block_id（8 bit）+ quad 内相对 UV 偏移（16 bit）+ 预留 8 bit
- **索引列表：** 顶点 + 索引（4 顶点/quad + 6 索引/quad），节省约 33% VRAM
- **UV 布局：** 存 quad 内相对偏移（16 bit 够）。纹理 atlas 切片由 block_id 在 shader 中查表，Phase 5 填充纹理逻辑时复用此布局

#### GPU 缓冲区生命周期（槽位池）

- **结构：** 单块大缓冲区分段，VB 和 IB 各一块，均分为 N 个等大槽位
- **槽位数 N：** = 最大同时活跃区块数（LOD0 + LOD1 + LOD2 之和，与 Phase 2 的流式上限对齐）
- **每槽大小：** 按 LOD0 最差情况预留（6 × 64² 面 → 贪心合并后保守估计 ~4096 quad）；各 LOD 层共用相同槽位大小（空间换管理复杂度）
- **remesh 路径：** 通过 StagingBuffer（已有）→ vkCmdCopyBuffer → 对应槽位，only dirty chunks 上传
- **槽位分配：** 区块进入 Active 时分配槽位，进入 Unloading 时释放槽位

#### LOD 边界接缝（Border Skirt）

- **策略：** LOD1/LOD2 负责生成 skirt —— 在面向更高精度（LOD0）邻居的一侧向下延伸额外面
- **触发条件：** LOD1 生成 mesh 时查询相邻位置的 LOD 级别；若相邻为 LOD0 且已 Active，则生成 skirt
- **未加载情况：** 若相邻 LOD0 尚未加载（未 Active），不生成 skirt；LOD0 进入 Active 后触发该 LOD1 区块失效重新生成（邻居失效机制复用）
- **LOD0 不生成 skirt：** LOD0 自身 mesh 不因相邻 LOD1 而改变

#### Draw Call 结构

- **方式：** Multi-draw indirect（`vkCmdDrawIndexedIndirect`）
- **Culling：** 独立 compute shader pass，对每个活跃区块做 AABB vs 当帧视锥体剔除，通过的写入 indirect draw buffer
- **Instance buffer 同步：** CPU 只在区块进出活跃集时增量更新（写入新增区块的 AABB，清除已卸载区块条目），其余帧不碰 buffer
- **时序：** 同帧内 compute pass → pipeline barrier → graphics draw pass；barrier 确保 indirect buffer 写完再读
- **Descriptor：** 一个 storage buffer 存所有活跃区块的 AABB + 槽位 index；compute 写 indirect buffer，graphics 读 indirect buffer + 大 VB/IB

### Claude's Discretion

- 具体 descriptor set layout 和 binding 编号
- VkPipeline / VkPipelineLayout 的创建与缓存策略
- compute shader GLSL/SPIR-V 的具体实现细节
- barrier 的精确 srcStageMask / dstStageMask 选择

### Deferred Ideas (OUT OF SCOPE)

- LOD 混合过渡（blend/淡入淡出，避免 hard cut 闪烁）—— Phase 4 或之后
- Hi-Z occlusion culling（在 frustum culling 基础上加 depth pyramid 剔除遮挡体）—— 后续优化阶段
- 顶点格式从 2× u32 压缩进一步到 1× u32（牺牲扩展性）—— 性能调优阶段
- 多 transfer queue 异步 mesh 上传（当前用 graphics queue）—— Phase 6 之后
- 完整 LOD 层 LOD3/LOD4 超远景 —— 后续阶段
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| MESH-01 | Engine can generate greedy meshes for visible chunk surfaces and update them incrementally. | Use a padded chunk-voxel input contract, 3-axis face-mask greedy sweep, and a stable chunk slot pool so remesh touches only the dirty chunk's slot and command entry. |
| MESH-02 | Chunk-border updates correctly invalidate neighbor meshes to avoid visible seams. | Track dirty causes separately from lifecycle state; invalidate self + touched border neighbors, and remesh low-detail neighbors when LOD0 activation changes skirt requirements. |
| MESH-03 | Renderer integration supports chunk-delta updates so chunk edits do not require full world reupload. | Keep one metadata/indirect entry per active chunk, reuse fixed VB/IB slots, and update only changed slots plus their indirect commands instead of rebuilding world buffers. |
</phase_requirements>

## Summary

Phase 3 should be planned as the first real chunk-rendering phase, not as a narrow mesher optimization. The live repository already has an `ash` renderer bootstrap, allocator-backed staging helper, scheduler stages, and streaming lifecycle state, but it does not yet have a structured chunk voxel payload, a greedy mesher, a graphics pipeline for chunk surfaces, a compute culling pipeline, or any chunk slot pool. The `.planning/codebase/*.md` files are partially stale here: they still describe an older `wgpu` world-sync architecture and say there is no `tests/` directory, while the live repo is the Phase 2.5 Vulkan shape and already has `tests/phase25_vulkan.rs` and `tests/phase2_streaming.rs`.

The strongest implementation shape is: define a structured chunk-voxel input first, generate quads with a greedy face-mask sweep over a 1-voxel padded neighborhood, pack those quads into the locked `2 x u32` vertex format plus 6 indices per quad, then upload them into fixed-size chunk slots inside large shared VB/IB buffers. Pair that with one metadata entry and one indirect draw command per active chunk. CPU work should only update metadata and command templates when a chunk becomes active, remeshes, or unloads. The compute culling pass then only toggles visibility for those active commands, followed by a compute-to-draw-indirect barrier and a single `vkCmdDrawIndexedIndirect`.

The largest planning risk is not Vulkan itself; it is the missing data contract between streaming and meshing. Today the background job system returns `Generated(Box<[u8]>)`, which is too weak for neighbor-aware greedy meshing, border invalidation, or LOD skirts. Phase 3 needs a real chunk voxel representation and a clear separation between streaming state, meshing state, and renderer state before the GPU path will stay predictable.

**Primary recommendation:** Plan Phase 3 around a stable per-chunk slot model: `ChunkVoxels -> GreedyQuads -> PackedMesh -> SlotUpload -> Metadata/Indirect update`, with dirty invalidation handled explicitly and the compute pass only deciding visibility, not rebuilding draw commands from scratch.

## Current Source Overrides Stale Docs

Use this trust order for Phase 3 planning:
1. Current source under `src/renderer/**`, `src/runtime/**`, and `src/streaming/**`
2. `.planning/ROADMAP.md` and `.planning/STATE.md`
3. Phase `02.5` artifacts (`02.5-CONTEXT.md`, `02.5-RESEARCH.md`, plans/summaries)
4. Older `.planning/codebase/*` mapping docs only when they do not conflict with the above

Explicit stale-doc conflicts found:
- `.planning/codebase/ARCHITECTURE.md` still describes a `wgpu` compute-first renderer, `src/app.rs`, `src/world/mod.rs`, and `src/renderer/core/*` modules that do not exist in the current Vulkan codebase.
- `.planning/codebase/STACK.md` still lists `wgpu`, `egui-wgpu`, and the old world/renderer module layout. The live repo uses `ash`, `gpu-allocator`, `src/renderer/{mod,instance,device,swapchain,frame,egui_backend}.rs`, and no `wgpu` renderer code.
- `.planning/codebase/STRUCTURE.md` points to `src/app.rs`, `src/ecs.rs`, `src/world/mod.rs`, and `src/renderer/core/*`; the live repo instead centers on `src/runtime/**`, `src/streaming/**`, and the flat Vulkan renderer files created in Phase 2.5.
- `.planning/codebase/INTEGRATIONS.md` describes `Renderer::new(window.clone(), world.clone())`, `wgpu` surface setup, and world-sync uploads that are not the current integration surface.
- `.planning/codebase/TESTING.md` says there is no `tests/` directory, but the live repo contains `tests/phase1_*.rs`, `tests/phase2_streaming.rs`, and `tests/phase25_vulkan.rs`.

Planning consequence:
- Treat the older `.planning/codebase/*` files as historical context only.
- Derive Phase 3 task structure from the current Vulkan renderer source tree and the Phase 02.5 artifacts, not from the retired `wgpu` architecture notes.

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `ash` | repo pin `0.38`; latest verified `0.38.0+1.3.281` (2024-04-01) | Raw Vulkan bindings and pipeline/device setup | Already adopted in Phase 2.5; current bindings expose the exact feature and barrier APIs this phase needs. |
| `gpu-allocator` | repo pin `0.27`; latest verified `0.28.0` (2025-09-26) | GPU buffer/image memory allocation | Solves Vulkan suballocation correctly; Phase 3 should keep using it for big VB/IB buffers and metadata buffers rather than inventing allocator logic. |
| `bytemuck` | repo pin `1.16`; latest verified `1.25.0` (2026-01-31) | POD casting for packed vertices, indices, indirect commands, and metadata records | Standard Rust way to make packed GPU structs explicit and testable. |
| `rayon` | repo pin `1.10`; latest verified `1.11.0` (2025-08-12) | Background CPU meshing work | Already used for chunk jobs; meshing jobs are the same kind of bounded CPU work. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `shaderc` | latest verified `0.10.1` (2025-09-06) | Build-time GLSL -> SPIR-V compilation | Use if Phase 3 adds in-repo shader sources for the chunk graphics pipeline and compute culling pipeline. |
| `block-mesh` | latest verified `0.2.0` | Reference implementation for padded greedy quad generation and merge-key rules | Use as a reference for algorithm structure only; the locked packed vertex format and LOD skirt rules still require custom project code. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom greedy mesher + custom packing | `block-mesh 0.2.0` directly | Faster bring-up, but it does not solve the locked `2 x u32` packing, chunk-slot metadata model, or LOD skirt invalidation policy. |
| `drawIndirectFirstInstance`-indexed metadata | `DrawIndex` via `shaderDrawParameters` | Viable, but it adds another required Vulkan feature and is less aligned with the locked “instance buffer + slot index” design. |
| `shaderc` build dependency | Checked-in `.spv` files | Fewer build dependencies, but worse shader iteration and easier shader/source drift. |

**Installation:**
```bash
# No new runtime crates are required for Phase 3.
# If you want in-repo GLSL -> SPIR-V compilation, add:
cargo add --build shaderc@0.10.1
```

**Version verification:**
- `ash`: docs.rs latest = `0.38.0+1.3.281` (2024-04-01)
- `gpu-allocator`: docs.rs latest = `0.28.0` (2025-09-26); repo is pinned to `0.27`
- `bytemuck`: docs.rs latest = `1.25.0` (2026-01-31); repo is pinned to `1.16`
- `rayon`: docs.rs latest = `1.11.0` (2025-08-12); repo is pinned to `1.10`
- `shaderc`: docs.rs latest = `0.10.1` (2025-09-06)

For this phase, keep the existing runtime crate pins unless a planned task explicitly broadens scope to dependency upgrades.

## Architecture Patterns

### Recommended Project Structure
```text
src/
├── meshing/
│   ├── mod.rs                # domain surface and shared types
│   ├── voxel_chunk.rs        # structured chunk voxels + neighbor/halo access
│   ├── greedy.rs             # 3-axis face-mask sweep and quad emission
│   ├── packing.rs            # 2 x u32 vertex packing + index emission
│   └── invalidation.rs       # dirty flags, border neighbors, skirt invalidation
├── renderer/
│   ├── chunk_pool.rs         # slot allocator + shared VB/IB + metadata buffers
│   ├── mesh_pipeline.rs      # chunk graphics pipeline and shader modules
│   └── cull_pipeline.rs      # compute frustum culling + indirect buffer update
└── runtime/
    └── scheduler.rs          # mesh job drain + renderer delta sync wiring
```

### Pattern 1: Structured Chunk Input Before Meshing
**What:** Replace `Generated(Box<[u8]>)` as the only meshing input with a typed chunk payload that exposes block id / occupancy queries and a fixed chunk edge length.
**When to use:** Immediately at the start of `03-01`; every later meshing and invalidation task depends on it.
**Example:**
```rust
pub const CHUNK_EDGE: u32 = 64;

pub struct ChunkVoxels {
    pub blocks: Box<[u8]>,
}

impl ChunkVoxels {
    pub fn block(&self, x: u32, y: u32, z: u32) -> u8 {
        let i = (z * CHUNK_EDGE * CHUNK_EDGE + y * CHUNK_EDGE + x) as usize;
        self.blocks[i]
    }
}
```

### Pattern 2: Greedy Face-Mask Sweep with Halo Reads
**What:** Sweep each axis, build a visible-face mask for one slice at a time, merge only contiguous faces with the same merge key, and require padded neighbor access so boundary faces can see across chunk edges.
**When to use:** Core of `03-01` greedy mesh generation.
**Example:**
```rust
for axis in 0..3 {
    for slice in 0..CHUNK_EDGE {
        face_mask.clear();
        build_face_mask(voxels_with_halo, axis, slice, &mut face_mask);
        greedy_merge_mask(&face_mask, |quad, face| {
            emit_packed_quad(face, quad, &mut vertices, &mut indices);
        });
    }
}
```
Source: 0fps greedy meshing article and `block-mesh`'s padded `greedy_quads` design.

### Pattern 3: Stable Slot Pool with CPU-Owned Command Templates
**What:** Give each active chunk a stable slot id. That slot owns a fixed VB subrange, fixed IB subrange, one metadata record, and one indirect draw command. Remeshes overwrite the same slot in place. Unloads free the slot.
**When to use:** `03-02` renderer integration.
**Example:**
```rust
struct ChunkRenderSlot {
    slot_id: u32,
    vertex_offset: u32,
    first_index: u32,
    index_count: u32,
    metadata_index: u32,
}
```

Recommended behavior:
- CPU writes `first_index`, `vertex_offset`, `firstInstance = metadata_index`, and `indexCount` when a chunk uploads or remeshes.
- Compute culling only toggles `instanceCount` to `1` or `0`.
- Graphics submits `vkCmdDrawIndexedIndirect` once with `drawCount = active_chunk_count`.

### Pattern 4: Neighbor-Aware Dirty Invalidation
**What:** Dirty state is separate from lifecycle state. A block edit or streamed chunk change marks the chunk dirty; if the touched voxel lies on a chunk boundary, it also marks the matching neighbor dirty. LOD0 activation additionally marks lower-detail neighbors dirty so skirts are regenerated.
**When to use:** End of `03-01` and all of `03-02`.
**Example:**
```rust
mark_dirty(chunk);
for neighbor in touched_border_neighbors(local_block_pos, chunk_key) {
    mark_dirty(neighbor);
}
for coarse_neighbor in newly_required_skirt_neighbors(chunk_key) {
    mark_dirty(coarse_neighbor);
}
```

### Pattern 5: Capability Gate Before No-Fallback Indirect Rendering
**What:** Since the user explicitly rejected a per-draw fallback, Phase 3 must fail early on unsupported hardware instead of silently degrading.
**When to use:** Device creation / feature enable path before any mesh pipeline work.
**Example:**
```rust
let features = vk::PhysicalDeviceFeatures::default()
    .sampler_anisotropy(true)
    .multi_draw_indirect(true)
    .draw_indirect_first_instance(true);
```
Source: Vulkan refpage for `VkPhysicalDeviceFeatures`.

### Anti-Patterns to Avoid
- **Reusing `ChunkState` as mesh dirtiness:** lifecycle state and mesh invalidation are different concerns; mixing them produces remesh bugs.
- **Remeshing without a padded neighborhood:** chunk-edge faces and skirts will be wrong the first time a border changes.
- **One Vulkan buffer per chunk:** too much allocation churn; the fixed slot-pool decision already rules this out.
- **Compute pass rewriting every draw command from scratch:** keep command templates CPU-owned and let compute only decide visibility.
- **Calling `submit_one_shot_commands` once per dirty chunk forever:** it currently waits for the whole graphics queue to go idle, which is acceptable for bring-up but poor steady-state behavior.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Vulkan memory suballocation | A custom allocator for chunk VB/IB and metadata buffers | `gpu-allocator` | Buffer/image suballocation edge cases are already solved there, and the project already uses it. |
| General crack-fixing LOD triangulation | A full Transvoxel-style transition-cell system for this phase | The locked border-skirt policy | This project is meshing axis-aligned voxel faces, not smooth marching-cubes terrain. Full transition-cell machinery is unnecessary scope. |
| Whole-world render sync | A monolithic “rebuild all mesh buffers” path | Stable slot pool + dirty chunk uploads | Full reupload defeats MESH-03 and recreates the bottleneck Phase 3 exists to remove. |
| Shader asset drift management | Hand-managed binary blobs with no source linkage | `shaderc` build step or a documented SPIR-V generation workflow | The phase introduces at least a graphics shader pair and a compute culling shader; they need a repeatable source-of-truth path. |

**Key insight:** The hard part of this phase is not the sweep itself. It is making chunk-local remeshes, neighbor invalidation, and indirect draw state all agree on the same stable per-chunk identity.

## Common Pitfalls

### Pitfall 1: Meshing Against an Unstructured Byte Blob
**What goes wrong:** The mesher cannot answer “is this face visible?” or “what block id is on the other side?” consistently.
**Why it happens:** `ChunkJobOutcome::Generated(Box<[u8]>)` exists, but there is no typed chunk voxel layout or chunk edge constant in the live code.
**How to avoid:** Introduce a `ChunkVoxels` contract first, with explicit occupancy/material accessors and a fixed chunk edge length.
**Warning signs:** Greedy code starts hard-coding offsets into `[u8]` slices in multiple modules.

### Pitfall 2: Missing Halo / Neighbor Reads at Chunk Borders
**What goes wrong:** Faces disappear at chunk seams or fail to appear until both chunks remesh.
**Why it happens:** Greedy meshing needs to compare each solid voxel against its neighbor. At the chunk boundary, that neighbor lives in another chunk or in a padded halo.
**How to avoid:** Mesh against a 1-voxel halo view and always invalidate matching border neighbors when a border block changes.
**Warning signs:** Interior faces are correct but chunk edges show holes or one-frame seams.

### Pitfall 3: Wrong Merge Keys
**What goes wrong:** Quads merge across different block ids, opposite normals, or skirt/non-skirt cases, which corrupts shading, UV selection, or seam coverage.
**Why it happens:** Greedy merging is only valid when every face in the merged rectangle shares the same merge value.
**How to avoid:** Use a merge key that includes at least face direction, block id, and any flag that changes emitted geometry semantics.
**Warning signs:** Large quads appear with the wrong block texture or a skirt swallows a normal face.

### Pitfall 4: Stale Slot Metadata After Unload or Reuse
**What goes wrong:** A freed chunk slot still draws old geometry or gets culled against the wrong AABB.
**Why it happens:** VB/IB data, metadata records, and indirect commands are not cleared or overwritten together.
**How to avoid:** Treat slot free/reuse as an atomic state change: remove chunk->slot mapping, clear metadata/AABB, and zero the command entry.
**Warning signs:** Geometry from unloaded chunks flashes back when new chunks reuse old slots.

### Pitfall 5: Queue-Idle Uploads Masquerading as “Incremental”
**What goes wrong:** Dirty-chunk uploads are technically delta uploads but still hitch because every upload idles the graphics queue.
**Why it happens:** The current `submit_one_shot_commands` helper ends with `queue_wait_idle`.
**How to avoid:** Use it for bring-up or rare setup, but plan a batched copy path for normal remesh churn if upload frequency becomes visible.
**Warning signs:** Frame time spikes scale with the number of dirty chunks even though only a few slots change.

### Pitfall 6: Order-Sensitive Tests Due to Global Runtime State
**What goes wrong:** Tests become flaky or start failing when added in a different order.
**Why it happens:** The runtime uses `OnceLock` global state and frame-index-sensitive integration tests already warn about order sensitivity.
**How to avoid:** Keep pure greedy/packing/slot tests in local unit modules; reserve scheduler integration tests for a small number of requirement-level flows with unique frame ranges.
**Warning signs:** Tests pass individually but fail in the full suite.

## Code Examples

Verified patterns from official or primary sources:

### Enable Indirect-Draw Features Explicitly
```rust
let features = vk::PhysicalDeviceFeatures::default()
    .sampler_anisotropy(true)
    .multi_draw_indirect(true)
    .draw_indirect_first_instance(true);

let create_info = vk::DeviceCreateInfo::default()
    .queue_create_infos(&queue_create_infos)
    .enabled_features(&features);
```
Source: https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceFeatures.html

### Compute -> Indirect Barrier
```rust
let barrier = [vk::BufferMemoryBarrier::default()
    .src_access_mask(vk::AccessFlags::SHADER_WRITE)
    .dst_access_mask(vk::AccessFlags::INDIRECT_COMMAND_READ)
    .buffer(indirect_buffer)
    .offset(0)
    .size(vk::WHOLE_SIZE)];

unsafe {
    device.cmd_pipeline_barrier(
        command_buffer,
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::DRAW_INDIRECT,
        vk::DependencyFlags::empty(),
        &[],
        &barrier,
        &[],
    );
}
```
Source: https://docs.vulkan.org/guide/latest/synchronization_examples.html

### Greedy Meshing Needs a Padded Boundary
```rust
// Mesh the chunk interior, but keep a 1-voxel halo available for visibility tests.
let padded_extent = chunk_extent.padded(1);
let mut output = GreedyQuadsBuffer::new(padded_extent.volume() as usize);
greedy_quads(&voxels_with_halo, &padded_extent, &mut output);
```
Source: `block-mesh` docs/source: https://docs.rs/block-mesh/latest/block_mesh/fn.greedy_quads.html

### Merge Only Faces with the Same Merge Value
```rust
pub trait MergeVoxel {
    type VoxelValue: Eq;
    fn voxel_merge_value(&self) -> Self::VoxelValue;
}
```
Source: https://docs.rs/building_blocks_mesh/latest/src/building_blocks_mesh/greedy_quads.rs.html

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| One quad per visible face or full cube emission | Greedy quads on visible faces only | Standard practice since 0fps (2012); still current | Much smaller chunk meshes with similar asymptotic traversal cost. |
| CPU issuing one draw call per chunk | Compute frustum culling + multi-draw indirect | Modern Vulkan rendering practice | Keeps CPU submission cost flat as active chunk count grows. |
| Rebuild or reupload the whole world on any change | Slot-level delta uploads and per-chunk command updates | Modern streaming voxel/render architectures | Necessary for edit responsiveness and streaming scalability. |
| Ad hoc binary shader blobs | Repeatable GLSL -> SPIR-V workflow | Ongoing Vulkan best practice | Makes new graphics/compute shaders reviewable and less drift-prone. |

**Deprecated/outdated:**
- Per-frame whole-world mesh uploads for local chunk edits.
- Per-chunk dedicated GPU buffers for a high-churn streaming set.
- Treating `MeshSync` as only a place to flip `Loading -> Active` without owning any real mesh state.

## Open Questions

1. **What is the authoritative chunk voxel layout?**
   What we know: the locked vertex format assumes `64^3` local coordinates, but the live repo does not define a chunk edge length or typed voxel payload.
   What's unclear: exact in-memory chunk representation, block id width, and whether empty/solid is enough or if material ids are already required.
   Recommendation: make `CHUNK_EDGE = 64` and a dense `u8` block-id chunk payload a Wave 0 decision inside `03-01`.

2. **Should Phase 3 add an in-repo shader compilation step now?**
   What we know: the renderer has no shader-module path yet, and Phase 3 needs at least one graphics pipeline plus one compute pipeline.
   What's unclear: whether the project wants `shaderc` build-time compilation or checked-in `.spv` artifacts.
   Recommendation: prefer `shaderc` as a build dependency unless build time or CI constraints explicitly reject it.

3. **How should the scheduler split generation completion from meshing completion?**
   What we know: the current `MeshSync` stage drains chunk generation results, but Phase 3 also needs real meshing jobs and renderer delta sync work.
   What's unclear: whether to add a dedicated meshing queue/state or overload the existing streaming result channel.
   Recommendation: separate generation payload arrival from meshing dirtiness; do not overload one result type with both concerns.

4. **What is the unsupported-hardware behavior?**
   What we know: the locked design requires `multiDrawIndirect`, and the recommended metadata path also needs `drawIndirectFirstInstance`.
   What's unclear: whether unsupported GPUs are out of scope or need a clear runtime error.
   Recommendation: add an explicit capability check and fail fast with a descriptive message; do not add a per-draw fallback in this phase.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust unit tests + Cargo integration tests |
| Config file | none |
| Quick run command | `cargo test --test phase3_meshing` |
| Full suite command | `cargo test` |

Current baseline: `cargo test` passes locally on 2026-03-22 in about 11 seconds.

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MESH-01 | Greedy mesher emits packed quads for visible surfaces and remeshes only dirty chunks | unit + integration | `cargo test --test phase3_meshing mesh_01_greedy_meshing_emits_expected_quads -x` | ❌ Wave 0 |
| MESH-02 | Border edits and LOD0 activation invalidate the right neighbors and regenerate skirts without seams | integration | `cargo test --test phase3_meshing mesh_02_border_invalidation_marks_neighbors -x` | ❌ Wave 0 |
| MESH-03 | Renderer updates only the affected chunk slots / metadata / indirect commands, not a full-world buffer | unit + integration | `cargo test --test phase3_meshing mesh_03_delta_sync_updates_only_dirty_slots -x` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test --test phase3_meshing`
- **Per wave merge:** `cargo test`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `tests/phase3_meshing.rs` - requirement-level integration coverage for MESH-01, MESH-02, and MESH-03
- [ ] `src/meshing/greedy.rs` unit tests - merge keys, halo reads, and quad packing edge cases
- [ ] `src/renderer/chunk_pool.rs` unit tests - slot reuse, metadata clearing, and indirect command template updates
- [ ] Scheduler integration harness notes - document frame-index ranges to avoid collisions with existing `OnceLock`-backed tests

## Sources

### Primary (HIGH confidence)
- Vulkan Guide synchronization examples - compute-to-draw-indirect and compute-to-index-input barrier patterns: https://docs.vulkan.org/guide/latest/synchronization_examples.html
- Vulkan refpage `VkPhysicalDeviceFeatures` - `multiDrawIndirect` and `drawIndirectFirstInstance` requirements: https://docs.vulkan.org/refpages/latest/refpages/source/VkPhysicalDeviceFeatures.html
- Vulkan Guide `Ways to Provide SPIR-V` - shader module provisioning options: https://docs.vulkan.org/guide/latest/ways_to_provide_spirv.html
- `ash` docs.rs crate page - current latest version/date verification: https://docs.rs/crate/ash/latest
- `gpu-allocator` docs.rs crate page - current latest version/date verification: https://docs.rs/crate/gpu-allocator/latest
- `bytemuck` docs.rs crate page - current latest version/date verification: https://docs.rs/crate/bytemuck/latest
- `rayon` docs.rs crate page - current latest version/date verification: https://docs.rs/crate/rayon/latest
- `shaderc` docs.rs crate page - current latest version/date verification: https://docs.rs/crate/shaderc/latest

### Secondary (MEDIUM confidence)
- 0fps, `Meshing in a Minecraft Game` - greedy meshing criteria, sweep structure, and complexity framing: https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/
- 0fps, `Meshing in a Minecraft Game (Part 2)` - merge keys for multiple voxel types and normals: https://0fps.net/2012/07/07/meshing-minecraft-part-2/
- `block-mesh` docs.rs crate docs - padded greedy meshing and reusable output buffer patterns: https://docs.rs/block-mesh/latest/block_mesh/
- `building_blocks_mesh` source view - merge-value trait and padded extent patterns: https://docs.rs/building_blocks_mesh/latest/src/building_blocks_mesh/greedy_quads.rs.html

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: MEDIUM - core crate versions were verified, but whether to add `shaderc` now is still a project judgment call.
- Architecture: MEDIUM - grounded in live code and official Vulkan docs, but the repo still lacks a real chunk graphics path, so some structure is prescriptive rather than observed.
- Pitfalls: HIGH - mostly derived from current repo gaps (`Generated(Box<[u8]>)`, global state tests, queue-idle uploads) and official Vulkan feature/sync requirements.

**Research date:** 2026-03-22
**Valid until:** 2026-04-21
