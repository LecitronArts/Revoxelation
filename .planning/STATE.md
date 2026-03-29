---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
current_phase: 07-lighting-and-shadows
status: complete
last_updated: "2026-03-29T08:54:07Z"
progress:
  total_phases: 14
  completed_phases: 10
  total_plans: 52
  completed_plans: 52
---

# Session State

## Project Reference

See: .planning/PROJECT.md

## Position

**Milestone:** v1.0 milestone
**Current phase:** 07-lighting-and-shadows
**Status:** COMPLETE (Plan 07-05 done, Phase 07 fully complete)

## Key Decisions (Phase 07 Plan 05)

- LGHT-05-01: Sky renders as fullscreen triangle at depth=1.0 BEFORE geometry (geometry overwrites at closer depth)
- LGHT-05-02: Double-buffered sky params SSBO at binding 23 with inv_view_proj for ray direction reconstruction
- LGHT-05-03: DayNightCycle drives sun_elevation/azimuth/color/intensity when use_day_night_cycle=true
- LGHT-05-04: Fog color tracks sky horizon color through day-night cycle for seamless blending
- LGHT-05-05: Dynamic clear color from fog_color to roughly match procedural sky
- LGHT-05-06: Day-night cycle starts paused; default day_speed=600s (10-minute game day)

## Key Decisions (Phase 07 Plan 03)

- SSAO-01: Single-pass horizontal bilateral blur with image copy back to binding 17 (avoids descriptor swapping mid-command-buffer)
- SSAO-02: AO images kept in GENERAL layout throughout (simplifies barrier management for compute read/write)
- SSAO-03: Fragment shaders use textureSize(ssao_texture) for screen UV computation (no extra push constant needed)
- SSAO-04: Interleaved gradient noise for sample randomization (cheaper than blue noise texture)
- SSAO-05: SSAO dispatch after Hi-Z generation, reads depth from Hi-Z mip 0 at binding 7

## Key Decisions (Phase 07 Plan 04)

- LGHT-04-01: AO packed in word0 bits 24-25 (2 bits), repurposing former skirt bit (skirts removed in MSHL-05)
- LGHT-04-02: AO curve non-linear [0.2, 0.5, 0.75, 1.0] for better visual contrast at dark end
- LGHT-04-03: AO applied to ambient term only (subtract raw ambient, re-add with AO factor) — physically correct
- LGHT-04-04: Quad diagonal flip when opposite corner AO sums differ — Minecraft-style interpolation fix
- LGHT-04-05: is_opaque_for_ao uses sample_with_halo for cross-chunk boundary lookups — air=non-occluding
- LGHT-04-06: Transparent block FLAG_TRANSPARENT check deferred — requires MaterialTable access in meshing (noted as TODO)

## Key Decisions (Phase 07 Plan 02)

- CSM-01: Single 2D array image with 4 layers (not 4 separate images) for cleaner descriptor binding
- CSM-02: Comparison sampler (LESS_OR_EQUAL) enables hardware PCF via sampler2DArrayShadow
- CSM-03: Practical split scheme with λ=0.5 (blend linear/logarithmic) for cascade partitioning
- CSM-04: Texel grid snapping to prevent shadow shimmer during camera movement
- CSM-05: Depth bias constant=1.25, slope=1.75 for shadow acne prevention (adjustable via egui)
- CSM-06: 10% transition zone for cascade blending — flicker-free boundaries
- CSM-07: Border color FLOAT_OPAQUE_WHITE — out-of-bounds samples read as fully lit
- CSM-08: Z range extended 2x behind camera to catch shadow casters not in view
- CSM-09: Shadow passes reuse visible meshlet indirect buffer from cull pass
- CSM-10: cascade_matrices/splits written to LightingParams SSBO via direct mapped pointer write

## Key Decisions (Phase 07 Plan 01)

