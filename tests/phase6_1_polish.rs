//! Source-grep tests for Phase 06.1 — Vulkan correctness fixes, streaming
//! state management, and rendering polish.
//!
//! Plan 01 tests: CRIT Vulkan bugs (depth store_op, scene grow, push constants, Hi-Z).
//! Plan 02 tests: MeshletPool reclamation, SSE world-coords, deactivation state handling,
//!                state store remove, cancel flag cleanup, dirty map cleanup, eviction
//!                comparison, real SSE enqueue, dirty HashSet dedup, mesh sync limit.

/// CRIT-01: Depth attachment store_op must be STORE (not DONT_CARE)
/// so the Hi-Z pyramid generator can read valid depth data.
#[test]
fn phase6_1_depth_store_op() {
    let src = std::fs::read_to_string("src/renderer/swapchain.rs")
        .expect("should read swapchain.rs");

    // The depth attachment description must use AttachmentStoreOp::STORE.
    assert!(
        src.contains("AttachmentStoreOp::STORE"),
        "depth attachment store_op should be STORE, not DONT_CARE"
    );

    // Verify no DONT_CARE remains on any depth-related store_op.
    // The only DONT_CARE should be on stencil ops (stencil_load_op / stencil_store_op).
    for (line_num, line) in src.lines().enumerate() {
        // Skip stencil-related lines — DONT_CARE is fine there.
        if line.contains("stencil_load_op") || line.contains("stencil_store_op") {
            continue;
        }
        // If a line has .store_op and DONT_CARE, that's a bug.
        if line.contains(".store_op(") && line.contains("DONT_CARE") {
            panic!(
                "swapchain.rs line {}: depth store_op still uses DONT_CARE:\n  {}",
                line_num + 1,
                line.trim()
            );
        }
    }
}

/// CRIT-02: scene_buffer grow_capacity must use per-region BufferCopy
/// entries with correct src/dst offsets (not a single flat copy).
#[test]
fn phase6_1_scene_grow_per_region() {
    let src = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("should read chunk_pool.rs");

    // Find the grow_capacity function body.
    let grow_start = src
        .find("fn grow_capacity")
        .expect("grow_capacity function must exist");
    let grow_body = &src[grow_start..];

    // Must call scene_buffer_region_offsets for both old and new capacities.
    assert!(
        grow_body.contains("scene_buffer_region_offsets(old_capacity)")
            || grow_body.contains("scene_buffer_region_offsets(old_cap"),
        "grow_capacity should compute old region offsets"
    );
    assert!(
        grow_body.contains("scene_buffer_region_offsets(new_capacity)")
            || grow_body.contains("scene_buffer_region_offsets(new_cap"),
        "grow_capacity should compute new region offsets"
    );

    // Must have multiple BufferCopy entries (at least 4 for the 4 regions).
    let copy_count = grow_body.matches("BufferCopy").count();
    assert!(
        copy_count >= 4,
        "grow_capacity should have at least 4 BufferCopy entries for 4 regions, found {copy_count}"
    );

    // Must NOT have a single flat scene copy with just .size(old_total_scene_size).
    let flat_copy_pattern = "scene_copy = vk::BufferCopy::default().size(old_total_scene_size)";
    assert!(
        !grow_body.contains(flat_copy_pattern),
        "grow_capacity must not use a single flat scene buffer copy"
    );
}

/// CRIT-03: Mesh shader push constants must be split into two
/// cmd_push_constants calls (one for TASK_EXT, one for MESH_EXT).
#[test]
fn phase6_1_push_constants_split() {
    let src = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("should read mesh_pipeline.rs");

    // Find the MeshShaderPath impl of record_draw.
    let impl_start = src
        .find("impl MeshletPipeline for MeshShaderPath")
        .expect("MeshShaderPath MeshletPipeline impl must exist");
    let impl_body = &src[impl_start..];

    // Must have at least two separate cmd_push_constants calls.
    let push_count = impl_body.matches("cmd_push_constants").count();
    assert!(
        push_count >= 2,
        "MeshShaderPath::record_draw should have at least 2 cmd_push_constants calls, found {push_count}"
    );

    // Must NOT have a combined TASK_EXT | MESH_EXT push constants call.
    assert!(
        !impl_body.contains("TASK_EXT | vk::ShaderStageFlags::MESH_EXT"),
        "must not combine TASK_EXT and MESH_EXT in a single push_constants call"
    );
}

