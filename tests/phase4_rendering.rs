//! Phase 4 rendering foundation tests.
//!
//! Source-grep tests that verify the OnceLock global state elimination
//! and App struct ownership model introduced by Plan 04-01.

// ---------------------------------------------------------------------------
// Task 1 tests
// ---------------------------------------------------------------------------

/// After Task 1: globals.rs deleted, no OnceLock in renderer/.
#[test]
fn rend_06_no_oncelock_in_renderer() {
    let renderer_dir = std::path::Path::new("src/renderer");

    // globals.rs should not exist.
    assert!(
        !renderer_dir.join("globals.rs").exists(),
        "src/renderer/globals.rs should be deleted"
    );

    // Check all .rs files in src/renderer/ for OnceLock.
    let mut oncelock_count = 0;
    for entry in std::fs::read_dir(renderer_dir).expect("src/renderer/ should exist") {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("should be able to read {}", path.display()));
            oncelock_count += content.matches("OnceLock").count();
        }
    }

    assert_eq!(
        oncelock_count, 0,
        "no file in src/renderer/ should contain OnceLock"
    );
}

#[test]
fn rend_06_app_struct_owns_renderer() {
    let app_source =
        std::fs::read_to_string("src/app.rs").expect("src/app.rs should exist");
    assert!(
        app_source.contains("struct App"),
        "src/app.rs should define struct App"
    );
    assert!(
        app_source.contains("renderer") && app_source.contains("Renderer"),
        "App struct should have a renderer: Renderer field"
    );
}

#[test]
fn rend_06_renderer_mod_no_globals_reexport() {
    let mod_source =
        std::fs::read_to_string("src/renderer/mod.rs").expect("src/renderer/mod.rs should exist");
    assert!(
        !mod_source.contains("pub mod globals"),
        "renderer/mod.rs should not declare pub mod globals"
    );
    assert!(
        !mod_source.contains("install_renderer"),
        "renderer/mod.rs should not re-export install_renderer"
    );
    assert!(
        !mod_source.contains("renderer_state"),
        "renderer/mod.rs should not re-export renderer_state"
    );
}

// ---------------------------------------------------------------------------
// Task 2 tests
// ---------------------------------------------------------------------------

#[test]
fn rend_06_scheduler_has_no_global_state() {
    let scheduler_source = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("src/runtime/scheduler.rs should exist");

    // No static OnceLock declarations.
    assert!(
        !scheduler_source.contains("OnceLock"),
        "scheduler.rs should not contain OnceLock"
    );

    // No static STREAMING or MESHING.
    assert!(
        !scheduler_source.contains("static STREAMING"),
        "scheduler.rs should not declare static STREAMING"
    );
    assert!(
        !scheduler_source.contains("static MESHING"),
        "scheduler.rs should not declare static MESHING"
    );
}

#[test]
fn rend_06_run_frame_takes_mutable_refs() {
    let scheduler_source = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("src/runtime/scheduler.rs should exist");

    assert!(
        scheduler_source.contains("streaming: &mut StreamingState"),
        "run_frame should accept streaming: &mut StreamingState parameter"
    );
    assert!(
        scheduler_source.contains("meshing: &mut MeshingState"),
        "run_frame should accept meshing: &mut MeshingState parameter"
    );
    assert!(
        scheduler_source.contains("renderer: &mut Renderer")
            || scheduler_source.contains("renderer: &mut crate::renderer::Renderer"),
        "run_frame should accept renderer: &mut Renderer parameter"
    );
}

#[test]
fn rend_06_app_struct_owns_all_subsystems() {
    let app_source =
        std::fs::read_to_string("src/app.rs").expect("src/app.rs should exist");
    assert!(
        app_source.contains("streaming") && app_source.contains("StreamingState"),
        "App struct should have a streaming: StreamingState field"
    );
    assert!(
        app_source.contains("meshing") && app_source.contains("MeshingState"),
        "App struct should have a meshing: MeshingState field"
    );
}

