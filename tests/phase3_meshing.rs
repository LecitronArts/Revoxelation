//! Phase 3 meshing integration tests.
//!
//! `run_frame` uses `OnceLock` global state, so runtime-oriented tests in this
//! file must reserve unique frame ranges. Phase 3 reserves `3000..3099`.

use std::mem::size_of;

use revoxelation::{
    meshing::{PackedVertex, pack_vertex},
    streaming::types::{CHUNK_EDGE, CHUNK_VOXEL_COUNT, ChunkJobOutcome, ChunkVoxels},
};

fn filled_chunk(fill: u8) -> ChunkVoxels {
    ChunkVoxels::new(vec![fill; CHUNK_VOXEL_COUNT].into_boxed_slice()).expect("valid chunk payload")
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