/// CRIT-07: Hi-Z pass 0 must do a 1:1 copy from depth to hiz mip 0
/// (not a 2x2 downsample), and the downsample loop must start from mip 1.
#[test]
fn phase6_1_hiz_pass0() {
    let src = std::fs::read_to_string("src/renderer/hiz.rs")
        .expect("should read hiz.rs");

    // Find the generate() function body.
    let gen_start = src
        .find("pub fn generate(")
        .expect("generate function must exist");
    let gen_body = &src[gen_start..];

    // The downsample loop must start from mip 1 (not mip 0).
    // Look for the loop pattern: `for mip in 1..`
    assert!(
        gen_body.contains("for mip in 1.."),
        "Hi-Z generation loop should start from mip 1, not mip 0"
    );

    // Pass 0 must be handled separately (before the loop).
    let loop_pos = gen_body.find("for mip in 1..").unwrap();
    let before_loop = &gen_body[..loop_pos];

    // Before the loop, there should be a dispatch for mip 0 (1:1 copy mode).
    let has_mip0_dispatch = before_loop.contains("cmd_dispatch")
        || before_loop.contains("cmd_copy_image")
        || before_loop.contains("cmd_blit_image");
    assert!(
        has_mip0_dispatch,
        "Hi-Z pass 0 should have a dedicated dispatch/copy before the downsample loop"
    );

    // Verify the shader supports 1:1 copy mode.
    let shader_src = std::fs::read_to_string("shaders/hiz_generate.comp")
        .expect("should read hiz_generate.comp");
    assert!(
        shader_src.contains("copy_mode"),
        "hiz_generate.comp should have a copy_mode push constant for 1:1 pass 0"
    );
}

// ===========================================================================
// Plan 02 tests — MeshletPool reclamation + streaming state fixes
// ===========================================================================

/// CRIT-04: MeshletPool::record_remove must decrement active_meshlet_count
/// by the removed range's meshlet count (not just remove from HashMap).
#[test]
fn phase6_1_meshlet_pool_remove() {
    let src = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("should read chunk_pool.rs");

    // Find record_remove in MeshletPool (not ChunkPool).
    let meshlet_pool_start = src
        .find("impl MeshletPool")
        .expect("MeshletPool impl must exist");
    let meshlet_body = &src[meshlet_pool_start..];
    let remove_start = meshlet_body
        .find("fn record_remove")
        .expect("MeshletPool::record_remove must exist");
    let remove_body = &meshlet_body[remove_start..];

    // Must decrement active_meshlet_count.
    assert!(
        remove_body.contains("active_meshlet_count") && remove_body.contains("-="),
        "record_remove must decrement active_meshlet_count"
    );

    // Must track freed ranges for reuse.
    assert!(
        src.contains("free_ranges"),
        "MeshletPool should have a free_ranges field for reclaiming space"
    );
}

/// CRIT-05: SSE distance calculation must convert chunk key to world-space
/// coordinates using CHUNK_EDGE and BLOCK_SIZE (or lod_scale).
#[test]
fn phase6_1_sse_world_coords() {
    let src = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("should read scheduler.rs");

    // The SSE distance calculation must reference CHUNK_EDGE and some form
    // of block size or lod_scale for world-space conversion.
    assert!(
        src.contains("CHUNK_EDGE") && (src.contains("BLOCK_SIZE") || src.contains("lod_scale") || src.contains("chunk_world")),
        "SSE distance must use CHUNK_EDGE and block/lod scale for world-space conversion"
    );
}

/// CRIT-06: deactivate_chunk must handle Queued state → Inactive transition.
#[test]
fn phase6_1_deactivate_queued() {
    let src = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("should read scheduler.rs");

    // Find deactivate_chunk function.
    let fn_start = src
        .find("fn deactivate_chunk")
        .expect("deactivate_chunk must exist");
    let fn_body = &src[fn_start..];

    // Must explicitly handle Queued state.
    assert!(
        fn_body.contains("Queued") && fn_body.contains("Inactive"),
        "deactivate_chunk must handle Queued → Inactive transition"
    );
}

/// HIGH-03: ChunkStateStore must have a remove() method.
#[test]
fn phase6_1_state_store_remove() {
    let src = std::fs::read_to_string("src/streaming/state_store.rs")
        .expect("should read state_store.rs");

    assert!(
        src.contains("fn remove"),
        "ChunkStateStore must have a remove() method"
    );
}

