//! Phase 6 meshlet pipeline tests (Plan 06-01 and 06-02).

// ============================================================================
// Task 1 — MeshletDescriptor and MeshletMesh type definitions
// ============================================================================

#[test]
fn phase6_meshlet_descriptor_fields() {
    use revoxelation::meshing::MeshletDescriptor;

    let desc = MeshletDescriptor {
        vertex_offset: 0,
        triangle_offset: 0,
        vertex_count: 4,
        triangle_count: 2,
        center: [0.0, 0.0, 0.0],
        radius: 1.0,
        cone_axis: [0.0, 1.0, 0.0],
        cone_cutoff: 0.5,
    };
    assert_eq!(desc.vertex_count, 4);
    assert_eq!(desc.triangle_count, 2);
    assert!(desc.radius > 0.0);
    assert!(desc.cone_cutoff >= -1.0 && desc.cone_cutoff <= 1.0);
}

#[test]
fn phase6_meshlet_mesh_fields() {
    use revoxelation::meshing::{MeshletDescriptor, MeshletMesh, PackedVertex};

    let mesh = MeshletMesh {
        meshlets: vec![MeshletDescriptor {
            vertex_offset: 0,
            triangle_offset: 0,
            vertex_count: 3,
            triangle_count: 1,
            center: [0.0; 3],
            radius: 1.0,
            cone_axis: [0.0, 0.0, 1.0],
            cone_cutoff: 0.0,
        }],
        vertices: vec![PackedVertex([0; 2])],
        triangles: vec![0, 1, 2],
        aabb_min: [-1.0, -1.0, -1.0],
        aabb_max: [1.0, 1.0, 1.0],
    };
    assert_eq!(mesh.meshlets.len(), 1);
    assert_eq!(mesh.vertices.len(), 1);
    assert_eq!(mesh.triangles.len(), 3);
    assert_eq!(mesh.aabb_min, [-1.0, -1.0, -1.0]);
    assert_eq!(mesh.aabb_max, [1.0, 1.0, 1.0]);
}

#[test]
fn phase6_meshopt_dependency() {
    let cargo_toml =
        std::fs::read_to_string("Cargo.toml").expect("Cargo.toml should exist");
    assert!(
        cargo_toml.contains("meshopt"),
        "Cargo.toml must contain meshopt dependency"
    );
}

// ============================================================================
// Task 2 — build_meshlets_from_packed
// ============================================================================

#[test]
fn phase6_build_meshlets_simple_quad() {
    use revoxelation::meshing::{PackedMesh, build_meshlets_from_packed, pack_vertex};

    // A single quad: 4 vertices, 6 indices (2 triangles).
    let v0 = pack_vertex([0, 0, 0], 0, 1, [0, 0]);
    let v1 = pack_vertex([1, 0, 0], 0, 1, [1, 0]);
    let v2 = pack_vertex([1, 1, 0], 0, 1, [1, 1]);
    let v3 = pack_vertex([0, 1, 0], 0, 1, [0, 1]);

    let packed = PackedMesh {
        vertices: vec![v0, v1, v2, v3].into_boxed_slice(),
        indices: vec![0, 1, 2, 0, 2, 3].into_boxed_slice(),
        quad_count: 1,
        aabb_min: [0.0, 0.0, 0.0],
        aabb_max: [1.0, 1.0, 0.0],
    };

    let meshlet_mesh = build_meshlets_from_packed(&packed);
    assert!(
        !meshlet_mesh.meshlets.is_empty(),
        "must produce at least 1 meshlet"
    );

    // Total vertices and triangles across all meshlets must match input.
    let total_verts: u32 = meshlet_mesh.meshlets.iter().map(|m| m.vertex_count).sum();
    let total_tris: u32 = meshlet_mesh.meshlets.iter().map(|m| m.triangle_count).sum();
    assert!(total_verts >= 3, "must have at least 3 vertices (one triangle worth)");
    assert_eq!(total_tris, 2, "input has 2 triangles");
}

