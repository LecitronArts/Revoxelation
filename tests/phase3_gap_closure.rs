//! Phase 3 gap-closure tests.
//!
//! `run_frame` uses `OnceLock` global state, so runtime-oriented tests in this
//! file must reserve unique frame ranges. Phase 3 gap closure reserves
//! `3100..3199`.

use std::{sync::mpsc, time::Duration};

use revoxelation::{
    meshing::{PackedMesh, PackedVertex},
    renderer::chunk_pool::SlotAllocator,
    streaming::{
        job_queue::PrioritizedTask,
        job_runner::spawn_chunk_job,
        types::{CHUNK_VOXEL_COUNT, ChunkJobOutcome, ChunkKey},
    },
};

fn chunk_key(x: i32, y: i32, z: i32, lod_level: u8) -> ChunkKey {
    ChunkKey::new(x, y, z, lod_level)
}

fn prioritized_task(key: ChunkKey) -> PrioritizedTask {
    PrioritizedTask::new(key, 0, 1.0)
}

fn fresh_pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("rayon test pool should build")
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

fn recv_generated_payload(
    rx: &mpsc::Receiver<revoxelation::streaming::types::ChunkJobResult>,
) -> Box<[u8]> {
    let result = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("chunk job should finish within 2s");
    match result.outcome {
        ChunkJobOutcome::Generated(voxels) => voxels.block_ids,
        other => panic!("expected generated chunk payload, got {other:?}"),
    }
}

#[test]
fn mesh_03_job_runner_emits_deterministic_non_empty_chunk_payload() {
    let pool = fresh_pool();
    let key = chunk_key(3, -2, 5, 1);
    let (tx, rx) = mpsc::channel();

    let _first = spawn_chunk_job(&pool, prioritized_task(key), tx.clone());
    let _second = spawn_chunk_job(&pool, prioritized_task(key), tx);

    let first = recv_generated_payload(&rx);
    let second = recv_generated_payload(&rx);

    assert_eq!(first.len(), CHUNK_VOXEL_COUNT);
    assert_eq!(second.len(), CHUNK_VOXEL_COUNT);
    assert_eq!(first, second, "same chunk key should produce the same payload");
    assert!(
        first.iter().any(|block_id| *block_id != 0),
        "generated payload should contain at least one non-air voxel"
    );
}

#[test]
fn mesh_03_dense_draw_list_swap_removes_sparse_slot_holes() {
    let mut allocator = SlotAllocator::with_capacity(4);
    let first = chunk_key(0, 0, 0, 0);
    let second = chunk_key(1, 0, 0, 0);
    let third = chunk_key(2, 0, 0, 0);

    allocator
        .prepare_upload(first, &sample_packed_mesh(6, 1))
        .expect("first upload should succeed");
    allocator
        .prepare_upload(second, &sample_packed_mesh(12, 2))
        .expect("second upload should succeed");
    allocator
        .prepare_upload(third, &sample_packed_mesh(18, 3))
        .expect("third upload should succeed");

    assert_eq!(allocator.active_draw_count(), 3);
    assert_eq!(allocator.draw_slots_shadow(), &[0, 1, 2]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(1), Some(1));
    assert_eq!(allocator.draw_index_for_slot(2), Some(2));

    let third_slot_before_remove = allocator
        .slot_for(third)
        .expect("third chunk should have a stable slot");
    let third_metadata_before_remove = allocator.metadata_shadow()[third_slot_before_remove as usize];

    let removed_slot = allocator
        .prepare_remove(second)
        .expect("removing a live chunk should free its stable slot");
    assert_eq!(removed_slot, 1);

    assert_eq!(allocator.active_chunk_count(), 2);
    assert_eq!(allocator.active_draw_count(), 2);
    assert_eq!(allocator.draw_slots_shadow(), &[0, 2]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(1), None);
    assert_eq!(allocator.draw_index_for_slot(2), Some(1));
    assert_eq!(
        allocator.metadata_shadow()[third_slot_before_remove as usize],
        third_metadata_before_remove,
        "stable-slot storage should remain keyed by slot id even when draw order compacts"
    );
}

#[test]
fn mesh_03_dense_draw_list_reuse_keeps_other_draw_indices_stable() {
    let mut allocator = SlotAllocator::with_capacity(4);
    let first = chunk_key(0, 0, 0, 0);
    let second = chunk_key(1, 0, 0, 0);
    let third = chunk_key(2, 0, 0, 0);
    let reused = chunk_key(9, 0, 0, 0);

    allocator
        .prepare_upload(first, &sample_packed_mesh(6, 1))
        .expect("first upload should succeed");
    allocator
        .prepare_upload(second, &sample_packed_mesh(12, 2))
        .expect("second upload should succeed");
    allocator
        .prepare_upload(third, &sample_packed_mesh(18, 3))
        .expect("third upload should succeed");

    allocator
        .prepare_remove(second)
        .expect("removing a middle slot should succeed");
    assert_eq!(allocator.draw_slots_shadow(), &[0, 2]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(2), Some(1));

    let reused_upload = allocator
        .prepare_upload(reused, &sample_packed_mesh(24, 4))
        .expect("reused upload should succeed");

    assert_eq!(reused_upload.slot_id, 1, "the freed stable slot should be reused");
    assert_eq!(allocator.active_chunk_count(), 3);
    assert_eq!(allocator.active_draw_count(), 3);
    assert_eq!(allocator.draw_slots_shadow(), &[0, 2, 1]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(2), Some(1));
    assert_eq!(allocator.draw_index_for_slot(1), Some(2));
}
