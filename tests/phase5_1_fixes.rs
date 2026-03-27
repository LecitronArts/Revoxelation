//! Phase 5.1 — Critical bug fixes and safety hardening tests.
//!
//! Source-scanning tests that verify structural wiring between modules.

/// Verify that `recreate_swapchain_context` in swapchain.rs references
/// `hiz_pyramid` or `HiZPyramid`, proving the Hi-Z recreation path is wired in.
#[test]
fn hiz_resize_wired_to_swapchain() {
    let source = std::fs::read_to_string("src/renderer/swapchain.rs")
        .expect("failed to read src/renderer/swapchain.rs");

    // Find the recreate_swapchain_context function body.
    let fn_start = source
        .find("fn recreate_swapchain_context")
        .expect("recreate_swapchain_context function not found in swapchain.rs");
    let body = &source[fn_start..];

    assert!(
        body.contains("hiz_pyramid") || body.contains("HiZPyramid"),
        "recreate_swapchain_context does not reference hiz_pyramid / HiZPyramid — \
         Hi-Z pyramid is NOT recreated on swapchain resize (BUG FIX-01)"
    );
}

/// Verify that `HiZPyramid::recreate` method exists in hiz.rs.
#[test]
fn hiz_pyramid_recreate_exists() {
    let source = std::fs::read_to_string("src/renderer/hiz.rs")
        .expect("failed to read src/renderer/hiz.rs");

    assert!(
        source.contains("fn recreate"),
        "HiZPyramid::recreate method not found in hiz.rs"
    );
}

/// FIX-03: run_world_update must NOT contain a hardcoded camera position.
/// The camera position must be a parameter, not `[0.0f32, 0.0, 0.0]`.
#[test]
fn camera_pos_not_hardcoded() {
    let source = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("failed to read src/runtime/scheduler.rs");

    // Find run_world_update function body.
    let fn_start = source
        .find("fn run_world_update")
        .expect("run_world_update function not found in scheduler.rs");
    let body = &source[fn_start..];
    // The body extends until the next `fn ` at the start of a line (or EOF).
    let fn_end = body[1..]
        .find("\nfn ")
        .map(|i| i + 1)
        .unwrap_or(body.len());
    let fn_body = &body[..fn_end];

    assert!(
        !fn_body.contains("camera_pos = [0.0f32, 0.0, 0.0]"),
        "run_world_update still contains hardcoded camera_pos = [0.0f32, 0.0, 0.0] — \
         camera position must come from a parameter (FIX-03)"
    );

    // Also verify that run_world_update accepts camera_pos as a parameter.
    let sig_end = fn_body.find('{').unwrap_or(fn_body.len());
    let signature = &fn_body[..sig_end];
    assert!(
        signature.contains("camera_pos"),
        "run_world_update signature must include a camera_pos parameter (FIX-03)"
    );
}

/// FIX-04: dense_indirect_shadow access must be bounds-checked.
#[test]
fn dense_indirect_bounds_check() {
    let source = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("failed to read src/renderer/chunk_pool.rs");

    // Find record_dense_indirect_copy function body.
    let fn_start = source
        .find("fn record_dense_indirect_copy")
        .expect("record_dense_indirect_copy function not found in chunk_pool.rs");
    let body = &source[fn_start..];
    let fn_end = body[1..]
        .find("\n    fn ")
        .or_else(|| body[1..].find("\n    pub fn "))
        .map(|i| i + 1)
        .unwrap_or(body.len());
    let fn_body = &body[..fn_end];

    let has_bounds_check = fn_body.contains("debug_assert!")
        || fn_body.contains(".get_mut(")
        || fn_body.contains(".get(");
    assert!(
        has_bounds_check,
        "record_dense_indirect_copy must contain a bounds check \
         (debug_assert! or .get_mut() or .get()) on draw_index (FIX-04)"
    );
}

/// FIX-03: run_frame must accept camera_pos parameter so app.rs can pass it.
#[test]
fn run_frame_accepts_camera_pos() {
    let source = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("failed to read src/runtime/scheduler.rs");

    let fn_start = source
        .find("pub fn run_frame")
        .expect("run_frame function not found in scheduler.rs");
    let sig_end = source[fn_start..].find('{').unwrap_or(source.len() - fn_start);
    let signature = &source[fn_start..fn_start + sig_end];

    assert!(
        signature.contains("camera_pos"),
        "run_frame signature must include a camera_pos parameter (FIX-03)"
    );
}

