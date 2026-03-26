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

// ---------------------------------------------------------------------------
// Plan 05-02 Task 1 — BindlessTable creation
// ---------------------------------------------------------------------------

/// BindlessTable module must be declared in renderer/mod.rs.
#[test]
fn phase5_bindless_table_module_exists() {
    let source = std::fs::read_to_string("src/renderer/mod.rs")
        .expect("src/renderer/mod.rs should exist");
    assert!(
        source.contains("pub mod bindless"),
        "src/renderer/mod.rs must contain 'pub mod bindless'"
    );
}

/// BindlessTable must declare all 10 bindings (0-9) with UPDATE_AFTER_BIND and PARTIALLY_BOUND.
#[test]
fn phase5_bindless_table_has_required_bindings() {
    let source = std::fs::read_to_string("src/renderer/bindless.rs")
        .expect("src/renderer/bindless.rs should exist");

    // Must reference UPDATE_AFTER_BIND and PARTIALLY_BOUND flags.
    assert!(
        source.contains("UPDATE_AFTER_BIND"),
        "bindless.rs must contain UPDATE_AFTER_BIND flag"
    );
    assert!(
        source.contains("PARTIALLY_BOUND"),
        "bindless.rs must contain PARTIALLY_BOUND flag"
    );

    // Must have 10 bindings (0 through 9).
    for i in 0..=9u32 {
        let binding_str = format!(".binding({i})");
        assert!(
            source.contains(&binding_str),
            "bindless.rs must contain binding {i} (looking for '{binding_str}')"
        );
    }
}

/// BindlessTable must define register_buffer and register_image methods.
#[test]
fn phase5_bindless_table_register_methods() {
    let source = std::fs::read_to_string("src/renderer/bindless.rs")
        .expect("src/renderer/bindless.rs should exist");
    assert!(
        source.contains("fn register_buffer"),
        "bindless.rs must define a register_buffer method"
    );
    assert!(
        source.contains("fn register_image"),
        "bindless.rs must define a register_image method"
    );
}

// ---------------------------------------------------------------------------
// Plan 05-02 Task 2 — Migrate pipelines to shared set 0
// ---------------------------------------------------------------------------

/// Cull pipeline must NOT have its own descriptor pool or set layout creation.
#[test]
fn phase5_cull_pipeline_no_own_descriptor_set() {
    let source = std::fs::read_to_string("src/renderer/cull_pipeline.rs")
        .expect("src/renderer/cull_pipeline.rs should exist");
    assert!(
        !source.contains("create_descriptor_pool"),
        "cull_pipeline.rs must NOT contain create_descriptor_pool (moved to bindless.rs)"
    );
    assert!(
        !source.contains("create_descriptor_set_layout"),
        "cull_pipeline.rs must NOT contain create_descriptor_set_layout (moved to bindless.rs)"
    );
}

/// Mesh pipeline must NOT have its own descriptor pool or set layout creation.
#[test]
fn phase5_mesh_pipeline_no_own_descriptor_set() {
    let source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("src/renderer/mesh_pipeline.rs should exist");
    assert!(
        !source.contains("create_descriptor_pool"),
        "mesh_pipeline.rs must NOT contain create_descriptor_pool (moved to bindless.rs)"
    );
    assert!(
        !source.contains("create_descriptor_set_layout"),
        "mesh_pipeline.rs must NOT contain create_descriptor_set_layout (moved to bindless.rs)"
    );
}

// ---------------------------------------------------------------------------
// Plan 05-04 Task 1 — BlockMaterial and MaterialTable
// ---------------------------------------------------------------------------

/// BlockMaterial must be exactly 8 bytes (4 × u16).
#[test]
fn phase5_block_material_size() {
    assert_eq!(
        std::mem::size_of::<revoxelation::renderer::material::BlockMaterial>(),
        8,
        "BlockMaterial must be 8 bytes (top_texture, side_texture, bottom_texture, flags — all u16)"
    );
}

/// Default MaterialTable must have at least 9 entries (air + 8 block types).
#[test]
fn phase5_block_material_ssbo_8_types() {
    let table = revoxelation::renderer::material::MaterialTable::default_table();
    let entries = table.entries();
    assert!(
        entries.len() >= 9,
        "MaterialTable must have at least 9 entries (0=air + 8 blocks), got {}",
        entries.len()
    );
    // Air (block_id=0) should have zero texture indices.
    assert_eq!(entries[0].top_texture, 0);
    assert_eq!(entries[0].side_texture, 0);
    assert_eq!(entries[0].bottom_texture, 0);
    // Non-air blocks should have non-zero texture indices.
    for i in 1..=8 {
        let m = &entries[i];
        assert!(
            m.top_texture != 0 || m.side_texture != 0 || m.bottom_texture != 0,
            "Block {i} must have at least one non-zero texture index"
        );
    }
}

/// Grass (block_id=2) must have distinct top, side, bottom textures.
#[test]
fn phase5_grass_per_face_textures() {
    let table = revoxelation::renderer::material::MaterialTable::default_table();
    let grass = &table.entries()[2];
    assert_ne!(
        grass.top_texture, grass.side_texture,
        "Grass top_texture must differ from side_texture"
    );
    assert_ne!(
        grass.top_texture, grass.bottom_texture,
        "Grass top_texture must differ from bottom_texture"
    );
    assert_ne!(
        grass.side_texture, grass.bottom_texture,
        "Grass side_texture must differ from bottom_texture"
    );
}

/// Both pipelines must reference a bindless layout parameter.
#[test]
fn phase5_shared_set0_pipelines() {
    let cull_source = std::fs::read_to_string("src/renderer/cull_pipeline.rs")
        .expect("src/renderer/cull_pipeline.rs should exist");
    let mesh_source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("src/renderer/mesh_pipeline.rs should exist");
    assert!(
        cull_source.contains("bindless_layout") || cull_source.contains("bindless"),
        "cull_pipeline.rs must reference bindless layout"
    );
    assert!(
        mesh_source.contains("bindless_layout") || mesh_source.contains("bindless"),
        "mesh_pipeline.rs must reference bindless layout"
    );
}
