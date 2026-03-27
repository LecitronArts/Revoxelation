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
