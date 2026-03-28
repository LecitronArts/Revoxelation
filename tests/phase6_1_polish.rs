//! Source-grep tests for Phase 06.1 Plan 01 — 4 CRIT Vulkan correctness fixes.
//!
//! These tests read source files and verify that the expected patterns
//! exist (or don't exist) after applying the CRIT fixes.

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
    // It should reference mip 0 / pass 0 / descriptor_sets[0] before the loop.
    let loop_pos = gen_body.find("for mip in 1..").unwrap();
    let before_loop = &gen_body[..loop_pos];

    // Before the loop, there should be a dispatch or copy for mip 0.
    let has_mip0_dispatch = before_loop.contains("cmd_dispatch")
        || before_loop.contains("cmd_copy_image")
        || before_loop.contains("cmd_blit_image");
    assert!(
        has_mip0_dispatch,
        "Hi-Z pass 0 should have a dedicated dispatch/copy before the downsample loop"
    );

    // Also check the shader for 1:1 copy mode support.
    let shader_src = std::fs::read_to_string("shaders/hiz_generate.comp")
        .expect("should read hiz_generate.comp");
    assert!(
        shader_src.contains("mode") || shader_src.contains("copy"),
        "hiz_generate.comp should support a 1:1 copy mode for pass 0"
    );
}
