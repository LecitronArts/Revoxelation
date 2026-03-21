//! Phase 3 meshing integration tests.
//!
//! `run_frame` uses `OnceLock` global state, so runtime-oriented tests in this
//! file must reserve unique frame ranges. Phase 3 reserves `3000..3099`.

use std::mem::size_of;

use revoxelation::{
    meshing::{
        FACE_NEG_X, FACE_NEG_Y, FACE_NEG_Z, FACE_POS_X, FACE_POS_Y, FACE_POS_Z, MeshDirtyCause,
        MeshingState, PackedVertex, fine_chunk_boundary_mask, pack_vertex,
    },
    streaming::types::{CHUNK_EDGE, CHUNK_VOXEL_COUNT, ChunkJobOutcome, ChunkKey, ChunkState, ChunkVoxels},
};

fn filled_chunk(fill: u8) -> ChunkVoxels {
    ChunkVoxels::new(vec![fill; CHUNK_VOXEL_COUNT].into_boxed_slice()).expect("valid chunk payload")
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
