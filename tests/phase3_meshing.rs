//! Phase 3 meshing integration tests.
//!
//! `run_frame` uses `OnceLock` global state, so runtime-oriented tests in this
//! file must reserve unique frame ranges. Phase 3 reserves `3000..3099`.

use std::mem::size_of;

use revoxelation::{
    meshing::{
        FACE_NEG_X, FACE_NEG_Y, FACE_NEG_Z, FACE_POS_X, FACE_POS_Y, FACE_POS_Z, MeshDirtyCause,
        MeshDirtyRecord, MeshingState, PackedMesh, PackedVertex, build_greedy_mesh,
        fine_chunk_boundary_mask, pack_vertex,
    },
    renderer::{chunk_pool::SlotAllocator, device::required_device_features_error},
    streaming::types::{CHUNK_EDGE, CHUNK_VOXEL_COUNT, ChunkJobOutcome, ChunkKey, ChunkState, ChunkVoxels},
};

fn filled_chunk(fill: u8) -> ChunkVoxels {
    ChunkVoxels::new(vec![fill; CHUNK_VOXEL_COUNT].into_boxed_slice()).expect("valid chunk payload")
}

fn chunk_with_blocks(blocks: &[(u8, u8, u8, u8)]) -> ChunkVoxels {
    let mut block_ids = vec![0; CHUNK_VOXEL_COUNT];
    for &(x, y, z, block_id) in blocks {
        let index = ChunkVoxels::linear_index(x, y, z);
        block_ids[index] = block_id;
    }
    ChunkVoxels::new(block_ids.into_boxed_slice()).expect("synthetic chunk fixture must be valid")
}

fn clean_dirty_record() -> MeshDirtyRecord {
    MeshDirtyRecord {
        causes: Vec::new(),
        source_revision: 1,
        finer_neighbor_face_mask: 0,
    }
}

fn skirt_vertex_count(vertices: &[PackedVertex]) -> usize {
    vertices
        .iter()
        .filter(|vertex| vertex.0[0] & (1 << 24) != 0)
        .count()
}

fn chunk_key(x: i32, y: i32, z: i32, lod_level: u8) -> ChunkKey {
    ChunkKey::new(x, y, z, lod_level)
}

fn chunk_state_name(state: ChunkState) -> &'static str {
    match state {
        ChunkState::Inactive => "Inactive",
        ChunkState::Queued => "Queued",
        ChunkState::Loading => "Loading",
        ChunkState::Active => "Active",
        ChunkState::Upgrading => "Upgrading",
        ChunkState::Downgrading => "Downgrading",
        ChunkState::Unloading => "Unloading",
        ChunkState::Error { .. } => "Error",
    }
}

fn sample_packed_mesh(index_count: usize, quad_count: u32) -> PackedMesh {
    let vertex_count = (index_count / 6) * 4;
    PackedMesh {
        vertices: vec![PackedVertex([0, 0]); vertex_count].into_boxed_slice(),
        indices: (0..index_count as u32).collect::<Vec<_>>().into_boxed_slice(),
        quad_count,
        aabb_min: [1.0, 2.0, 3.0],
        aabb_max: [4.0, 5.0, 6.0],
    }
}

#[test]
fn mesh_01_chunk_voxels_contract_and_packed_layout() {
    let error = ChunkVoxels::new(vec![0; CHUNK_VOXEL_COUNT - 1].into_boxed_slice())
        .expect_err("wrong payload length must be rejected");
    assert!(
        error.contains("expected"),
        "error should explain the required voxel count"
    );

    let mut block_ids = vec![0; CHUNK_VOXEL_COUNT];
    let index = ChunkVoxels::linear_index(1, 2, 3);
    block_ids[index] = 7;
    let voxels = ChunkVoxels::new(block_ids.into_boxed_slice()).expect("payload length is exact");
    assert_eq!(voxels.block(1, 2, 3), 7);
    assert_eq!(CHUNK_EDGE, 64);
    assert_eq!(size_of::<PackedVertex>(), 8);

    let packed = pack_vertex([1, 2, 3], 4, 513, [5, 6]);
    assert_eq!(packed.0[0], 1 | (2 << 6) | (3 << 12) | (4 << 18));
    assert_eq!(packed.0[1], 513 | (5 << 16) | (6 << 24));
}