// ---------------------------------------------------------------------------
// Task 3 tests
// ---------------------------------------------------------------------------

#[test]
fn rend_06_env_logger_initialized() {
    let main_source =
        std::fs::read_to_string("src/main.rs").expect("src/main.rs should exist");
    assert!(
        main_source.contains("env_logger::init()") || main_source.contains("env_logger::builder()"),
        "src/main.rs should initialize env_logger before app::run()"
    );
}

#[test]
fn rend_06_submit_frame_errors_propagated() {
    let app_source =
        std::fs::read_to_string("src/app.rs").expect("src/app.rs should exist");
    assert!(
        app_source.contains("submit_frame"),
        "app.rs should call submit_frame"
    );
    assert!(
        app_source.contains("Err") || app_source.contains("if let Err") || app_source.contains("log::error!"),
        "app.rs should handle submit_frame errors"
    );
}

#[test]
fn rend_06_env_logger_in_cargo_toml() {
    let cargo_toml =
        std::fs::read_to_string("Cargo.toml").expect("Cargo.toml should exist");
    assert!(
        cargo_toml.contains("env_logger"),
        "Cargo.toml should include env_logger dependency"
    );
}

// ---------------------------------------------------------------------------
// Plan 04-02 Task 1 — FpsCamera and CameraUniforms
// ---------------------------------------------------------------------------

