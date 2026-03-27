# Phase 6: Meshlet Pipeline - Research

**Researched:** 2026-03-27
**Status:** Complete
**Method:** Codebase exploration + domain knowledge (external research unavailable)

## Phase Goal

Split greedy mesh output into meshlets (64 verts / 124 tris clusters), implement per-meshlet GPU culling (backface+frustum+Hi-Z), and optionally leverage VK_EXT_mesh_shader hardware path with compute+indirect fallback.

## Requirements Coverage

| Req ID | Description | Research Focus |
|--------|-------------|----------------|
| MSHL-01 | Meshlet generation with bounding spheres + orientation cones | meshoptimizer API, GPU data layout |
| MSHL-02 | Per-meshlet GPU culling (backface, frustum, Hi-Z) | Compute shader design, compaction |
| MSHL-03 | Software mesh shader emulation via compute+indirect | Indirect draw architecture |
| MSHL-04 | VK_EXT_mesh_shader hardware path with fallback | Extension detection, task/mesh shaders |
| MSHL-05 | Seamless LOD transitions between meshlet groups | DAG simplification, alpha dither |

---

## 1. Meshlet Generation (MSHL-01)

### 1.1 meshoptimizer / meshopt crate

The `meshopt` Rust crate wraps meshoptimizer C++ library. Key API:

```rust
// meshopt::build_meshlets(vertices, indices, max_vertices, max_triangles) -> Meshlets
// Returns: meshlet descriptors + vertex/triangle index buffers
pub fn build_meshlets(
    indices: &[u32],
    vertices: &[f32],  // position data, stride in floats
    vertex_count: usize,
    max_vertices: usize,     // 64
    max_triangles: usize,    // 124
    cone_weight: f32,        // 0.0-1.0, spatial vs orientation clustering
) -> Meshlets;

pub struct Meshlets {
    pub meshlets: Vec<Meshlet>,
    pub vertices: Vec<u32>,    // vertex remap indices
    pub triangles: Vec<u8>,    // local triangle indices (3 bytes per tri)
}

pub struct Meshlet {
    pub vertex_offset: u32,
    pub triangle_offset: u32,
    pub vertex_count: u32,
    pub triangle_count: u32,
}
```

**Bounding sphere + orientation cone** (computed separately):
```rust
meshopt::compute_meshlet_bounds(meshlet, vertices, indices) -> MeshletBounds
pub struct MeshletBounds {
    pub center: [f32; 3],    // bounding sphere center
    pub radius: f32,         // bounding sphere radius
    pub cone_apex: [f32; 3], // cone apex (for backface culling)
    pub cone_axis: [f32; 3], // normalized cone axis
    pub cone_cutoff: f32,    // cos(half-angle), < 0 = more than hemisphere
    // Also: cone_axis_s8 + cone_cutoff_s8 for quantized versions
}
```

### 1.2 Integration with Existing Greedy Mesher

**Current flow**: `build_greedy_mesh()` → `PackedMesh { vertices, indices, quad_count, aabb }`

**Phase 6 flow**: `build_greedy_mesh()` → `PackedMesh` → `build_meshlets()` → `MeshletMesh`

Key consideration: meshoptimizer needs float positions for spatial clustering, but PackedVertex packs positions into u32 bitfields. Two approaches:

**Option A (Recommended)**: Unpack positions temporarily for meshoptimizer, keep PackedVertex for GPU.
- `build_greedy_mesh()` → `PackedMesh`
- Extract float positions from PackedVertex (7-bit x/y/z → f32)
- Call `meshopt::build_meshlets()` with float positions
- Output `MeshletMesh` containing meshlet descriptors + original PackedVertex data

**Option B**: Keep separate position array during meshing. More invasive to greedy.rs.

### 1.3 GPU Data Layout (GpuMeshlet)