#[test]
fn mesh_01_generated_payload_uses_typed_chunk_voxels() {
    let outcome = ChunkJobOutcome::Generated(filled_chunk(1));
    match outcome {
        ChunkJobOutcome::Generated(voxels) => assert_eq!(voxels.block_ids.len(), CHUNK_VOXEL_COUNT),
        other => panic!("expected generated payload, got {other:?}"),
    }
}

#[test]
fn mesh_02_border_invalidation_marks_neighbors() {
    let names = [
        chunk_state_name(ChunkState::Inactive),
        chunk_state_name(ChunkState::Queued),
        chunk_state_name(ChunkState::Loading),
        chunk_state_name(ChunkState::Active),
        chunk_state_name(ChunkState::Upgrading),
        chunk_state_name(ChunkState::Downgrading),
        chunk_state_name(ChunkState::Unloading),
        chunk_state_name(ChunkState::Error {
            retry_count: 0,
            next_retry_frame: 0,
        }),
    ];
    assert_eq!(
        names,
        [
            "Inactive",
            "Queued",
            "Loading",
            "Active",
            "Upgrading",
            "Downgrading",
            "Unloading",
            "Error",
        ]
    );

    let center = chunk_key(0, 0, 0, 0);
    let mut meshing = MeshingState::default();
    meshing.mark_face_neighbors_dirty(center, FACE_POS_X | FACE_NEG_Y | FACE_POS_Z, 11);

    let dirty_batch = meshing.take_dirty_batch(8);
    assert_eq!(
        dirty_batch,
        vec![
            chunk_key(1, 0, 0, 0),
            chunk_key(0, -1, 0, 0),
            chunk_key(0, 0, 1, 0),
        ]
    );

    let px = meshing
        .dirty
        .get(&chunk_key(1, 0, 0, 0))
        .expect("+X neighbor should be dirty");
    assert_eq!(
        px.causes,
        vec![MeshDirtyCause::BorderTouched {
            face_mask: FACE_NEG_X,
        }]
    );

    let ny = meshing
        .dirty
        .get(&chunk_key(0, -1, 0, 0))
        .expect("-Y neighbor should be dirty");
    assert_eq!(
        ny.causes,
        vec![MeshDirtyCause::BorderTouched {
            face_mask: FACE_POS_Y,
        }]
    );

    let pz = meshing
        .dirty
        .get(&chunk_key(0, 0, 1, 0))
        .expect("+Z neighbor should be dirty");
    assert_eq!(
        pz.causes,
        vec![MeshDirtyCause::BorderTouched {
            face_mask: FACE_NEG_Z,
        }]
    );
}

