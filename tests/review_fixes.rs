//! Regression tests for post-review fixes.

/// Helper: read MeshletPool source from whichever file it lives in.
/// After Phase 5, MeshletPool is in meshlet_pool.rs; chunk_pool.rs re-exports it.
fn read_meshlet_pool_source() -> String {
    let chunk_pool =
        std::fs::read_to_string("src/renderer/chunk_pool.rs").expect("should read chunk_pool.rs");
    let meshlet_pool =
        std::fs::read_to_string("src/renderer/meshlet_pool.rs").unwrap_or_default();
    format!("{chunk_pool}\n{meshlet_pool}")
}

#[test]
fn review_swapchain_image_destroy_order_is_correct_everywhere() {
    let helpers =
        std::fs::read_to_string("src/renderer/helpers.rs").expect("should read helpers.rs");
    let swapchain =
        std::fs::read_to_string("src/renderer/swapchain.rs").expect("should read swapchain.rs");
    let renderer =
        std::fs::read_to_string("src/renderer/mod.rs").expect("should read renderer/mod.rs");

    let helper_start = helpers
        .find("fn destroy_allocated_image")
        .expect("destroy_allocated_image must exist");
    let helper_body = &helpers[helper_start..helper_start + 400.min(helpers.len() - helper_start)];
    let helper_destroy = helper_body
        .find("destroy_image")
        .expect("helper must destroy image");
    let helper_free = helper_body
        .find(".free(")
        .expect("helper must free allocation");
    assert!(
        helper_destroy < helper_free,
        "destroy_allocated_image must destroy image before freeing allocation"
    );

    for (label, src) in [("swapchain.rs", &swapchain), ("renderer/mod.rs", &renderer)] {
        let first_free = src.find(".free(").unwrap_or(usize::MAX);
        let first_destroy = src.find("destroy_image(").unwrap_or(usize::MAX);
        assert!(
            first_destroy < first_free,
            "{label} must destroy swapchain-dependent images before allocator.free()"
        );
    }
}

#[test]
fn review_shadow_pass_runs_before_main_cull_and_builds_its_own_draw_list() {
    let submit = std::fs::read_to_string("src/renderer/submit.rs").expect("should read submit.rs");
    let combined = read_meshlet_pool_source();

    let shadow_call = submit
        .find("record_csm_shadow_passes")
        .expect("submit.rs must call record_csm_shadow_passes");
    let cull_call = submit
        .find("dispatch_chunk_cull")
        .expect("submit.rs must call dispatch_chunk_cull");
    assert!(
        shadow_call < cull_call,
        "shadow passes should run before main-view culling overwrites visible meshlet buffers"
    );

    assert!(
        combined.contains("record_shadow_draw_setup")
            || combined.contains("shadow_draw_commands"),
        "MeshletPool should build a dedicated shadow draw list instead of reusing camera-visible results"
    );
}

#[test]
fn review_shadow_depth_bias_comes_from_runtime_config() {
    let shadow = std::fs::read_to_string("src/renderer/shadow.rs").expect("should read shadow.rs");
    let submit = std::fs::read_to_string("src/renderer/submit.rs").expect("should read submit.rs");

    assert!(
        shadow.contains("DynamicState::DEPTH_BIAS"),
        "shadow pipeline should enable dynamic depth bias"
    );
    assert!(
        submit.contains("cmd_set_depth_bias")
            && (submit.contains("shadow_config.bias_constant")
                || submit.contains("config.shadow.bias_constant"))
            && (submit.contains("shadow_config.bias_slope")
                || submit.contains("config.shadow.bias_slope")),
        "shadow pass should push runtime-configured depth bias values each frame"
    );
}

#[test]
fn review_ssao_is_recreated_when_surface_or_quality_changes() {
    let app = std::fs::read_to_string("src/app.rs").expect("should read app.rs");

    let recreates_ssao = app.contains("ssao_pass.take()")
        || app.contains(".recreate(&mut self.renderer")
        || app.contains("sync_ssao_pass");
    assert!(
        recreates_ssao,
        "app.rs should recreate SSAO resources when size or half-resolution config changes"
    );
}