```glsl
struct GpuMeshlet {
    // Bounding sphere (16 bytes)
    vec3 center;      // 12 bytes, local-space
    float radius;     // 4 bytes

    // Orientation cone (16 bytes)
    vec3 cone_axis;   // 12 bytes, normalized
    float cone_cutoff; // 4 bytes, cos(half-angle)

    // Data offsets (16 bytes)
    uint vertex_offset;    // into global meshlet vertex buffer
    uint triangle_offset;  // into global meshlet triangle buffer
    uint vertex_count;     // max 64
    uint triangle_count;   // max 124

    // Metadata (16 bytes)
    uint chunk_slot;       // which chunk this meshlet belongs to
    uint lod_level;        // LOD level (0=full, 1=simplified)
    uint pad0, pad1;
};
// Total: 64 bytes per meshlet
```

### 1.4 SSBO Layout

Replace ChunkPool's 3-buffer design (vertex_buffer + index_buffer + scene_buffer) with meshlet-oriented storage:

**Buffer 1: Meshlet Metadata SSBO**
- Array of GpuMeshlet structs (64 bytes each)
- Indexed by global meshlet ID

**Buffer 2: Meshlet Vertex Data SSBO**
- PackedVertex data (8 bytes each), contiguous per-meshlet
- Meshlet.vertex_offset indexes into this

**Buffer 3: Meshlet Triangle Index SSBO**
- u8 local indices (3 bytes per triangle), packed as u32 for alignment
- Meshlet.triangle_offset indexes into this

**Buffer 4: Scene Buffer (retained)**
- GpuChunkInstance array (chunk-level metadata: origin, scale, AABB)
- Dense indirect commands (for compute path)
- Draw count

### 1.5 Capacity Management

Reuse Phase 5's dynamic capacity doubling strategy:
- Initial meshlet capacity: e.g. 64K meshlets
- Grow at 90% threshold
- GPU→GPU copy during grow
- Update bindless descriptors after grow

---

## 2. Per-Meshlet GPU Culling (MSHL-02)

### 2.1 Two-Level Cascade

Retain existing `chunk_cull.comp` as first level (chunk AABB vs frustum + Hi-Z). Chunks that pass enter meshlet-level culling.

**Level 1 (existing)**: chunk_cull.comp → outputs list of visible chunk slots
**Level 2 (new)**: meshlet_cull.comp → for each meshlet in visible chunks, test backface+frustum+Hi-Z → output compact visible meshlet list

### 2.2 meshlet_cull.comp Design

```glsl
layout(local_size_x = 64) in;

// Per-meshlet tests:
// 1. Backface: dot(cone_axis, normalize(camera_pos - center)) < cone_cutoff → cull
// 2. Frustum: sphere(center, radius) vs 6 planes → cull if outside all
// 3. Hi-Z: project sphere to screen rect, sample max depth → cull if behind

// Output: atomicAdd to visible_meshlet_count, write to visible_meshlet_indices[]
```

### 2.3 Compaction Strategy

From CONTEXT.md: "subgroup ballot + atomicAdd"

```glsl
// Phase 1: Each thread evaluates visibility → bool
bool visible = !backface_culled && !frustum_culled && !hiz_culled;

// Phase 2: Subgroup compact
uvec4 ballot = subgroupBallot(visible);
uint local_count = subgroupBallotBitCount(ballot);
uint local_offset = subgroupBallotExclusiveBitCount(ballot);

// Phase 3: One atomic per subgroup (not per thread!)
uint global_offset;
if (subgroupElect()) {
    global_offset = atomicAdd(visible_count, local_count);
}
global_offset = subgroupBroadcastFirst(global_offset);

// Phase 4: Write visible meshlet index
if (visible) {
    visible_meshlets[global_offset + local_offset] = meshlet_id;
}
```

### 2.4 Independent Toggle (Push Constants)

```glsl
layout(push_constant) uniform CullConfig {
    uint total_meshlet_count;
    uint enable_backface;   // 0 or 1
    uint enable_frustum;    // 0 or 1
    uint enable_hiz;        // 0 or 1
};
```

### 2.5 Subgroup Requirements

Vulkan 1.1+ (project already requires 1.2). Need:
- `VK_SUBGROUP_FEATURE_BALLOT_BIT`
- `VK_SUBGROUP_FEATURE_BASIC_BIT`
- Check at device creation via `VkPhysicalDeviceSubgroupProperties`

---

## 3. Software Mesh Shader Emulation (MSHL-03)

### 3.1 Compute + Indirect Draw Architecture

For GPUs without VK_EXT_mesh_shader, emulate the pipeline:

1. **meshlet_cull.comp** → compact visible meshlet indices + count
2. **meshlet_emit.comp** (optional) → transform visible meshlet indices into indirect draw commands
3. **vkCmdDrawIndexedIndirectCount** → draw visible meshlets

### 3.2 Indirect Draw Command Generation

Each visible meshlet becomes one indirect draw command:
```glsl
// meshlet_emit.comp or integrated into meshlet_cull.comp
VkDrawIndexedIndirectCommand cmd;
cmd.indexCount = meshlet.triangle_count * 3;
cmd.instanceCount = 1;
cmd.firstIndex = meshlet.triangle_offset;  // into triangle index buffer
cmd.vertexOffset = meshlet.vertex_offset;  // into vertex buffer
cmd.firstInstance = meshlet.chunk_slot;     // for scene data lookup
```

### 3.3 Vertex Shader Adaptation

The vertex shader needs to:
1. Read meshlet's local vertex data from SSBO (not traditional VB/IB binding)
2. Use `gl_DrawID` (requires `VK_KHR_shader_draw_parameters` / Vulkan 1.1) to identify which meshlet

**Two sub-approaches:**

**Approach A (VB/IB based)**: Keep vertex/index buffer binding, each meshlet gets its own indirect draw cmd with offsets. Simpler, compatible.

**Approach B (Pure SSBO)**: No VB/IB binding, vertex shader fetches everything from SSBO. More flexible for mesh shader parity but requires shader rewrite.

**Recommended**: Approach A for compute path (minimal shader changes), Approach B only for mesh shader path.

### 3.4 Render Pipeline Changes

Current: 1 vkCmdDrawIndexedIndirectCount call (chunks)
Phase 6: 1 vkCmdDrawIndexedIndirectCount call (meshlets) — more draw commands but each is smaller

---

## 4. VK_EXT_mesh_shader Hardware Path (MSHL-04)

### 4.1 Extension Detection

```rust
// At device creation:
let mesh_shader_supported = physical_device_extensions
    .iter()
    .any(|e| e.extension_name_as_c_str() == c"VK_EXT_mesh_shader");

// Enable if available:
if mesh_shader_supported {
    // Add to device create info
    // Query VkPhysicalDeviceMeshShaderFeaturesEXT
    // Check: taskShader = VK_TRUE, meshShader = VK_TRUE
}
```

### 4.2 Task Shader (meshlet.task)

```glsl
#extension GL_EXT_mesh_shader : require

layout(local_size_x = 32) in;  // 1 thread per meshlet group

// Task shader does culling (same as meshlet_cull.comp)
// Emits payload to mesh shader for visible meshlets
taskPayloadSharedEXT MeshletPayload payload;

void main() {
    // Cull meshlet
    if (visible) {
        // Pack visible meshlet ID into payload
    }
    EmitMeshTasksEXT(visible_count, 1, 1);
}
```

### 4.3 Mesh Shader (meshlet.mesh)

```glsl
#extension GL_EXT_mesh_shader : require

layout(local_size_x = 64) in;  // 1 thread per vertex (max 64)
layout(triangles, max_vertices = 64, max_primitives = 124) out;

void main() {
    // Read meshlet data from SSBO
    // Each thread outputs one vertex
    // Set triangle indices
    SetMeshOutputsEXT(vertex_count, triangle_count);
    gl_MeshVerticesEXT[gl_LocalInvocationIndex].gl_Position = ...;
}
```

### 4.4 Fallback Strategy (from CONTEXT.md)

- **Detection**: Application startup, one-time check
- **Selection**: `MeshletPipeline` trait with two implementations
- **No runtime switching**: Selected once at init

```rust
pub trait MeshletPipeline {
    fn create(device: &ash::Device, ...) -> Result<Self> where Self: Sized;
    fn record_dispatch(&self, cmd: vk::CommandBuffer, meshlet_count: u32, ...);
    fn record_draw(&self, cmd: vk::CommandBuffer, ...);
}

pub struct ComputeIndirectPath { /* compute cull + indirect draw */ }
pub struct MeshShaderPath { /* task + mesh shaders */ }

impl MeshletPipeline for ComputeIndirectPath { ... }
impl MeshletPipeline for MeshShaderPath { ... }
```