- LGHT-01: BindlessTable extended from 16 to 25 bindings (CSM, SSAO, lighting, PBR textures, point lights, sky, SSAO blur)
- BlockMaterial expanded from 8B (4 x u16) to 32B (16 x u16) with per-face MR/normal/emissive texture indices + emissive_intensity
- New flags: FLAG_HAS_MR=0x04, FLAG_HAS_NORMAL=0x08, FLAG_HAS_EMISSIVE_MAP=0x10, FLAG_IS_32X32=0x20
- Cook-Torrance BRDF in common.glsl: GGX NDF + Smith geometry + Schlick Fresnel + Lambertian diffuse
- LightingState: double-buffered SSBOs at binding 18, CPU-side sun elevation/azimuth/intensity/ambient controls
- PointLightManager: double-buffered SSBOs at binding 22, MAX_VISIBLE_POINT_LIGHTS=64
- PBR texture arrays (MR/normal/emissive) at bindings 19/20/21, 16x16 RGBA8 with mipmaps+aniso
- Push constants updated to VERTEX|FRAGMENT for all 3 pipeline paths (camera_pos needed for V vector)
- Default PBR: metallic=0.0, roughness=0.8 (dielectric, slightly rough)
- 32x32 arrays deferred — no blocks currently use FLAG_IS_32X32

## Key Decisions (Phase 06.1 Plan 08)

- REFAC-01: Sub-structs (VulkanCore, PipelineSet, PoolManager) as separate module files with accessor methods; flat fields retained for borrow-checker ergonomics
- REFAC-02: submit_frame decomposed into 8+ named sub-functions (wait_fence_and_prepare, acquire_image, begin_command_buffer, dispatch_chunk_cull, begin_render_pass, draw_meshlets, draw_egui, generate_hiz, present)
- REFAC-03: build_msaa_resources + build_framebuffers shared helpers eliminate ~60 lines of swapchain creation duplication; MsaaResources intermediate struct
- REFAC-04: stage_and_copy(ring, device, cmd, data, alignment, dst_buffer, dst_offset) replaces 10+ repetitions in chunk_pool.rs
- REFAC-05: 16 named binding constants (BINDING_SCENE through BINDING_MESHLET_COUNT) in bindless.rs
- REFAC-08: App::tick() method keeping event loop body clean (already in plan 07)

## Key Decisions (Phase 06.1 Plan 07)

- Camera process_keyboard takes delta_time: f32; movement = direction * move_speed * delta_time (POLISH-09)
- Camera process_mouse: 2-arg API with self.mouse_sensitivity (configurable, default 0.1)
- GpuChunkInstance: spawn_time + _pad_fade for 48→64 byte alignment; GLSL struct updated
- Fragment shader fade: alpha = clamp((current_time - spawn_time) / 0.5, 0.0, 1.0) with Bayer dither discard (POLISH-08)
- current_time passed via MeshletDrawPushConstants (84→88 bytes)
- Octree parent_of: skip (return None) for out-of-range parent coordinates instead of clamp (MED-12)
- hecs crate removed from Cargo.toml (REFAC-06); ChunkDrawMetadata struct deleted (REFAC-07)
- Skirt emission code removed from greedy.rs (REFAC-07/MSHL-05)

## Key Decisions (Phase 06.1 Plan 06)

- POLISH-05: All runtime unwrap()/panic!() replaced with anyhow Results and log framework
- POLISH-06: GpuReadbackCounters double-buffered HOST_VISIBLE readback for meshlet count
- MED-10: All eprintln! replaced with log::warn/error (diagnostics cleanup)
- MED-11: seed_input_commands guarded by cfg(test)

## Key Decisions (Phase 06.1 Plan 04)

- MeshletDrawPushConstants (84B) extends CameraUniforms with screen_height + sse_threshold for ComputeIndirectPath vertex shader
- common.glsl shared include: GpuChunkInstance, GpuMeshlet, decode_position, face_normal_from_index, Bayer 8x8, compute_lod_transition, CameraUniforms, MIN_LOD_DISTANCE
- chunk_cull.comp uses #include common.glsl for GpuChunkInstance (was duplicated inline)
- MeshShaderPath receives sse_threshold from renderer (was hardcoded 2.0)
- shaderc OptimizationLevel::Performance for SPIR-V compilation

## Key Decisions (Phase 06.1 Plan 05)

- MSAA_SAMPLES=TYPE_4 as pub const in swapchain.rs, shared by all pipelines
- 4-attachment MSAA render pass: MSAA color, MSAA depth, resolve color (swapchain), resolve depth (Hi-Z)
- Depth resolve via VkSubpassDescriptionDepthStencilResolve SAMPLE_ZERO mode (universally supported)
- MSAA intermediates use TRANSIENT_ATTACHMENT for potential lazily-allocated memory
- depth_store_op test updated: DONT_CARE correct on MSAA intermediates, only resolved attachments need STORE