#[test]
fn phase6_build_meshlets_bounds_valid() {
    use revoxelation::meshing::{PackedMesh, build_meshlets_from_packed, pack_vertex};

    // Create multiple quads to get multiple meshlets potentially.
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for i in 0..16u8 {
        let base = vertices.len() as u32;
        let x = i * 2;
        vertices.push(pack_vertex([x, 0, 0], 0, 1, [0, 0]));
        vertices.push(pack_vertex([x + 1, 0, 0], 0, 1, [1, 0]));
        vertices.push(pack_vertex([x + 1, 1, 0], 0, 1, [1, 1]));
        vertices.push(pack_vertex([x, 1, 0], 0, 1, [0, 1]));
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let packed = PackedMesh {
        vertices: vertices.into_boxed_slice(),
        indices: indices.into_boxed_slice(),
        quad_count: 16,
        aabb_min: [0.0, 0.0, 0.0],
        aabb_max: [32.0, 1.0, 0.0],
    };

    let meshlet_mesh = build_meshlets_from_packed(&packed);
    for (i, m) in meshlet_mesh.meshlets.iter().enumerate() {
        assert!(
            m.radius > 0.0,
            "meshlet {i} bounding sphere radius must be > 0, got {}",
            m.radius
        );
        assert!(
            m.cone_cutoff >= -1.0 && m.cone_cutoff <= 1.0,
            "meshlet {i} cone_cutoff must be in [-1, 1], got {}",
            m.cone_cutoff
        );
    }
}

#[test]
fn phase6_meshlet_vertex_limit() {
    use revoxelation::meshing::{PackedMesh, build_meshlets_from_packed, pack_vertex};

    // Create many quads so total vertices > 64.
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for i in 0..32u8 {
        let base = vertices.len() as u32;
        let x = i * 2;
        vertices.push(pack_vertex([x, 0, 0], 0, 1, [0, 0]));
        vertices.push(pack_vertex([x + 1, 0, 0], 0, 1, [1, 0]));
        vertices.push(pack_vertex([x + 1, 1, 0], 0, 1, [1, 1]));
        vertices.push(pack_vertex([x, 1, 0], 0, 1, [0, 1]));
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    assert!(vertices.len() > 64, "need >64 verts to test limit");

    let packed = PackedMesh {
        vertices: vertices.into_boxed_slice(),
        indices: indices.into_boxed_slice(),
        quad_count: 32,
        aabb_min: [0.0, 0.0, 0.0],
        aabb_max: [64.0, 1.0, 0.0],
    };

    let meshlet_mesh = build_meshlets_from_packed(&packed);
    for (i, m) in meshlet_mesh.meshlets.iter().enumerate() {
        assert!(
            m.vertex_count <= 64,
            "meshlet {i} vertex_count must be <= 64, got {}",
            m.vertex_count
        );
        assert!(
            m.triangle_count <= 124,
            "meshlet {i} triangle_count must be <= 124, got {}",
            m.triangle_count
        );
    }
}

#[test]
fn phase6_meshlet_preserves_aabb() {
    use revoxelation::meshing::{PackedMesh, build_meshlets_from_packed, pack_vertex};

    let v0 = pack_vertex([0, 0, 0], 0, 1, [0, 0]);
    let v1 = pack_vertex([10, 0, 0], 0, 1, [1, 0]);
    let v2 = pack_vertex([10, 10, 0], 0, 1, [1, 1]);
    let v3 = pack_vertex([0, 10, 0], 0, 1, [0, 1]);

    let packed = PackedMesh {
        vertices: vec![v0, v1, v2, v3].into_boxed_slice(),
        indices: vec![0, 1, 2, 0, 2, 3].into_boxed_slice(),
        quad_count: 1,
        aabb_min: [-5.0, -5.0, -5.0],
        aabb_max: [15.0, 15.0, 15.0],
    };

    let meshlet_mesh = build_meshlets_from_packed(&packed);
    assert_eq!(meshlet_mesh.aabb_min, packed.aabb_min);
    assert_eq!(meshlet_mesh.aabb_max, packed.aabb_max);
}

// ============================================================================
// Task 3 — GpuMeshlet and MeshletPool
// ============================================================================

#[test]
fn phase6_gpu_meshlet_size() {
    use revoxelation::renderer::chunk_pool::GpuMeshlet;
    assert_eq!(
        std::mem::size_of::<GpuMeshlet>(),
        64,
        "GpuMeshlet must be exactly 64 bytes"
    );
}

#[test]
fn phase6_gpu_meshlet_pod() {
    use revoxelation::renderer::chunk_pool::GpuMeshlet;
    // Compile-time check: if GpuMeshlet is Pod, we can call bytes_of on it.
    let m = GpuMeshlet::zeroed();
    let _bytes: &[u8] = bytemuck::bytes_of(&m);
}

#[test]
fn phase6_meshlet_pool_buffers() {
    let source = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("src/renderer/chunk_pool.rs should exist");

    for name in &[
        "meshlet_meta_buffer",
        "meshlet_vertex_buffer",
        "meshlet_tri_buffer",
        "visible_meshlet_buffer",
        "meshlet_indirect_buffer",
        "meshlet_count_buffer",
    ] {
        assert!(
            source.contains(name),
            "chunk_pool.rs must contain field: {name}"
        );
    }
}

#[test]
fn phase6_render_delta_meshlet_mesh() {
    let source = std::fs::read_to_string("src/renderer/mod.rs")
        .expect("src/renderer/mod.rs should exist");
    assert!(
        source.contains("MeshletMesh"),
        "RenderDelta::Upsert must reference MeshletMesh in renderer/mod.rs"
    );
}

// ============================================================================
// Plan 06-02 Task 1 — Subgroup feature validation
// ============================================================================

#[test]
fn phase6_subgroup_feature_check() {
    let source = std::fs::read_to_string("src/renderer/device.rs")
        .expect("src/renderer/device.rs should exist");
    assert!(
        source.contains("SubgroupProperties"),
        "device.rs must query VkPhysicalDeviceSubgroupProperties"
    );
    assert!(
        source.contains("BALLOT"),
        "device.rs must check for SUBGROUP_FEATURE_BALLOT_BIT"
    );
}

// ============================================================================
// Plan 06-03 Task 3 — MeshletPipeline trait + ComputeIndirectPath
// ============================================================================

#[test]
fn phase6_meshlet_pipeline_trait_exists() {
    let source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("src/renderer/mesh_pipeline.rs should exist");
    assert!(
        source.contains("MeshletPipeline"),
        "mesh_pipeline.rs must contain MeshletPipeline trait"
    );
    assert!(
        source.contains("record_draw"),
        "mesh_pipeline.rs must contain record_draw method"
    );
    assert!(
        source.contains("ComputeIndirectPath"),
        "mesh_pipeline.rs must contain ComputeIndirectPath struct"
    );
}