/// HIGH-04: cancel_flags must be removed for Queued deactivations.
#[test]
fn phase6_1_cancel_flag_cleanup() {
    let src = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("should read scheduler.rs");

    // Find deactivate_chunk function.
    let fn_start = src
        .find("fn deactivate_chunk")
        .expect("deactivate_chunk must exist");
    let fn_body = &src[fn_start..];

    // Must call cancel_flags.remove in deactivate_chunk.
    assert!(
        fn_body.contains("cancel_flags.remove"),
        "deactivate_chunk must call cancel_flags.remove for cleanup"
    );
}

/// HIGH-05: Dirty mesh records with absent payload must be removed from dirty map.
#[test]
fn phase6_1_dirty_cleanup() {
    let src = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("should read scheduler.rs");

    // Find run_mesh_sync function.
    let fn_start = src
        .find("fn run_mesh_sync")
        .expect("run_mesh_sync must exist");
    let fn_body = &src[fn_start..];

    // When payload is None, must remove from dirty map (not just continue/skip).
    assert!(
        fn_body.contains("dirty.remove") || fn_body.contains("remove_absent") || fn_body.contains("dirty_map.remove"),
        "run_mesh_sync must remove dirty entries when payload is absent"
    );
}

/// HIGH-06: Job queue eviction must compare new task SSE vs evicted task SSE.
#[test]
fn phase6_1_eviction_comparison() {
    let src = std::fs::read_to_string("src/streaming/job_queue.rs")
        .expect("should read job_queue.rs");

    // Find enqueue function.
    let fn_start = src.find("fn enqueue").expect("enqueue must exist");
    let fn_body = &src[fn_start..];

    // Must compare SSE of new task vs evicted task (reject if lower).
    assert!(
        fn_body.contains("sse_bits") && (fn_body.contains("reject") || fn_body.contains("<=") || fn_body.contains("<")),
        "enqueue must compare new task SSE against evicted task SSE"
    );
}

/// MED-06: PrioritizedTask must use real SSE at enqueue time (not placeholder 1.0).
#[test]
fn phase6_1_real_sse_enqueue() {
    let src = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("should read scheduler.rs");

    // Find run_world_update function.
    let fn_start = src
        .find("fn run_world_update")
        .expect("run_world_update must exist");
    let fn_body = &src[fn_start..];

    // Must NOT have placeholder `sse: 1.0` or `sse_bits: 1.0f32.to_bits()`.
    assert!(
        !fn_body.contains("1.0f32.to_bits()"),
        "run_world_update must not use placeholder SSE 1.0 at enqueue time"
    );
}

/// MED-07: InvalidationTracker (MeshingState) should use HashSet for O(1) dirty dedup.
#[test]
fn phase6_1_dirty_hashset() {
    let src = std::fs::read_to_string("src/meshing/invalidation.rs")
        .expect("should read invalidation.rs");

    assert!(
        src.contains("HashSet"),
        "invalidation.rs must use HashSet for O(1) dirty dedup"
    );
}

/// MED-08: run_mesh_sync must limit job results processed per frame.
#[test]
fn phase6_1_mesh_sync_limit() {
    let src = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("should read scheduler.rs");

    // Find run_mesh_sync function.
    let fn_start = src
        .find("fn run_mesh_sync")
        .expect("run_mesh_sync must exist");
    let fn_body = &src[fn_start..];

    // Must have a per-frame results cap (max_results, recv_count limit, etc.).
    assert!(
        fn_body.contains("max_results") || fn_body.contains("MAX_RESULTS") || fn_body.contains("recv_cap"),
        "run_mesh_sync must have a per-frame result processing limit"
    );
}

// ===========================================================================
// Plan 03 tests — Vulkan resource safety, camera, staging, atomics
// ===========================================================================

/// HIGH-01: egui descriptor set must use per-frame descriptor sets (array of 2)
/// or UPDATE_AFTER_BIND to avoid use-after-free on font texture update.
#[test]
fn phase6_1_egui_descriptor_safety() {
    let src = std::fs::read_to_string("src/renderer/egui_backend.rs")
        .expect("should read egui_backend.rs");

    // Must have per-frame descriptor sets: an array field like `descriptor_sets: [vk::DescriptorSet; 2]`
    // or UPDATE_AFTER_BIND on the egui descriptor pool/layout.
    let has_per_frame = src.contains("[vk::DescriptorSet; 2]")
        || src.contains("[vk::DescriptorSet;2]");
    let has_update_after_bind = src.contains("UPDATE_AFTER_BIND");

    assert!(
        has_per_frame || has_update_after_bind,
        "egui_backend.rs must use per-frame descriptor sets [vk::DescriptorSet; 2] or UPDATE_AFTER_BIND"
    );
}