#[test]
fn review_error_chunks_can_retry_when_backoff_expires() {
    let scheduler =
        std::fs::read_to_string("src/runtime/scheduler.rs").expect("should read scheduler.rs");

    assert!(
        scheduler.contains("ChunkState::Error")
            && scheduler.contains("next_retry_frame")
            && scheduler.contains("frame_index")
            && scheduler.contains("ChunkState::Queued"),
        "scheduler.rs should requeue Error chunks when next_retry_frame has elapsed"
    );
}

#[test]
fn review_egui_input_handles_text_scale_and_modifiers() {
    let app = std::fs::read_to_string("src/app.rs").expect("should read app.rs");

    assert!(
        app.contains("pixels_per_point")
            || app.contains("scale_factor")
            || app.contains("native_pixels_per_point"),
        "egui raw input should include DPI / pixels_per_point information"
    );
    assert!(
        app.contains("WindowEvent::Ime")
            || app.contains("Event::Text")
            || app.contains("ModifiersChanged"),
        "app.rs should forward text input and modifier changes to egui"
    );
}

#[test]
fn review_idle_redraw_is_not_unconditional_busy_loop() {
    let app = std::fs::read_to_string("src/app.rs").expect("should read app.rs");

    let about_to_wait = app
        .find("Event::AboutToWait")
        .expect("AboutToWait handler must exist");
    let redraw_call = app[about_to_wait..]
        .find("window.request_redraw()")
        .expect("AboutToWait should still be able to request redraws");
    let redraw_region = &app[about_to_wait
        ..about_to_wait + redraw_call + 120.min(app.len() - about_to_wait - redraw_call)];
    assert!(
        redraw_region.contains("if ")
            || redraw_region.contains("should_redraw")
            || redraw_region.contains("WaitUntil"),
        "AboutToWait should not request redraw unconditionally every loop"
    );
}

#[test]
fn review_meshlet_runtime_path_is_initialized_in_run() {
    let app = std::fs::read_to_string("src/app.rs").expect("should read app.rs");

    assert!(
        app.contains("renderer.meshlet_pool = Some(")
            && app.contains("MeshletPool::new(&mut renderer)?"),
        "run() should create meshlet_pool so meshlet rendering and shadow passes can execute at runtime"
    );
    assert!(
        app.contains("register_meshlet_buffers"),
        "run() should register meshlet buffers into bindless bindings 10-15"
    );
    assert!(
        app.contains("create_meshlet_pipeline")
            && app.contains("renderer.meshlet_pipeline = Some("),
        "run() should create the meshlet render pipeline instead of leaving meshlet rendering dormant"
    );
}

#[test]
fn review_render_deltas_sync_chunk_and_meshlet_pools() {
    let renderer =
        std::fs::read_to_string("src/renderer/mod.rs").expect("should read renderer/mod.rs");

    assert!(
        renderer.contains("chunk_pool.record_upload")
            && renderer.contains("meshlet_pool.record_upload")
            && renderer.contains("chunk_pool.prepare_remove")
            && renderer.contains("meshlet_pool.record_remove")
            && renderer.contains("pending_chunk_deltas"),
        "record_chunk_delta_uploads should upload the same pending deltas to both chunk_pool and meshlet_pool"
    );
}

#[test]
fn review_meshlet_allocator_separates_tail_cursor_from_active_counts() {
    let combined = read_meshlet_pool_source();

    assert!(
        combined.contains("meshlet_tail")
            && combined.contains("vertex_tail")
            && combined.contains("tri_tail"),
        "MeshletPool should track append cursors separately from active counters"
    );
    assert!(
        !combined.contains(
            "(self.active_meshlet_count, self.active_vertex_count, self.active_tri_count)"
        ),
        "MeshletPool must not use active counts as append cursors after non-tail removals"
    );
}

#[test]
fn review_meshlet_upload_guards_against_capacity_overflow() {
    let combined = read_meshlet_pool_source();

    assert!(
        combined.contains("meshlet pool exhausted")
            || combined.contains("meshlet append exceeds capacity")
            || combined.contains("checked_add(meshlet_count)"),
        "MeshletPool uploads should check capacity before appending instead of overwriting live ranges"
    );
}

#[test]
fn review_meshlet_pool_has_growth_path_and_rebinds_bindless() {
    let combined = read_meshlet_pool_source();

    assert!(
        combined.contains("pub fn grow_capacity")
            && combined.contains("register_meshlet_buffers"),
        "MeshletPool should expose a grow_capacity path and re-register meshlet buffers in bindless after growth"
    );
}