## Key Decisions (Phase 06.1 Plan 03)

- HIGH-01: Per-frame descriptor sets [vk::DescriptorSet; 2] for egui — simpler than UPDATE_AFTER_BIND
- HIGH-02: destroy_buffer/image BEFORE allocator.free — correct Vulkan destruction order
- HIGH-07: TASK_EXT|MESH_EXT added to bindless stageFlags conditional on mesh_shader_supported
- MED-01: Near plane = row2 only (Vulkan z in [0,w]) — not row3+row2 (OpenGL)
- MED-02: TASK_SHADER_EXT|MESH_SHADER_EXT in barrier dstStageMask when mesh shaders active
- MED-03: Catch-all: ALL_COMMANDS + MEMORY_READ|WRITE + log::warn
- MED-04: StagingBuffer::write returns Result<()>; checks mapped memory
- MED-05: meshlet_pool.meshlet_capacity() for dynamic max_draw_count
- MED-09: Acquire for load, Release for store on cancel_flags AtomicBool

## Key Decisions (Phase 06.1 Plan 02)

- MeshletRange struct (6 fields) replaces (u32, u32) tuple for GPU buffer range tracking + free-list reuse
- BLOCK_SIZE=1/16m constant in scheduler.rs; world-space: key.x * CHUNK_EDGE * BLOCK_SIZE * lod_scale + half_edge
- Queued deactivation: direct Queued→Inactive + state_store.remove + cancel_flags.remove
- Loading deactivation: set cancel flag → handle_job_result checks was_cancelled → Inactive
- Job queue eviction: reject if new_task.sse_bits <= evicted.sse_bits
- MAX_RESULTS_PER_FRAME=16 caps run_mesh_sync to prevent frame stalls
- HashSet<ChunkKey> queued_set shadows VecDeque for O(1) membership checks

## Key Decisions (Phase 06.1 Plan 01)

- CRIT-07: Shader copy_mode push constant (1:1 vs 2x2) instead of vkCmdCopyImage — D32_SFLOAT→R32_SFLOAT cross-format copy not supported
- CRIT-02: Per-region BufferCopy with explicit offsets — align_up(16) shifts region boundaries at different capacities
- CRIT-03: Two separate cmd_push_constants calls — pipeline layout gap at [40..48) prohibits single combined push

## Key Decisions (Phase 6)

- meshopt crate (0.2) for meshlet generation: 64v/124t clusters, cone_weight=0.5
- MeshletPool manages 6 SSBOs (meta/vertex/tri/visible/indirect/count), BindlessTable bindings 10-15
- GpuMeshlet 64 bytes #[repr(C)] Pod+Zeroable (center, radius, cone_axis, cone_cutoff, offsets, counts, parent_error, group_id)
- Two-level cascade: chunk_cull.comp → meshlet_cull.comp (implicit cascade, no visibility mask SSBO)
- Subgroup ballot compaction: one atomicAdd per subgroup (not per thread)
- MeshletPipeline trait with ComputeIndirectPath (VB/IB + vkCmdDrawIndexedIndirectCount) and MeshShaderPath (task+mesh shaders + vkCmdDrawMeshTasksIndirectCountEXT)
- Automatic path selection at startup: mesh_shader_supported → MeshShaderPath else ComputeIndirectPath
- 2-level Nanite-style DAG LOD: LOD0 original + LOD1 simplified (meshopt::simplify with locked boundary vertices)
- SSE-based GPU LOD selection + Bayer 8x8 alpha dither for smooth transitions
- Border skirt removed — DAG shared boundary vertices replace skirt approach
- Push constants: 40 bytes for meshlet cull (+ sse_threshold + screen_height)
- Triangle indices: u8→u32 widening during MeshletPool::record_upload

- SequenceClock::next renamed to next_seq to avoid Iterator::next method confusion
- Boundary stubs kept with #[allow(dead_code)] — used in tests, needed later
- App.window_extent dead writes removed; field retained for future use

## Key Decisions (Phase 05.1 Plan 05)

- On staging Err, push failed delta back to front of VecDeque and return Ok (partial success)
- log::warn with deferred count and error message for diagnosability
- No separate retry counter — deferred deltas retry naturally next frame via existing VecDeque