#[test]
fn mesh_02_finer_neighbor_face_mask_updates_coarse_chunk() {
    let fine_key = chunk_key(0, 0, 0, 0);
    assert_eq!(
        fine_chunk_boundary_mask(fine_key),
        FACE_NEG_X | FACE_NEG_Y | FACE_NEG_Z
    );

    let mut meshing = MeshingState::default();
    meshing.mark_coarse_lod_neighbors_dirty(
        fine_key,
        fine_chunk_boundary_mask(fine_key),
        true,
        21,
    );

    let coarse_x = chunk_key(-1, 0, 0, 1);
    let coarse_y = chunk_key(0, -1, 0, 1);
    let coarse_z = chunk_key(0, 0, -1, 1);

    assert_eq!(
        meshing
            .dirty
            .get(&coarse_x)
            .expect("coarse +X-facing chunk should be tracked")
            .finer_neighbor_face_mask,
        FACE_POS_X
    );
    assert_eq!(
        meshing
            .dirty
            .get(&coarse_y)
            .expect("coarse +Y-facing chunk should be tracked")
            .finer_neighbor_face_mask,
        FACE_POS_Y
    );
    assert_eq!(
        meshing
            .dirty
            .get(&coarse_z)
            .expect("coarse +Z-facing chunk should be tracked")
            .finer_neighbor_face_mask,
        FACE_POS_Z
    );

    meshing.update_finer_neighbor_face_mask(coarse_x, FACE_POS_X, false, 22);
    let coarse_x_record = meshing
        .dirty
        .get(&coarse_x)
        .expect("coarse record should remain tracked");
    assert_eq!(coarse_x_record.finer_neighbor_face_mask, 0);
    assert_eq!(
        coarse_x_record.causes,
        vec![
            MeshDirtyCause::FinerNeighborMaskChanged {
                face_mask: FACE_POS_X,
                active: true,
            },
            MeshDirtyCause::RevisionMismatch,
            MeshDirtyCause::FinerNeighborMaskChanged {
                face_mask: FACE_POS_X,
                active: false,
            },
        ]
    );
}

#[test]
fn mesh_01_greedy_meshing_emits_expected_quads() {
    let isolated = chunk_with_blocks(&[(1, 1, 1, 2)]);
    let isolated_mesh = build_greedy_mesh(
        &isolated,
        &revoxelation::meshing::ChunkNeighborSet::default(),
        &clean_dirty_record(),
    );
    assert_eq!(isolated_mesh.quad_count, 6);
    assert_eq!(isolated_mesh.indices.len(), 36);
    assert_eq!(isolated_mesh.aabb_min, [1.0, 1.0, 1.0]);
    assert_eq!(isolated_mesh.aabb_max, [2.0, 2.0, 2.0]);

    let mut slab_blocks = Vec::new();
    for x in 1..=2 {
        for y in 1..=2 {
            slab_blocks.push((x, y, 1, 3));
        }
    }
    let slab = chunk_with_blocks(&slab_blocks);
    let slab_mesh = build_greedy_mesh(
        &slab,
        &revoxelation::meshing::ChunkNeighborSet::default(),
        &clean_dirty_record(),
    );
    assert_eq!(slab_mesh.quad_count, 6, "2x2x1 slab should merge into 6 quads");
    assert!(
        slab_mesh.quad_count < 16,
        "greedy meshing should beat the naive visible-face count"
    );

    let seam_center = chunk_with_blocks(&[(63, 10, 10, 5)]);
    let seam_neighbor = chunk_with_blocks(&[(0, 10, 10, 5)]);
    let seam_mesh = build_greedy_mesh(
        &seam_center,
        &revoxelation::meshing::ChunkNeighborSet {
            px: Some(&seam_neighbor),
            ..Default::default()
        },
        &clean_dirty_record(),
    );
    assert_eq!(
        seam_mesh.quad_count, 5,
        "halo neighbor data should suppress the shared +X border face"
    );
}

#[test]
fn mesh_02_coarse_chunk_generates_skirts_only_for_flagged_faces() {
    let coarse_chunk = filled_chunk(1);
    let no_skirts = build_greedy_mesh(
        &coarse_chunk,
        &revoxelation::meshing::ChunkNeighborSet::default(),
        &clean_dirty_record(),
    );
    assert_eq!(no_skirts.quad_count, 6);
    assert_eq!(skirt_vertex_count(&no_skirts.vertices), 0);

    let dirty = MeshDirtyRecord {
        causes: vec![MeshDirtyCause::FinerNeighborMaskChanged {
            face_mask: FACE_NEG_X | FACE_POS_Z,
            active: true,
        }],
        source_revision: 2,
        finer_neighbor_face_mask: FACE_NEG_X | FACE_POS_Z,
    };
    let with_skirts = build_greedy_mesh(
        &coarse_chunk,
        &revoxelation::meshing::ChunkNeighborSet {
            finer_neighbor_face_mask: FACE_NEG_X | FACE_POS_Z,
            ..Default::default()
        },
        &dirty,
    );
    assert_eq!(with_skirts.quad_count, 8);
    assert_eq!(skirt_vertex_count(&with_skirts.vertices), 8);
}

