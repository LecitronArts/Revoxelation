//! Phase 5 bindless architecture tests.
//!
//! Source-grep tests that verify Vulkan 1.2 feature enforcement
//! in device selection (Plan 05-01, BIND-01).

// ---------------------------------------------------------------------------
// Plan 05-01 Task 1 — Vulkan 1.2 feature probe and hard-require enforcement
// ---------------------------------------------------------------------------

/// All 7 required Vulkan 1.2 feature names must appear in device.rs.
#[test]
fn phase5_vulkan12_required_features_listed() {
    let source = std::fs::read_to_string("src/renderer/device.rs")
        .expect("src/renderer/device.rs should exist");

    let required_features = [
        "descriptor_indexing",
        "shader_sampled_image_array_non_uniform_indexing",
        "runtime_descriptor_array",
        "descriptor_binding_partially_bound",
        "descriptor_binding_sampled_image_update_after_bind",
        "descriptor_binding_storage_buffer_update_after_bind",
        "draw_indirect_count",
    ];

    for feature in &required_features {
        assert!(
            source.contains(feature),
            "src/renderer/device.rs must reference Vulkan 1.2 feature: {feature}"
        );
    }
}

/// Device creation must use PhysicalDeviceVulkan12Features via pNext chain.
#[test]
fn phase5_vulkan12_pnext_chain_used() {
    let source = std::fs::read_to_string("src/renderer/device.rs")
        .expect("src/renderer/device.rs should exist");

    assert!(
        source.contains("PhysicalDeviceVulkan12Features"),
        "device.rs must use PhysicalDeviceVulkan12Features struct"
    );
    assert!(
        source.contains("push_next"),
        "device.rs must use push_next for pNext chain"
    );
}

/// Error path must include descriptive text about missing features.
#[test]
fn phase5_graceful_error_missing_features() {
    let source = std::fs::read_to_string("src/renderer/device.rs")
        .expect("src/renderer/device.rs should exist");

    // Must have a function or logic that collects missing feature names
    assert!(
        source.contains("missing") && source.contains("Vulkan 1.2"),
        "device.rs must contain logic for reporting missing Vulkan 1.2 features"
    );
}

/// No fallback path for Vulkan 1.2 features.
#[test]
fn phase5_no_fallback_path() {
    let source = std::fs::read_to_string("src/renderer/device.rs")
        .expect("src/renderer/device.rs should exist");

    // Count occurrences of "fallback"
    let fallback_occurrences: Vec<&str> = source
        .lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("fallback") && !lower.contains("no fallback")
        })
        .collect();

    assert!(
        fallback_occurrences.is_empty(),
        "device.rs must NOT contain a fallback path (found {} lines with 'fallback')",
        fallback_occurrences.len()
    );
}