/// HIGH-02: destroy_allocated_buffer and destroy_allocated_image must call
/// device.destroy_buffer/image BEFORE allocator.free (correct Vulkan order).
#[test]
fn phase6_1_destroy_order() {
    let src = std::fs::read_to_string("src/renderer/helpers.rs")
        .expect("should read helpers.rs");

    // Check destroy_allocated_buffer: destroy_buffer must appear BEFORE .free
    let fn_start = src
        .find("fn destroy_allocated_buffer")
        .expect("destroy_allocated_buffer must exist");
    let fn_body = &src[fn_start..fn_start + 400.min(src.len() - fn_start)];
    let destroy_pos = fn_body.find("destroy_buffer").expect("must call destroy_buffer");
    let free_pos = fn_body.find(".free(").expect("must call .free()");
    assert!(
        destroy_pos < free_pos,
        "destroy_buffer must appear BEFORE .free() in destroy_allocated_buffer"
    );

    // Check destroy_allocated_image: destroy_image must appear BEFORE .free
    let fn_start2 = src
        .find("fn destroy_allocated_image")
        .expect("destroy_allocated_image must exist");
    let fn_body2 = &src[fn_start2..fn_start2 + 400.min(src.len() - fn_start2)];
    let destroy_pos2 = fn_body2.find("destroy_image").expect("must call destroy_image");
    let free_pos2 = fn_body2.find(".free(").expect("must call .free()");
    assert!(
        destroy_pos2 < free_pos2,
        "destroy_image must appear BEFORE .free() in destroy_allocated_image"
    );
}

/// HIGH-07: Bindless descriptor set layout stageFlags must include
/// TASK_EXT and MESH_EXT (or TASK_SHADER_BIT_EXT/MESH_SHADER_BIT_EXT)
/// when mesh shaders are supported.
#[test]
fn phase6_1_bindless_mesh_shader_flags() {
    let src = std::fs::read_to_string("src/renderer/bindless.rs")
        .expect("should read bindless.rs");

    assert!(
        src.contains("TASK_EXT") || src.contains("TASK_SHADER"),
        "bindless.rs must include TASK_EXT (or TASK_SHADER_BIT_EXT) in stageFlags"
    );
    assert!(
        src.contains("MESH_EXT") || src.contains("MESH_SHADER"),
        "bindless.rs must include MESH_EXT (or MESH_SHADER_BIT_EXT) in stageFlags"
    );
}

/// MED-01: Camera near-plane extraction must use Vulkan z∈[0,w] formula:
/// near plane = row2 of MVP (not row3+row2 which is OpenGL convention).
#[test]
fn phase6_1_camera_near_plane() {
    let src = std::fs::read_to_string("src/renderer/camera.rs")
        .expect("should read camera.rs");

    // Find extract_frustum_planes function.
    let fn_start = src
        .find("fn extract_frustum_planes")
        .expect("extract_frustum_planes must exist");
    let fn_body = &src[fn_start..];

    // Near plane must be row2 only (Vulkan), NOT row3 + row2 (OpenGL).
    // The near plane line should contain just "row2" without "row3 + row2".
    let near_line = fn_body.lines().find(|l| l.contains("near"));
    assert!(
        near_line.is_some(),
        "must have a near plane extraction line"
    );
    let near_text = near_line.unwrap();
    assert!(
        near_text.contains("row2") && !near_text.contains("row3"),
        "near plane must use row2 only (Vulkan z∈[0,w]), not row3+row2 (OpenGL): found '{}'",
        near_text.trim()
    );
}

/// MED-02: Pipeline barriers must include TASK_EXT | MESH_EXT
/// (or TASK_SHADER_BIT_EXT) in dstStageMask when mesh shaders are enabled.
#[test]
fn phase6_1_barrier_mesh_shader_stages() {
    let src = std::fs::read_to_string("src/renderer/submit.rs")
        .expect("should read submit.rs");

    assert!(
        src.contains("TASK_SHADER_EXT") || src.contains("TASK_SHADER"),
        "submit.rs must include TASK_SHADER_EXT in barrier dstStageMask"
    );
}