## Key Decisions (Phase 05.1 Plan 04)

- DrawCmdPod #[repr(C)] Pod/Zeroable wrapper copies 5 u32 fields from vk::DrawIndexedIndirectCommand for safe bytemuck::bytes_of
- SAFETY comments on all 3 unsafe impl Send blocks (StagingAllocation, StagingRing, ChunkCullPipeline)
- All 8 `let _ =` in Renderer::drop replaced with log::warn error logging

## Key Decisions (Phase 5)

- Vulkan 1.2 hard requirement, BindlessTable set 0, unified scene_buffer, BlockMaterial textures, dynamic capacity + IndirectCount

## Key Decisions (Phase 4)

- Camera push constants (80 bytes), dynamic viewport/scissor
- StagingRing 32MB, GpuOnly chunk pool buffers
- GPU frustum + Hi-Z occlusion culling
- Pipeline cache, perf counters, shader hot-reload, runtime config

## Accumulated Context

### Roadmap Evolution
- Phase 05.1 inserted after Phase 5: Critical Bug Fixes and Safety Hardening (URGENT)
- Phase 06.1 inserted after Phase 6: Rendering Polish and Optimization (user-requested quality pass)

## Session Log

- 2026-03-22: Executed 03-07 — gap closure complete
- 2026-03-25: Roadmap restructured — 4 rendering phases inserted (4-7)
- 2026-03-25: Executed 04-01 through 04-07 — Phase 4 COMPLETE
- 2026-03-26: Executed 05-01 through 05-05 — Phase 5 COMPLETE
- 2026-03-27: Executed 05.1-01 through 05.1-06 — Phase 05.1 COMPLETE (all 9 FIX requirements resolved)
- 2026-03-28: Executed 06-01 through 06-05 — Phase 6 COMPLETE (all 5 MSHL requirements resolved)
- 2026-03-28: Phase 06.1 inserted — Rendering Polish and Optimization (9 POLISH requirements, 4 plans)
- 2026-03-28: Phase 06.1 expanded after deep code review — added 7 CRIT + 7 HIGH + 12 MED bugs + 8 REFAC items, total 8 plans
- 2026-03-28: Executed 06.1-01 — 4 CRIT Vulkan bugs fixed (depth store_op, scene_buffer grow, push constants, Hi-Z pass 0)
- 2026-03-28: Executed 06.1-02 — MeshletPool reclamation + streaming state fixes (CRIT-04~06, HIGH-03~06, MED-06~08)
- 2026-03-28: Executed 06.1-03 — Vulkan resource safety fixes (HIGH-01~02, HIGH-07, MED-01~05, MED-09)
- 2026-03-28: Executed 06.1-05 — Texture mipmaps + aniso filtering + MSAA 4x (POLISH-02, POLISH-03)
- 2026-03-28: Executed 06.1-06 — Error handling hardening + GPU readback counters (POLISH-05, POLISH-06, MED-10~11)
- 2026-03-28: Executed 06.1-07 — Camera smoothing + chunk fade-in + dead code cleanup (POLISH-08, POLISH-09, MED-12, REFAC-06~07)
- 2026-03-28: Executed 06.1-08 — Architecture refactoring (REFAC-01~05, REFAC-08) — Phase 06.1 COMPLETE
- 2026-03-29: Executed 07-01 — Directional light + PBR lighting model (LGHT-01) — Cook-Torrance BRDF, 25 bindless bindings, 32B BlockMaterial, LightingState, PointLightManager
- 2026-03-29: Executed 07-02 — 4-cascade CSM (LGHT-02) — D32_SFLOAT depth array, comparison sampler, PCF 3x3, cascade blending, texel snapping, egui controls
- 2026-03-29: Executed 07-04 — Voxel AO (LGHT-04) — 4-corner AO in greedy meshing, word0 bits 24-25, decode in shaders, ambient-only modulation, diagonal flip
- 2026-03-29: Executed 07-03 — SSAO (LGHT-03) — GTAO/HBAO+/classic compute pipelines, bilateral blur, combined AO (voxel×SSAO), egui controls
- 2026-03-29: Executed 07-05 — Sky/atmosphere/fog (LGHT-05) — Preetham sky model, day-night cycle, 4-type distance fog, egui controls — Phase 07 COMPLETE