### 4.5 ash Mesh Shader Support

`ash` 0.38 includes `ash::ext::mesh_shader::Device` extension loader:
```rust
let mesh_shader_fn = ash::ext::mesh_shader::Device::new(&instance, &device);
// mesh_shader_fn.cmd_draw_mesh_tasks_ext(cmd, group_count_x, 1, 1);
```

---

## 5. Cluster LOD Transitions (MSHL-05)

### 5.1 Nanite-Style DAG (2-Level Initial)

From CONTEXT.md: "2 级 DAG（LOD0 原始 meshlet → LOD1 简化 meshlet）"

**LOD0**: Original meshlets from meshoptimizer
**LOD1**: Simplified meshlets (merged groups of 4 LOD0 meshlets → 1 LOD1 meshlet)

### 5.2 DAG Simplification

```rust
// For each group of ~4 adjacent LOD0 meshlets:
// 1. Merge their triangles
// 2. Run meshopt::simplify() to reduce triangle count by ~4x
// 3. Re-split into LOD1 meshlets via build_meshlets()
// 4. Lock shared boundary vertices (prevent simplification at group edges)
```

meshoptimizer provides:
```rust
meshopt::simplify(
    indices: &[u32],
    vertices: &[f32],
    target_count: usize,    // target index count
    target_error: f32,      // max error threshold
    options: SimplifyOptions,
) -> Vec<u32>;

// With locked vertices:
meshopt::simplify_with_attributes_and_locks(...)
```

### 5.3 SSE-Based LOD Selection

Reuse existing SSE (screen-space error) infrastructure from Phase 2:
- Each meshlet group has a `parent_error` (simplification error of LOD1 vs LOD0)
- At runtime: project error to screen pixels
- If projected error < threshold (e.g., 2px): use LOD1 (parent)
- If projected error >= threshold: use LOD0 (children)

### 5.4 Alpha Dither Transition

From CONTEXT.md: "Alpha dither 淡入淡出，相邻 LOD 级别的 meshlet 在 1-2 帧内 dither 过渡"

```glsl
// In fragment shader:
float dither_factor = compute_lod_transition_factor(sse, threshold);
float dither_pattern = bayer_matrix_8x8(gl_FragCoord.xy);
if (dither_factor < dither_pattern) discard;
```

### 5.5 Seamless Boundaries

From CONTEXT.md: "DAG 共享边界顶点天然消除接缝"