/// MED-03: transition_image_layout catch-all must use MEMORY_READ|MEMORY_WRITE
/// and emit log::warn (no silent zero-synchronization).
#[test]
fn phase6_1_transition_catchall_warn() {
    let src = std::fs::read_to_string("src/renderer/helpers.rs")
        .expect("should read helpers.rs");

    // Find transition_image_layout function.
    let fn_start = src
        .find("fn transition_image_layout")
        .expect("transition_image_layout must exist");
    let fn_body = &src[fn_start..];

    // Catch-all must use MEMORY_READ and MEMORY_WRITE.
    assert!(
        fn_body.contains("MEMORY_READ") && fn_body.contains("MEMORY_WRITE"),
        "transition_image_layout catch-all must use MEMORY_READ|MEMORY_WRITE"
    );

    // Must emit a warning log.
    assert!(
        fn_body.contains("warn!") || fn_body.contains("log::warn"),
        "transition_image_layout catch-all must emit log::warn"
    );
}

/// MED-04: StagingBuffer::write must return Result<()> and check for unmapped memory.
#[test]
fn phase6_1_staging_write_result() {
    let src = std::fs::read_to_string("src/renderer/staging.rs")
        .expect("should read staging.rs");

    // Find the fn write signature line — it must return Result.
    let write_line = src.lines().find(|l| l.contains("fn write(") || l.contains("fn write ("));
    assert!(
        write_line.is_some(),
        "StagingBuffer must have a fn write method"
    );
    assert!(
        write_line.unwrap().contains("Result"),
        "StagingBuffer::write must return Result<()>, got: '{}'",
        write_line.unwrap().trim()
    );
}

/// MED-05: max_draw_count must use meshlet_pool.meshlet_capacity() instead of
/// hardcoded INITIAL_MESHLET_CAPACITY.
#[test]
fn phase6_1_max_draw_count_dynamic() {
    let src = std::fs::read_to_string("src/renderer/submit.rs")
        .expect("should read submit.rs");

    // Find the meshlet rendering path.
    let render_start = src
        .find("used_meshlet_path")
        .expect("meshlet rendering path must exist");
    let render_body = &src[render_start..];

    // Must use meshlet_capacity() instead of INITIAL_MESHLET_CAPACITY.
    assert!(
        render_body.contains("meshlet_capacity()"),
        "max_draw_count must use meshlet_pool.meshlet_capacity() instead of hardcoded constant"
    );
    assert!(
        !render_body.contains("INITIAL_MESHLET_CAPACITY"),
        "max_draw_count must NOT use hardcoded INITIAL_MESHLET_CAPACITY in meshlet path"
    );
}

/// MED-09: cancel_flags must use Ordering::Release for store and Ordering::Acquire
/// for load (correct cross-thread visibility on ARM).
#[test]
fn phase6_1_atomic_ordering() {
    let job_runner_src = std::fs::read_to_string("src/streaming/job_runner.rs")
        .expect("should read job_runner.rs");
    let scheduler_src = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("should read scheduler.rs");

    // job_runner.rs: cancel flag loads should use Acquire (not Relaxed).
    let fn_start = job_runner_src
        .find("fn spawn_chunk_job")
        .or_else(|| job_runner_src.find("pub fn spawn_chunk_job"))
        .expect("spawn_chunk_job must exist");
    let fn_body = &job_runner_src[fn_start..];
    assert!(
        fn_body.contains("Ordering::Acquire"),
        "job_runner.rs cancel flag load must use Ordering::Acquire, not Relaxed"
    );

    // scheduler.rs: cancel flag stores should use Release (not Relaxed).
    let fn_start2 = scheduler_src
        .find("fn deactivate_chunk")
        .expect("deactivate_chunk must exist");
    let fn_body2 = &scheduler_src[fn_start2..];
    assert!(
        fn_body2.contains("Ordering::Release"),
        "scheduler.rs cancel flag store must use Ordering::Release, not Relaxed"
    );
}

// ===========================================================================
// Plan 04 tests — Shader parameterization, shared includes, SPIR-V optimization
// ===========================================================================

/// POLISH-01: No shader file should contain hardcoded `1080.0` as a magic
/// screen height. All screen_height values must come from push constants or UBO.
#[test]
fn phase6_1_no_hardcoded_1080() {
    let shader_dir = std::path::Path::new("shaders");
    let extensions = ["vert", "frag", "comp", "mesh", "task"];
    for ext in &extensions {
        for entry in std::fs::read_dir(shader_dir).expect("should read shaders dir") {
            let entry = entry.expect("should read dir entry");
            let path = entry.path();
            if path.extension().map_or(false, |e| e == *ext) {
                let source = std::fs::read_to_string(&path)
                    .unwrap_or_else(|_| panic!("should read {}", path.display()));
                assert!(
                    !source.contains("1080.0"),
                    "shader {} contains hardcoded 1080.0 — must use push constant or UBO",
                    path.display()
                );
            }
        }
    }
}