#[test]
fn mesh_02_skirt_face_mask_clears_when_finer_neighbor_unloads() {
    let coarse_chunk = filled_chunk(1);
    let coarse_key = chunk_key(0, 0, 0, 1);
    let mut meshing = MeshingState::default();
    meshing.update_finer_neighbor_face_mask(coarse_key, FACE_NEG_X, true, 31);
    let dirty_with_skirt = meshing
        .dirty
        .get(&coarse_key)
        .cloned()
        .expect("coarse chunk should record the activated finer neighbor mask");

    let with_skirt = build_greedy_mesh(
        &coarse_chunk,
        &revoxelation::meshing::ChunkNeighborSet {
            finer_neighbor_face_mask: dirty_with_skirt.finer_neighbor_face_mask,
            ..Default::default()
        },
        &dirty_with_skirt,
    );
    assert_eq!(with_skirt.quad_count, 7);
    assert_eq!(skirt_vertex_count(&with_skirt.vertices), 4);

    meshing.update_finer_neighbor_face_mask(coarse_key, FACE_NEG_X, false, 32);
    let dirty_without_skirt = meshing
        .dirty
        .get(&coarse_key)
        .cloned()
        .expect("coarse chunk should keep the dirty record after the mask changes");
    let without_skirt = build_greedy_mesh(
        &coarse_chunk,
        &revoxelation::meshing::ChunkNeighborSet {
            finer_neighbor_face_mask: dirty_without_skirt.finer_neighbor_face_mask,
            ..Default::default()
        },
        &dirty_without_skirt,
    );
    assert_eq!(dirty_without_skirt.finer_neighbor_face_mask, 0);
    assert_eq!(without_skirt.quad_count, 6);
    assert_eq!(skirt_vertex_count(&without_skirt.vertices), 0);
}

#[test]
fn mesh_03_chunk_pool_slot_reuse_clears_metadata() {
    let mut allocator = SlotAllocator::with_capacity(2);
    let first_key = chunk_key(0, 0, 0, 0);
    let second_key = chunk_key(1, 0, 0, 0);

    let upload = allocator
        .prepare_upload(first_key, &sample_packed_mesh(6, 1))
        .expect("first chunk upload should allocate a slot");
    assert_eq!(upload.slot_id, 0);
    assert_eq!(allocator.active_chunk_count(), 1);
    assert_eq!(allocator.metadata_shadow()[0].index_count, 6);
    assert_eq!(allocator.indirect_shadow()[0].instance_count, 1);

    let removed = allocator
        .prepare_remove(first_key)
        .expect("removing an active chunk should free its slot");
    assert_eq!(removed, 0);
    assert_eq!(allocator.active_chunk_count(), 0);
    assert_eq!(allocator.metadata_shadow()[0].aabb_min, [0.0, 0.0, 0.0]);
    assert_eq!(allocator.metadata_shadow()[0].aabb_max, [0.0, 0.0, 0.0]);
    assert_eq!(allocator.metadata_shadow()[0].index_count, 0);
    assert_eq!(allocator.indirect_shadow()[0].instance_count, 0);

    let reused = allocator
        .prepare_upload(second_key, &sample_packed_mesh(12, 2))
        .expect("freed slot should be reusable");
    assert_eq!(reused.slot_id, 0);
    assert_eq!(allocator.slot_for(second_key), Some(0));
    assert_eq!(allocator.metadata_shadow()[0].index_count, 12);
}

#[test]
fn mesh_03_vulkan_feature_gate_is_fail_fast() {
    assert_eq!(
        required_device_features_error(),
        "Vulkan device missing required features: samplerAnisotropy, multiDrawIndirect, drawIndirectFirstInstance"
    );
}
