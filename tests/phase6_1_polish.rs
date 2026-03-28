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