#[test]
fn rend_01_camera_view_proj_is_valid() {
    use revoxelation::renderer::camera::FpsCamera;
    let camera = FpsCamera::default();
    let uniforms = camera.view_proj(16.0 / 9.0);
    // view_proj must not be identity
    let identity: [[f32; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    assert_ne!(uniforms.view_proj, identity, "view_proj must not be identity");
    // All values must be finite
    for row in &uniforms.view_proj {
        for val in row {
            assert!(val.is_finite(), "view_proj must contain only finite values");
        }
    }
}

#[test]
fn rend_01_camera_uniforms_size_is_80_bytes() {
    use revoxelation::renderer::camera::CameraUniforms;
    assert_eq!(
        std::mem::size_of::<CameraUniforms>(),
        80,
        "CameraUniforms must be exactly 80 bytes for push constants"
    );
}

#[test]
fn rend_01_camera_movement_changes_position() {
    use revoxelation::renderer::camera::FpsCamera;
    let mut camera = FpsCamera::default();
    let original_pos = camera.position;
    camera.process_keyboard(revoxelation::renderer::camera::CameraKey::Forward, true, 1.0 / 60.0);
    assert_ne!(
        camera.position, original_pos,
        "Camera position should change after forward movement"
    );
}

#[test]
fn rend_01_camera_pitch_clamped() {
    use revoxelation::renderer::camera::FpsCamera;
    let mut camera = FpsCamera::default();
    // Try to look straight down — pitch should be clamped
    camera.process_mouse(0.0, 10000.0, 0.1);
    assert!(
        camera.pitch >= -89.0_f32.to_radians() - 0.01,
        "Pitch should be clamped to >= -89 degrees, got {}",
        camera.pitch.to_degrees()
    );
    assert!(
        camera.pitch <= 89.0_f32.to_radians() + 0.01,
        "Pitch should be clamped to <= 89 degrees, got {}",
        camera.pitch.to_degrees()
    );
    // Reset and look straight up
    camera.pitch = 0.0;
    camera.process_mouse(0.0, -10000.0, 0.1);
    assert!(
        camera.pitch >= -89.0_f32.to_radians() - 0.01,
        "Pitch should be clamped to >= -89 degrees after looking up, got {}",
        camera.pitch.to_degrees()
    );
    assert!(
        camera.pitch <= 89.0_f32.to_radians() + 0.01,
        "Pitch should be clamped to <= 89 degrees after looking up, got {}",
        camera.pitch.to_degrees()
    );
}

// ---------------------------------------------------------------------------
// Plan 04-04 Task 1 — StagingRing allocator
// ---------------------------------------------------------------------------

#[test]
fn rend_05_staging_ring_allocation_returns_valid_offset() {
    use revoxelation::renderer::staging_ring::StagingRing;
    // 32 MB total, 2 frames → 16 MB per frame
    let mut ring = StagingRing::new_layout_only(32 * 1024 * 1024, 2);
    let a1 = ring.allocate(256, 16).expect("first allocation should succeed");
    let a2 = ring.allocate(512, 16).expect("second allocation should succeed");
    // First allocation starts at offset 0 within frame 0 region
    assert_eq!(a1.offset, 0, "first alloc offset should be 0");
    // Second allocation must come after the first, respecting alignment
    assert!(
        a2.offset >= 256,
        "second alloc offset ({}) must be >= 256",
        a2.offset
    );
    // Second allocation must be 16-byte aligned
    assert_eq!(
        a2.offset % 16,
        0,
        "second alloc offset must be 16-byte aligned"
    );
}

#[test]
fn rend_05_staging_ring_frame_advance_resets_offset() {
    use revoxelation::renderer::staging_ring::StagingRing;
    let mut ring = StagingRing::new_layout_only(32 * 1024 * 1024, 2);
    // Allocate in frame 0
    let _a1 = ring.allocate(1024, 16).expect("allocation should succeed");
    // Advance to frame 1 — cursor resets to frame 1 region start
    ring.advance_frame();
    let a2 = ring.allocate(256, 16).expect("allocation in new frame should succeed");
    // After advancing, offset should be in the second frame's region (16 MB into the buffer)
    let frame_size: u64 = 16 * 1024 * 1024;
    assert_eq!(
        a2.offset, frame_size,
        "after advance_frame, offset should start at second frame region ({}), got {}",
        frame_size, a2.offset
    );
}

// ---------------------------------------------------------------------------
// Plan 04-04 Task 2 — ChunkPool GpuOnly + staging upload path
// ---------------------------------------------------------------------------

#[test]
fn rend_05_chunk_pool_uses_gpu_only() {
    let source = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("src/renderer/chunk_pool.rs should exist");
    // All buffer creations should use GpuOnly
    let gpu_only_count = source.matches("MemoryLocation::GpuOnly").count();
    let cpu_to_gpu_count = source.matches("MemoryLocation::CpuToGpu").count();
    assert!(
        gpu_only_count >= 6,
        "chunk_pool.rs must have at least 6 GpuOnly allocations, found {gpu_only_count}"
    );
    assert_eq!(
        cpu_to_gpu_count, 0,
        "chunk_pool.rs must have zero CpuToGpu allocations, found {cpu_to_gpu_count}"
    );
}

#[test]
fn rend_05_no_queue_wait_idle_in_hot_path() {
    let submit_source = std::fs::read_to_string("src/renderer/submit.rs")
        .expect("src/renderer/submit.rs should exist");
    let chunk_pool_source = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("src/renderer/chunk_pool.rs should exist");
    assert!(
        !submit_source.contains("queue_wait_idle"),
        "submit.rs must NOT contain queue_wait_idle"
    );
    assert!(
        !chunk_pool_source.contains("queue_wait_idle"),
        "chunk_pool.rs must NOT contain queue_wait_idle"
    );
}

#[test]
fn rend_05_depth_image_has_sampled_usage() {
    let source = std::fs::read_to_string("src/renderer/swapchain.rs")
        .expect("src/renderer/swapchain.rs should exist");
    // Depth image must include SAMPLED usage for Hi-Z pyramid
    assert!(
        source.contains("DEPTH_STENCIL_ATTACHMENT") && source.contains("SAMPLED"),
        "swapchain.rs depth image creation must include both DEPTH_STENCIL_ATTACHMENT and SAMPLED usage flags"
    );
}

#[test]
fn rend_05_chunk_pool_has_record_uploads() {
    let source = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("src/renderer/chunk_pool.rs should exist");
    assert!(
        source.contains("record_uploads"),
        "chunk_pool.rs must have a record_uploads method"
    );
    assert!(
        source.contains("cmd_copy_buffer"),
        "chunk_pool.rs must contain cmd_copy_buffer for staging uploads"
    );
}

#[test]
fn rend_05_submit_uses_staging_ring() {
    let source = std::fs::read_to_string("src/renderer/submit.rs")
        .expect("src/renderer/submit.rs should exist");
    assert!(
        source.contains("StagingRing") || source.contains("staging_ring"),
        "submit.rs must reference StagingRing for upload coordination"
    );
}

// ---------------------------------------------------------------------------
// Plan 04-03 Task 1 — Swapchain recreation function
// ---------------------------------------------------------------------------

#[test]
fn rend_02_swapchain_recreate_fn_exists() {
    let source = std::fs::read_to_string("src/renderer/swapchain.rs")
        .expect("src/renderer/swapchain.rs should exist");
    assert!(
        source.contains("pub fn recreate_swapchain"),
        "swapchain.rs must contain a pub fn recreate_swapchain function"
    );
}

#[test]
fn rend_02_swapchain_old_handle_passed() {
    let source = std::fs::read_to_string("src/renderer/swapchain.rs")
        .expect("src/renderer/swapchain.rs should exist");
    assert!(
        source.contains("old_swapchain"),
        "swapchain.rs recreation must pass old_swapchain handle to create_info"
    );
}

// ---------------------------------------------------------------------------
// Plan 04-03 Task 2 — Resize and OUT_OF_DATE/SUBOPTIMAL handling
// ---------------------------------------------------------------------------

#[test]
fn rend_02_submit_handles_out_of_date() {
    let source = std::fs::read_to_string("src/renderer/submit.rs")
        .expect("src/renderer/submit.rs should exist");
    assert!(
        source.contains("OUT_OF_DATE") || source.contains("ErrorOutOfDateKhr") || source.contains("ERROR_OUT_OF_DATE_KHR"),
        "submit.rs must handle OUT_OF_DATE errors from acquire/present"
    );
}

#[test]
fn rend_02_app_handles_resize_event() {
    let source = std::fs::read_to_string("src/app.rs")
        .expect("src/app.rs should exist");
    assert!(
        source.contains("Resized") && source.contains("recreate_swapchain"),
        "app.rs must handle WindowEvent::Resized and trigger swapchain recreation"
    );
}

#[test]
fn rend_02_minimized_skips_rendering() {
    let source = std::fs::read_to_string("src/app.rs")
        .expect("src/app.rs should exist");
    // Check that app checks for zero-size extent before rendering
    assert!(
        (source.contains("width == 0") || source.contains("width < 1") || source.contains("is_minimized"))
            && (source.contains("height == 0") || source.contains("height < 1") || source.contains("is_minimized")),
        "app.rs must check for zero-size window (minimized) and skip rendering"
    );
}

#[test]
fn rend_02_swapchain_device_wait_idle() {
    let source = std::fs::read_to_string("src/renderer/swapchain.rs")
        .expect("src/renderer/swapchain.rs should exist");
    assert!(
        source.contains("device_wait_idle"),
        "swapchain.rs recreation must call device_wait_idle before destroying resources"
    );
}

// ---------------------------------------------------------------------------
// Plan 04-05 Task 1 — Frustum plane extraction
// ---------------------------------------------------------------------------

#[test]
fn rend_03_frustum_planes_from_identity_matrix() {
    use glam::Mat4;
    use revoxelation::renderer::camera::extract_frustum_planes;
    let vp = Mat4::IDENTITY;
    let planes = extract_frustum_planes(&vp);
    // 6 planes, each with non-zero normal and finite w
    for (i, plane) in planes.planes.iter().enumerate() {
        let len = (plane[0] * plane[0] + plane[1] * plane[1] + plane[2] * plane[2]).sqrt();
        assert!(
            len > 0.0,
            "plane {i} should have a non-zero normal, got length {len}"
        );
        assert!(
            plane[3].is_finite(),
            "plane {i} w component should be finite, got {}",
            plane[3]
        );
    }
}

#[test]
fn rend_03_point_inside_frustum_passes() {
    use glam::{Mat4, Vec3};
    use revoxelation::renderer::camera::{extract_frustum_planes, FpsCamera};
    // Camera at origin looking +Z... but our camera looks -Z at yaw=0
    // So let's create a perspective projection looking along -Z and test point at (0,0,-5)
    let mut camera = FpsCamera::default();
    camera.position = Vec3::ZERO;
    camera.yaw = 0.0;
    camera.pitch = 0.0;
    let uniforms = camera.view_proj(1.0);
    let vp = Mat4::from_cols_array_2d(&uniforms.view_proj);
    let planes = extract_frustum_planes(&vp);
    // Point at (0, 0, -5) should be inside the frustum (camera looks along -Z)
    let point = Vec3::new(0.0, 0.0, -5.0);
    for (i, plane) in planes.planes.iter().enumerate() {
        let normal = Vec3::new(plane[0], plane[1], plane[2]);
        let dist = normal.dot(point) + plane[3];
        assert!(
            dist >= -0.01,
            "point (0,0,-5) should be inside plane {i}, got dist={dist}"
        );
    }
}

#[test]
fn rend_03_point_behind_camera_fails() {
    use glam::{Mat4, Vec3};
    use revoxelation::renderer::camera::{extract_frustum_planes, FpsCamera};
    let mut camera = FpsCamera::default();
    camera.position = Vec3::ZERO;
    camera.yaw = 0.0;
    camera.pitch = 0.0;
    let uniforms = camera.view_proj(1.0);
    let vp = Mat4::from_cols_array_2d(&uniforms.view_proj);
    let planes = extract_frustum_planes(&vp);
    // Point at (0,0,5) is BEHIND the camera (camera looks -Z)
    let point = Vec3::new(0.0, 0.0, 5.0);
    let mut outside_count = 0;
    for plane in &planes.planes {
        let normal = Vec3::new(plane[0], plane[1], plane[2]);
        let dist = normal.dot(point) + plane[3];
        if dist < 0.0 {
            outside_count += 1;
        }
    }
    assert!(
        outside_count >= 1,
        "point (0,0,5) should be outside at least one frustum plane"
    );
}

#[test]
fn rend_03_frustum_planes_struct_size() {
    use revoxelation::renderer::camera::FrustumPlanes;
    assert_eq!(
        std::mem::size_of::<FrustumPlanes>(),
        96,
        "FrustumPlanes must be exactly 96 bytes (6 * vec4)"
    );
}

// ---------------------------------------------------------------------------
// Plan 04-02 Task 2 — Push constants and dynamic viewport in mesh pipeline
// ---------------------------------------------------------------------------

#[test]
fn rend_01_vertex_shader_uses_push_constant_view_proj() {
    let shader_source = std::fs::read_to_string("shaders/chunk_mesh.vert")
        .expect("shaders/chunk_mesh.vert should exist");
    assert!(
        shader_source.contains("push_constant"),
        "Vertex shader must contain push_constant block"
    );
    assert!(
        shader_source.contains("view_proj"),
        "Vertex shader must contain view_proj"
    );
    assert!(
        !shader_source.contains("debug_project"),
        "Vertex shader must NOT contain debug_project"
    );
}

#[test]
fn rend_01_mesh_pipeline_has_dynamic_viewport() {
    let source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("src/renderer/mesh_pipeline.rs should exist");
    assert!(
        source.contains("DynamicState::VIEWPORT") || source.contains("DYNAMIC_STATE_VIEWPORT"),
        "Mesh pipeline must use dynamic viewport state"
    );
}

#[test]
fn rend_01_mesh_pipeline_has_push_constant_range() {
    let source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("src/renderer/mesh_pipeline.rs should exist");
    assert!(
        source.contains("push_constant_range") || source.contains("PushConstantRange"),
        "Mesh pipeline must define push constant ranges"
    );
}
