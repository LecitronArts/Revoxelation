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