/// POLISH-04: Shared shader include file common.glsl must exist with
/// key shared definitions: face_normal_from_index, GpuChunkInstance, Bayer matrix.
#[test]
fn phase6_1_shared_shader_include() {
    let common = std::fs::read_to_string("shaders/common.glsl")
        .expect("shaders/common.glsl must exist");
    assert!(
        common.contains("face_normal"),
        "common.glsl must contain face_normal_from_index"
    );
    assert!(
        common.contains("GpuChunkInstance"),
        "common.glsl must contain GpuChunkInstance struct"
    );
    assert!(
        common.contains("Bayer") || common.contains("bayer"),
        "common.glsl must contain Bayer dither matrix"
    );
    assert!(
        common.contains("CameraUniforms") || common.contains("compute_lod_transition"),
        "common.glsl must contain CameraUniforms or LOD transition helper"
    );
}

/// POLISH-07: build.rs must use OptimizationLevel::Performance for SPIR-V compilation.
#[test]
fn phase6_1_shader_optimization() {
    let build = std::fs::read_to_string("build.rs").expect("should read build.rs");
    assert!(
        build.contains("OptimizationLevel::Performance"),
        "build.rs must use OptimizationLevel::Performance for SPIR-V compilation"
    );
}

/// POLISH-04: build.rs must use set_include_callback to resolve #include directives.
#[test]
fn phase6_1_include_callback() {
    let build = std::fs::read_to_string("build.rs").expect("should read build.rs");
    assert!(
        build.contains("set_include_callback") || build.contains("include_callback"),
        "build.rs must use set_include_callback for shader #include resolution"
    );
}

// ===========================================================================
// Plan 05 tests — Texture mipmaps, anisotropic filtering, and MSAA 4×
// ===========================================================================

/// POLISH-02: Texture array must generate mipmaps via vkCmdBlitImage
/// and have mip-level configuration.
#[test]
fn phase6_1_texture_mipmaps() {
    let src = std::fs::read_to_string("src/renderer/texture_array.rs")
        .expect("should read texture_array.rs");

    // Must contain mipmap generation (cmd_blit_image or mip_levels calculation).
    let has_blit = src.contains("cmd_blit_image");
    let has_mip = src.contains("mip_levels") && src.contains("log2");
    assert!(
        has_blit || has_mip,
        "texture_array.rs must generate mipmaps via cmd_blit_image or calculate mip_levels"
    );

    // Image creation must specify mip_levels > 1.
    assert!(
        src.contains(".mip_levels(mip_levels)"),
        "texture array image must be created with computed mip_levels (not hardcoded 1)"
    );
}

/// POLISH-03: MSAA 4× must be enabled — swapchain render pass and mesh pipelines
/// must reference SampleCountFlags::TYPE_4.
#[test]
fn phase6_1_msaa_enabled() {
    let swap_src = std::fs::read_to_string("src/renderer/swapchain.rs")
        .expect("should read swapchain.rs");
    let pipe_src = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("should read mesh_pipeline.rs");

    // Render pass must have TYPE_4 for MSAA attachments.
    assert!(
        swap_src.contains("TYPE_4"),
        "swapchain.rs render pass must use SampleCountFlags::TYPE_4 for MSAA"
    );

    // All graphics pipelines must use TYPE_4 (or MSAA_SAMPLES which resolves to TYPE_4).
    assert!(
        pipe_src.contains("TYPE_4") || pipe_src.contains("MSAA_SAMPLES"),
        "mesh_pipeline.rs must use SampleCountFlags::TYPE_4 (or MSAA_SAMPLES) in multisample state"
    );
}

/// POLISH-02: Sampler must have anisotropy_enable set to true.
#[test]
fn phase6_1_aniso_sampler() {
    let src = std::fs::read_to_string("src/renderer/texture_array.rs")
        .expect("should read texture_array.rs");

    assert!(
        src.contains("anisotropy_enable(true)"),
        "texture_array.rs sampler must have anisotropy_enable(true)"
    );

    // Must also set max_anisotropy to a reasonable value (>= 8.0).
    assert!(
        src.contains("max_anisotropy"),
        "texture_array.rs sampler must set max_anisotropy"
    );
}
