# Phase 4: Rendering Foundation Overhaul - Research

**Researched:** 2026-03-25
**Status:** Complete

## Phase Boundary

Phase 4 transforms the renderer from a "working prototype" to a "correct and efficient rendering foundation." It addresses 7 requirements (REND-01 through REND-07) covering: real camera system, swapchain lifecycle, frustum+Hi-Z culling, GpuOnly memory, dependency injection, pipeline cache, and error propagation.

## Current Codebase Analysis

### Global State Architecture (Critical — REND-06)

Three `OnceLock<Mutex<>>` singletons exist:

1. **`src/renderer/globals.rs`** (18 lines):
   - `static RENDERER: OnceLock<Mutex<Renderer>>` — single renderer instance
   - `install_renderer()` / `renderer_state()` accessor functions

2. **`src/runtime/scheduler.rs`** (lines 92-101):
   - `static STREAMING: OnceLock<Mutex<StreamingState>>`
   - `static MESHING: OnceLock<Mutex<MeshingState>>`
   - Both initialized via `get_or_init()` lazy pattern

**Access pattern:** `src/app.rs` (line 84-92) locks renderer for egui output; `scheduler.rs` (line 136-139) locks for `submit_frame()`. All access is through `lock()`.

**Refactoring strategy:** Create `App` struct owning `Renderer`, `StreamingState`, `MeshingState` directly. Convert `run_frame()` to `App::run_frame(&mut self)`. Delete `globals.rs` entirely. This is straightforward — only 3 access sites.

### Projection System (Critical — REND-01)

**Current:** `shaders/chunk_mesh.vert` lines 30-39 use hardcoded `debug_project()`:
```glsl
vec3 centered = pos - vec3(32.0, 32.0, 32.0);
float scale = 1.0 / 400.0;
gl_Position = vec4(centered.x * scale, -centered.y * scale, centered.z * 0.5 + 0.5, 1.0);
```

**Issues:** No camera, no perspective, no view matrix, Y-flip hack, hardcoded center/scale.

**Replacement plan:** Push constants with `CameraUniforms { view_proj: Mat4, camera_pos: Vec3, _pad: f32 }` = 80 bytes (well within 128-byte minimum guarantee). glam provides `Mat4::perspective_rh` (Vulkan RH with [0,1] depth) and `Mat4::look_at_rh`.

### Swapchain Lifecycle (Critical — REND-02)

**Current state:**
- `src/renderer/swapchain.rs` creates swapchain once (lines 50-94)
- `src/app.rs` has NO resize handling — `WindowEvent::Resized` is not matched
- `submit.rs` does NOT check for `VK_ERROR_OUT_OF_DATE_KHR` or `VK_SUBOPTIMAL_KHR`
- Minimization (extent 0×0) will crash

**Recreation strategy:**
1. `device_wait_idle()` → destroy framebuffers, depth image/view, old image views
2. Create new swapchain with `old_swapchain` parameter for driver optimization
3. Recreate depth image to match new extent
4. Recreate framebuffers
5. Handle `acquire_next_image` returning OUT_OF_DATE → trigger recreation
6. Skip rendering entirely when extent is 0×0 (minimized)

### Memory Model (Critical — REND-05)

**Current:** ALL 6 ChunkPool buffers use `MemoryLocation::CpuToGpu` (chunk_pool.rs lines 224-275):
- vertex_buffer, index_buffer, metadata_buffer, indirect_template_buffer, draw_slot_buffer, dense_indirect_buffer

**Only GpuOnly usage:** depth image (swapchain.rs line 127)

**Migration path:**
1. Change all 6 buffers to `MemoryLocation::GpuOnly` with `TRANSFER_DST` usage flag
2. Create ring-buffer staging allocator: per-frame staging region from a large CpuToGpu buffer
3. Record `vkCmdCopyBuffer` commands instead of direct `write_allocation_bytes()`
4. Fence-based reclamation: track which staging regions are consumed per frame

**Transfer queue:** Check for dedicated TRANSFER queue family. If available, use it for async copies with semaphore sync. If not, fall back to graphics queue inline copies. gpu-allocator already handles the memory type selection.

### Compute Culling (Critical — REND-03, REND-04)

**Current cull shader** (`shaders/chunk_cull.comp`, 50 lines):
- Workgroup size: `local_size(1,1,1)` — terrible GPU occupancy
- Logic: only checks `index_count > 0 && indexCount > 0` — no spatial culling at all
- Output: sets `instanceCount = 0 or 1`

**Frustum culling plan:**
- Change to `local_size_x = 64` for better wave occupancy
- Push constants: 6 frustum planes (6 × vec4 = 96 bytes) + draw_count (4 bytes) = 100 bytes
  - Note: combined with camera push constants this exceeds 128-byte minimum. Solution: use separate push constant ranges per pipeline stage, or pass frustum planes via SSBO
- For each chunk: test AABB against 6 planes. If any plane rejects all 8 corners → cull.
- **Draw count buffer:** `atomicAdd` to count visible chunks → use with `vkCmdDrawIndexedIndirectCount`

**Available AABB data:** `ChunkDrawMetadata` already has `aabb_min` and `aabb_max` (lines 18-31 of chunk_pool.rs). This is the key enabler — no mesh changes needed.

**Hi-Z occlusion culling plan:**
- Generate Hi-Z pyramid: new `hiz_generate.comp` does 2×2 max downsampling of depth buffer
- Store as `R32_SFLOAT` mip chain image
- In cull shader: project AABB to screen-space rect → determine mip level → sample → compare max depth
- Use previous frame's depth (1-frame latency, acceptable for voxels)
- New `src/renderer/hiz.rs` manages Hi-Z image lifecycle