#[test]
fn review_meshlet_growth_runs_before_frame_recording() {
    let submit = std::fs::read_to_string("src/renderer/submit.rs").expect("should read submit.rs");

    let prepare = submit
        .find("unsafe fn wait_fence_and_prepare")
        .expect("wait_fence_and_prepare must exist");
    let body = &submit[prepare..];
    assert!(
        body.contains("renderer")
            && body.contains(".meshlet_pool")
            && body.contains(".is_some_and(|mp| mp.needs_grow())")
            && body.contains("meshlet_pool.grow_capacity(renderer, &bindless)")
            && body.contains("renderer.meshlet_pool = Some(meshlet_pool)"),
        "meshlet growth should happen after fence wait and before command recording"
    );
}

#[test]
fn review_meshlet_buffers_are_created_with_transfer_src_for_growth() {
    let combined = read_meshlet_pool_source();

    let meshlet_impl = combined
        .find("impl MeshletPool")
        .expect("MeshletPool impl must exist");
    let meshlet_body = &combined[meshlet_impl..];
    assert!(
        meshlet_body.contains("TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::STORAGE_BUFFER")
            || meshlet_body.contains("TRANSFER_DST\n            | vk::BufferUsageFlags::TRANSFER_SRC\n            | vk::BufferUsageFlags::STORAGE_BUFFER"),
        "growable meshlet buffers should include TRANSFER_SRC usage from creation time"
    );
}

#[test]
fn review_hiz_is_initialized_and_destroyed_before_depth_on_resize() {
    let app = std::fs::read_to_string("src/app.rs").expect("should read app.rs");
    let swapchain =
        std::fs::read_to_string("src/renderer/swapchain.rs").expect("should read swapchain.rs");

    assert!(
        app.contains("HiZPyramid::new") || app.contains("renderer.hiz_pyramid = Some("),
        "run() should create the initial Hi-Z pyramid so Hi-Z culling and SSAO have a valid depth pyramid from frame 0"
    );

    let recreate_start = swapchain
        .find("pub fn recreate_swapchain_context")
        .expect("recreate_swapchain_context must exist");
    let body = &swapchain[recreate_start..];
    let hiz_destroy = body
        .find("old_hiz.destroy(renderer)")
        .expect("swapchain recreation must destroy the old Hi-Z pyramid");
    let depth_destroy = body
        .find("destroy_image(renderer.swapchain_ctx.depth_image")
        .expect("swapchain recreation must destroy the old resolved depth image");
    assert!(
        hiz_destroy < depth_destroy,
        "swapchain recreation must destroy Hi-Z views that reference the old depth image before destroying the depth image itself"
    );
}

#[test]
fn review_ssao_uses_dedicated_compute_descriptors_and_real_two_pass_blur() {
    let ssao = std::fs::read_to_string("src/renderer/ssao.rs").expect("should read ssao.rs");
    let compute = std::fs::read_to_string("shaders/ssao_compute.comp")
        .expect("should read ssao_compute.comp");
    let blur =
        std::fs::read_to_string("shaders/ssao_blur.comp").expect("should read ssao_blur.comp");

    assert!(
        ssao.contains("compute_descriptor_set_layout")
            && ssao.contains("blur_descriptor_set_layout")
            && ssao.contains("compute_descriptor_set")
            && ssao.contains("blur_h_descriptor_set")
            && ssao.contains("blur_v_descriptor_set"),
        "SSAO should own dedicated compute/blur descriptor sets instead of reusing the fragment bindless SSAO sampler binding for storage writes"
    );
    assert!(
        compute.contains("binding = 0") && compute.contains("binding = 1"),
        "SSAO compute shader should use a dedicated sampler+storage descriptor set layout"
    );
    assert!(
        blur.contains("binding = 0") && blur.contains("binding = 1"),
        "SSAO blur shader should use a dedicated sampler+storage descriptor set layout"
    );
    assert!(
        ssao.contains("direction: [0.0, 1.0]") && !ssao.contains("cmd_copy_image("),
        "SSAO blur should run a real vertical compute pass instead of copying the horizontal result back into AO"
    );
}

