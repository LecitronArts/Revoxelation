---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
current_phase: 05.1-critical-bug-fixes-and-safety-hardening
status: executing
last_updated: "2026-03-27T10:07:52Z"
progress:
  total_phases: 12
  completed_phases: 6
  total_plans: 34
  completed_plans: 30
---

# Session State

## Project Reference

See: .planning/PROJECT.md

## Position

**Milestone:** v1.0 milestone
**Current phase:** 05.1-critical-bug-fixes-and-safety-hardening
**Status:** Executing (plan 02 complete, 4 remaining)

## Key Decisions (Phase 05.1 Plan 02)

- scratch_buffers: [Vec<(vk::Buffer, Allocation)>; 2] — per-frame ring indexed by current_frame
- free_stale_scratch(current_frame) drains current slot (safe after fence wait)
- paint() accepts current_frame to select correct ring slot

## Key Decisions (Phase 5 Plan 05)

- INITIAL_CAPACITY=1024 replaces MAX_RENDER_CHUNKS=881; capacity is runtime-dynamic
- Growth trigger: active > capacity * 0.9; growth factor: 2x doubling
- Growth between frames via submit_one_shot_commands (after fence wait, before command recording)
- vkCmdDrawIndexedIndirectCount uses draw_count_buffer from cull shader; max_draw_count = capacity
- CPU active_draw_count only used for cull dispatch workgroup count

## Key Decisions (Phase 5 Plan 03)

- GpuChunkInstance (48 bytes) replaces ChunkDrawMetadata — material_id replaces draw-command fields
- Unified scene_buffer: 4 regions (instances, indirect templates, draw slots, dense indirect) with 16-byte alignment
- Cull shader uses raw uint array with capacity-derived offsets; push constant expanded to { active_draw_count, capacity }
- Vertex shader reads scene_data.instances[gl_InstanceIndex]; BindlessTable binding 0 → scene_buffer (WHOLE_SIZE)
- ChunkPool reduced from 6 GPU buffers to 3 (vertex, index, scene_buffer)

## Key Decisions (Phase 5 Plan 04)

- BlockMaterial: 4 x u16 (top/side/bottom texture + flags) = 8 bytes, #[repr(C)] with bytemuck
- 10 procedural textures generated in Rust (no PNG loading yet): dirt, grass_top, grass_side, stone, sand, log_bark, log_end, planks, leaves, water
- Material SSBO at binding 8, texture array at binding 9 — matching BindlessTable reserved slots
- Fragment shader uses face normal threshold (y > 0.5 / y < -0.5) for top/bottom/side selection
- Vertex shader outputs v_block_id (flat uint), v_face_normal (vec3), v_uv (vec2) — replaces v_color
- nonuniformEXT used on texture array index for descriptor indexing safety

## Key Decisions (Phase 5 Plan 02)

- BindlessTable owns descriptor set 0 with 10 bindings (0-7 active, 8-9 reserved for Plan 04 materials)
- PARTIALLY_BOUND + UPDATE_AFTER_BIND flags on all 10 bindings
- register_buffer/register_image API for dynamic descriptor updates

## Key Decisions (Phase 5 Plan 01)

- Vulkan 1.2 hard requirement: 7 features (descriptor_indexing, etc.)
- PhysicalDeviceVulkan12Features via pNext chain
- No fallback: missing features produce descriptive error

## Key Decisions (Phase 4)

- Camera push constants (80 bytes), dynamic viewport/scissor
- StagingRing 32MB, GpuOnly chunk pool buffers
- GPU frustum + Hi-Z occlusion culling
- Pipeline cache, perf counters, shader hot-reload, runtime config

## Accumulated Context

### Roadmap Evolution
- Phase 05.1 inserted after Phase 5: Critical Bug Fixes and Safety Hardening (URGENT)

## Session Log

- 2026-03-22: Executed 03-07 — gap closure complete
- 2026-03-25: Roadmap restructured — 4 rendering phases inserted (4-7)
- 2026-03-25: Executed 04-01 through 04-07 — Phase 4 COMPLETE
- 2026-03-26: Executed 05-01 — Vulkan 1.2 hard requirement (BIND-01)
- 2026-03-26: Executed 05-02 — BindlessTable + pipeline migration (BIND-02)
- 2026-03-26: Executed 05-04 — BlockMaterial, texture array, shader sampling (BIND-04)
- 2026-03-26: Executed 05-03 — Unified scene_buffer, GpuChunkInstance, 6→3 buffers (BIND-03)
- 2026-03-26: Executed 05-05 — Dynamic capacity, IndirectCount draw (BIND-05) — Phase 5 COMPLETE
- 2026-03-27: Executed 05.1-02 — egui scratch buffer per-frame ring (FIX-02)