/// FIX-03: app.rs must extract camera position and pass to run_frame.
#[test]
fn app_passes_camera_pos_to_run_frame() {
    let source = std::fs::read_to_string("src/app.rs")
        .expect("failed to read src/app.rs");

    // The call site must reference camera position.
    assert!(
        source.contains("camera.position") || source.contains("camera_pos"),
        "app.rs must extract camera position from FpsCamera and pass to run_frame (FIX-03)"
    );

    // Must appear near run_frame call.
    let run_frame_idx = source.find("run_frame").expect("run_frame call not found in app.rs");
    let context_start = run_frame_idx.saturating_sub(200);
    let context_end = (run_frame_idx + 300).min(source.len());
    let context = &source[context_start..context_end];
    assert!(
        context.contains("camera") && context.contains("position"),
        "camera position must be extracted near the run_frame call site in app.rs (FIX-03)"
    );
}

/// FIX-05: All `unsafe impl Send` blocks must have `// SAFETY:` documentation
/// explaining the invariant and where it is enforced.
#[test]
fn send_safety_documented() {
    let files_with_unsafe_send = [
        "src/renderer/staging_ring.rs",
        "src/renderer/cull_pipeline.rs",
    ];

    for file_path in &files_with_unsafe_send {
        let source = std::fs::read_to_string(file_path)
            .unwrap_or_else(|_| panic!("failed to read {file_path}"));
        let lines: Vec<&str> = source.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            if line.contains("unsafe impl Send") {
                // Check that a `// SAFETY:` comment exists within 5 lines above.
                let start = i.saturating_sub(5);
                let preceding = &lines[start..i];
                let has_safety_comment = preceding
                    .iter()
                    .any(|l| l.contains("// SAFETY:"));
                assert!(
                    has_safety_comment,
                    "{file_path}:{}: `unsafe impl Send` at line {} lacks a `// SAFETY:` comment \
                     within 5 lines above it (FIX-05)",
                    i + 1,
                    i + 1,
                );
            }
        }
    }
}

/// FIX-06: `draw_cmd_as_bytes` must be replaced with a safe bytemuck cast.
/// No `from_raw_parts` manual pointer reinterpretation should remain for draw commands.
#[test]
fn draw_cmd_no_raw_parts() {
    let source = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("failed to read src/renderer/chunk_pool.rs");

    assert!(
        !source.contains("from_raw_parts"),
        "chunk_pool.rs still contains `from_raw_parts` — \
         draw_cmd_as_bytes must be replaced with a safe bytemuck cast (FIX-06)"
    );
}

/// FIX-09: Drop implementations must log cleanup failures instead of
/// silently discarding them with `let _ = ...`.
#[test]
fn drop_impl_logs_errors() {
    let source = std::fs::read_to_string("src/renderer/mod.rs")
        .expect("failed to read src/renderer/mod.rs");

    // Find the `fn drop` block.
    let drop_start = source
        .find("fn drop(&mut self)")
        .expect("fn drop not found in mod.rs");
    let drop_body = &source[drop_start..];

    // The drop body should NOT contain `let _ =` for fallible calls.
    // Count occurrences of `let _ =` in the drop function.
    let let_underscore_count = drop_body
        .lines()
        .take_while(|line| {
            // Rough heuristic: stop at end of impl block (closing brace at column 0)
            // We'll scan until we see enough context.
            true
        })
        .filter(|line| line.contains("let _ ="))
        .count();

    assert_eq!(
        let_underscore_count, 0,
        "Drop impl in mod.rs contains {} occurrences of `let _ =` — \
         all cleanup failures must be logged with log::warn (FIX-09)",
        let_underscore_count,
    );
}

/// FIX-02: egui scratch buffers must use a per-frame ring (array of Vecs)
/// instead of a single Vec, so buffers from frame N are not freed until
/// frame N+2's fence has been waited on.
#[test]
fn egui_scratch_per_frame_ring() {
    let source = std::fs::read_to_string("src/renderer/egui_backend.rs")
        .expect("failed to read egui_backend.rs");

    // The struct must declare scratch_buffers as an array of Vecs (2 = FRAMES_IN_FLIGHT),
    // NOT a single Vec. Pattern: `[Vec<(vk::Buffer, Allocation)>; 2]` or equivalent.
    let has_array_of_vecs = source.contains("[Vec<(vk::Buffer, Allocation)>; 2]");
    assert!(
        has_array_of_vecs,
        "scratch_buffers must be [Vec<(vk::Buffer, Allocation)>; 2], \
         not a single Vec — required for double-buffered GPU safety"
    );

    // paint() must accept current_frame parameter to select the correct ring slot.
    let has_current_frame_param = source.contains("current_frame: usize");
    assert!(
        has_current_frame_param,
        "paint() must accept current_frame: usize to select the correct scratch ring slot"
    );
}