#[test]
fn review_fog_disable_path_uses_an_explicit_disabled_sentinel() {
    let lighting =
        std::fs::read_to_string("src/renderer/lighting.rs").expect("should read lighting.rs");
    let chunk_frag =
        std::fs::read_to_string("shaders/chunk_mesh.frag").expect("should read chunk_mesh.frag");
    let meshlet_frag = std::fs::read_to_string("shaders/meshlet_draw.frag")
        .expect("should read meshlet_draw.frag");

    assert!(
        lighting.contains("u32::MAX"),
        "disabled fog should use an explicit sentinel instead of aliasing Linear fog (0)"
    );
    assert!(
        !chunk_frag.contains("fog_density > 0.0 || lp.fog_type == 0u")
            && !meshlet_frag.contains("fog_density > 0.0 || lp.fog_type == 0u"),
        "fragment shaders should not treat fog_type == Linear as equivalent to fog enabled"
    );
}

#[test]
fn review_runtime_screen_and_shadow_params_drive_fragment_sampling() {
    let common = std::fs::read_to_string("shaders/common.glsl").expect("should read common.glsl");
    let chunk_frag =
        std::fs::read_to_string("shaders/chunk_mesh.frag").expect("should read chunk_mesh.frag");
    let meshlet_frag = std::fs::read_to_string("shaders/meshlet_draw.frag")
        .expect("should read meshlet_draw.frag");

    assert!(
        common.contains("render_params"),
        "LightingParams should expose runtime render parameters (screen size / shadow resolution) to fragment shaders"
    );
    assert!(
        chunk_frag.contains("lp.render_params.xy") && meshlet_frag.contains("lp.render_params.xy"),
        "fragment shaders should derive SSAO sampling UVs from the full-resolution screen size, not from the AO texture size"
    );
    assert!(
        chunk_frag.contains("lp.render_params.z") && meshlet_frag.contains("lp.render_params.z"),
        "fragment shaders should use the runtime shadow-map resolution instead of a hardcoded 2048.0"
    );
}

#[test]
fn review_transparent_blocks_affect_meshing_ao_and_raster_blending() {
    let greedy = std::fs::read_to_string("src/meshing/greedy.rs").expect("should read greedy.rs");
    let mesh_pipeline = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("should read mesh_pipeline.rs");
    let chunk_frag =
        std::fs::read_to_string("shaders/chunk_mesh.frag").expect("should read chunk_mesh.frag");
    let meshlet_frag = std::fs::read_to_string("shaders/meshlet_draw.frag")
        .expect("should read meshlet_draw.frag");

    assert!(
        greedy.contains("is_transparent_block")
            || greedy.contains("should_render_face_against")
            || greedy.contains("crate::renderer::material"),
        "greedy meshing should consult transparent-block semantics instead of treating every non-air block as a full opaque occluder"
    );
    assert!(
        mesh_pipeline.contains(".blend_enable(true)"),
        "graphics pipelines should enable alpha blending so transparent block textures are not forced through an opaque path"
    );
    assert!(
        chunk_frag.contains("discard;") && meshlet_frag.contains("discard;"),
        "fragment shaders should alpha-clip transparent texels so cutout materials like leaves don't render as solid quads"
    );
}

#[test]
fn review_emissive_point_lights_are_rebuilt_from_chunk_payloads() {
    let app = std::fs::read_to_string("src/app.rs").expect("should read app.rs");
    let point_light =
        std::fs::read_to_string("src/renderer/point_light.rs").expect("should read point_light.rs");

    assert!(
        point_light.contains("rebuild_from_payloads"),
        "PointLightManager should rebuild visible lights from active chunk payloads instead of uploading an always-empty staging vector"
    );
    assert!(
        app.contains("rebuild_from_payloads(&self.meshing.payloads")
            || app.contains("rebuild_from_payloads(\n                &self.meshing.payloads"),
        "the app should rebuild emissive point lights from current meshing payloads before submit"
    );

    let table = revoxelation::renderer::material::MaterialTable::default_table();
    assert!(
        table.entries().iter().any(|mat| {
            (mat.flags & revoxelation::renderer::material::FLAG_EMISSIVE) != 0
                && mat.emissive_intensity > 0
        }),
        "default materials should contain at least one emissive block so the emissive/point-light path is exercised in normal runtime content"
    );
}