- LOD group boundaries lock shared vertices during simplification
- Adjacent meshlet groups at different LOD levels share edge vertices
- No skirt geometry needed (replacing Phase 3's approach)

---

## 6. Existing Codebase Integration Analysis

### 6.1 Files to Modify

| File | Change |
|------|--------|
| `Cargo.toml` | Add `meshopt` dependency |
| `src/meshing/greedy.rs` | Output `MeshletMesh` instead of `PackedMesh` |
| `src/meshing/packing.rs` | Add `MeshletMesh` struct, retain `PackedVertex` |
| `src/renderer/chunk_pool.rs` | Replace 3-buffer with meshlet SSBO layout |
| `src/renderer/cull_pipeline.rs` | Add meshlet-level culling (or new file) |
| `src/renderer/mesh_pipeline.rs` | Refactor to `MeshletPipeline` trait + implementations |
| `src/renderer/bindless.rs` | Register new meshlet SSBOs |
| `src/renderer/submit.rs` | Adapt submission for meshlet pipeline |
| `src/renderer/perf_counters.rs` | Add meshlet-specific counters |
| `src/runtime/scheduler.rs` | `RenderDelta::Upsert` carries `MeshletMesh` |
| `build.rs` | Add new shader compilation entries |
| `shaders/` | New: meshlet_cull.comp, meshlet_draw.vert/frag (compute path), meshlet.task, meshlet.mesh (mesh shader path) |

### 6.2 Current Data Flow (to be modified)

```
build_greedy_mesh() → PackedMesh
  → RenderDelta::Upsert { key, mesh: PackedMesh }
    → ChunkPool.record_upload() → staging → vertex_buffer + index_buffer + scene_buffer
      → chunk_cull.comp → dense_indirect
        → vkCmdDrawIndexedIndirectCount
```

### 6.3 Phase 6 Data Flow

```
build_greedy_mesh() → PackedMesh → build_meshlets() → MeshletMesh
  → RenderDelta::Upsert { key, mesh: MeshletMesh }
    → MeshletPool.record_upload() → staging → meshlet_meta_ssbo + meshlet_vertex_ssbo + meshlet_tri_ssbo
      → chunk_cull.comp → visible chunks (Level 1, retained)
        → meshlet_cull.comp → visible meshlets (Level 2, new)
          → MeshletPipeline::record_draw() → either:
            A) vkCmdDrawIndexedIndirectCount (compute path)
            B) vkCmdDrawMeshTasksIndirectCountEXT (mesh shader path)
```

### 6.4 BindlessTable Extensions

Current bindings 0-9. Phase 6 needs additional bindings:

| Binding | Type | Usage |
|---------|------|-------|
| 10 | STORAGE_BUFFER | Meshlet metadata SSBO |
| 11 | STORAGE_BUFFER | Meshlet vertex data SSBO |
| 12 | STORAGE_BUFFER | Meshlet triangle index SSBO |
| 13 | STORAGE_BUFFER | Visible meshlet indices (output of cull) |
| 14 | STORAGE_BUFFER | Meshlet indirect commands |
| 15 | STORAGE_BUFFER | Visible meshlet count |

### 6.5 Shader Compilation (build.rs additions)

```rust
// New shaders to compile:
"shaders/meshlet_cull.comp",
"shaders/meshlet_draw.vert",   // compute path vertex shader
"shaders/meshlet_draw.frag",   // shared fragment shader
// Optional (mesh shader path):
"shaders/meshlet.task",
"shaders/meshlet.mesh",
```

Note: shaderc supports `.task` and `.mesh` shader kinds via `shaderc::ShaderKind::TaskNV` / `MeshNV` or the EXT equivalents. May need to use `compile_into_spirv_with_options` with target env `vulkan1_2`.

---

## 7. Risk Assessment

### 7.1 High Risk

- **ChunkPool refactoring**: Moving from 3 fixed buffers to meshlet SSBO is a major restructure. Must maintain backward compatibility during transition or do atomic switch.
- **Two rendering paths**: Compute+indirect and mesh shader paths both need to produce identical output. Testing/verification is complex.

### 7.2 Medium Risk

- **meshopt crate compatibility**: Need to verify meshopt crate version supports Meshlet API with bounds computation.
- **Subgroup operations**: Not all GPUs have same subgroup size. Ballot operations need portability.
- **LOD DAG simplification**: Boundary vertex locking in meshoptimizer may have edge cases with greedy mesh topology.

### 7.3 Low Risk

- **Bindless extension**: Already established pattern, just adding more bindings.
- **build.rs changes**: Straightforward addition of new shader files.
- **perf_counters**: Additive, no breaking changes.

---

## 8. Validation Architecture

### 8.1 Testable Invariants

1. **Meshlet bounds correctness**: Every triangle in a meshlet must lie within its bounding sphere.
2. **Culling correctness**: With all culling disabled, rendering must match pre-meshlet output pixel-for-pixel (modulo triangle order).
3. **Compute vs mesh shader parity**: Both paths must produce identical visible meshlet sets.
4. **LOD seam-free**: At LOD transition boundaries, shared vertices must have identical positions.
5. **Performance regression**: Frame time must not increase vs pre-meshlet baseline for equivalent scene.

### 8.2 Integration Tests

- Meshlet generation: unit tests with known geometry → verify bounds, counts
- Culling: render with/without each mode → verify no artifacts
- Both paths: render same scene → diff framebuffer
- LOD transitions: fly camera through transition zones → screenshot comparison

---

## RESEARCH COMPLETE

Research covers all 5 requirements (MSHL-01 through MSHL-05) with:
- meshoptimizer API and integration strategy
- GPU data layout and SSBO design
- Two-level culling with subgroup compaction
- Compute+indirect and mesh shader dual-path architecture
- Nanite-style DAG LOD with alpha dither transitions
- Complete codebase integration analysis with file-level change map