### Pipeline Architecture

**Current pipelines:**
1. `ChunkMeshPipeline` — graphics, 1 descriptor set (metadata SSBO), vertex input (packed uvec2)
2. `ChunkCullPipeline` — compute, 1 descriptor set (4 SSBOs: metadata, indirect_template, draw_slot, dense_indirect)
3. `EguiAshBackend` — graphics, 1 descriptor set (font sampler), push constants (screen size)

**Viewport/Scissor:** Currently set to fixed swapchain extent (mesh_pipeline.rs lines 120-132). Need dynamic state for resize support.

**Pipeline cache (REND-07):** Not currently used. `vkCreateGraphicsPipelines` and `vkCreateComputePipelines` pass `VK_NULL_HANDLE` for cache. Strategy: create pipeline cache at startup, load from `cache/pipeline.bin` if exists, save on exit.

### Frame Submission Sequence

Current flow in `submit.rs` (lines 18-190):
1. `wait_for_fences` (in-flight fence)
2. `acquire_next_image`
3. Reset fence + command buffer
4. Record: chunk_delta_uploads → compute_cull → barrier → render_pass → mesh_draw → egui → end
5. Submit with semaphores
6. Present

**Error handling:** Uses `.context()` throughout — proper anyhow propagation. But `src/runtime/scheduler.rs` line 136-139 calls `submit_frame` and may discard errors. Need to propagate to app level.

### Build System

`build.rs` compiles 5 shaders via shaderc: chunk_mesh.vert, chunk_mesh.frag, chunk_cull.comp, egui.vert, egui.frag. New shaders (hiz_generate.comp) must be added here.

## Validation Architecture

### Dimension Map

| Dimension | What to Validate | Method |
|-----------|-----------------|--------|
| D1: Behavior | Camera WASD+mouse, resize, frustum cull correctness | Integration tests + manual UAT |
| D2: API Contract | Push constant layout, descriptor set bindings | Validation layer + unit tests |
| D3: State Machine | Swapchain lifecycle states, frame sync | State transition tests |
| D4: Data Integrity | AABB correctness, Hi-Z pyramid values | GPU readback verification |
| D5: Error Handling | OUT_OF_DATE, minimize, submit errors | Forced error injection |
| D6: Performance | No queue_wait_idle, staging pipeline throughput | Frame time measurement |
| D7: Integration | Renderer ↔ streaming ↔ meshing via App struct | Full pipeline test |
| D8: Regression | Existing chunk rendering unchanged | Visual comparison |

### Test Strategy Per Plan

**04-01 (DI refactor):** Unit test — construct App without OnceLock. Grep for OnceLock → 0 matches.
**04-02 (Camera):** Visual test — perspective projection correct. Unit test — frustum plane extraction.
**04-03 (Swapchain):** Manual test — resize window, minimize, restore. Error path test — force OUT_OF_DATE.
**04-04 (GpuOnly):** Grep for CpuToGpu in chunk_pool → 0 matches. Validation layer — no memory visibility errors.
**04-05 (Frustum):** GPU readback — verify culled chunks have instanceCount=0. Camera behind chunks → 0 visible.
**04-06 (Hi-Z):** Readback Hi-Z pyramid — verify values. Occluded chunks produce 0 fragments.
**04-07 (Pipeline cache):** Second startup faster. HUD shows draw/triangle/fragment stats.

## Key Technical Risks

1. **Push constant size:** Combined camera (80B) + frustum planes (96B) = 176B exceeds 128B minimum. Mitigation: pass frustum planes via uniform buffer or SSBO instead.

2. **Ring-buffer staging complexity:** Need careful fence tracking per staging region. Mitigation: simple per-frame allocator with frame index → region mapping.

3. **Hi-Z temporal artifacts:** 1-frame latency can cause objects to pop. Mitigation: conservative test (use slightly larger AABB projection).

4. **Swapchain recreation race:** In-flight commands may reference old resources. Mitigation: `device_wait_idle()` before recreation.

5. **Transfer queue availability:** Not all GPUs have dedicated transfer queue. Mitigation: always fall back to graphics queue.

## Dependency Analysis

```
04-01 (DI refactor) ──→ 04-02 (Camera) ──→ 04-03 (Swapchain)
                                          ╲
                          04-04 (GpuOnly) ──→ 04-05 (Frustum) ──→ 04-06 (Hi-Z)
                                                                          │
                                                                   04-07 (Cache+HUD)
```

Wave 1: 04-01 (foundation — everything depends on clean architecture)
Wave 2: 04-02, 04-04 (parallel — camera and memory are independent)
Wave 3: 04-03, 04-05 (swapchain needs camera; frustum needs GpuOnly ready)
Wave 4: 04-06 (Hi-Z needs frustum infrastructure)
Wave 5: 04-07 (polish — needs everything stable)

## Existing Patterns to Preserve

- **64-byte ChunkDrawMetadata** alignment with aabb_min/aabb_max already present
- **SlotAllocator** swap-remove pattern for dense draw indices
- **Double-buffered frames** with fence/semaphore sync
- **anyhow::Result** error propagation throughout renderer
- **gpu-allocator** for all memory management (no manual vkAllocateMemory)
- **Packed vertex format** (uvec2, 8 bytes) — unchanged
- **CLOCKWISE front face** convention (Phase 3 decision)
- **shaderc** build-time compilation in build.rs

---

*Phase: 04-rendering-foundation-overhaul*
*Research completed: 2026-03-25*
