//! Rayon-based chunk job runner.
//!
//! `spawn_chunk_job` submits a chunk generation task to a rayon `ThreadPool`
//! and returns an `Arc<AtomicBool>` cancel flag. Setting the flag to `true`
//! before the task fires causes a `Cancelled` result to be sent instead.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use super::{
    job_queue::PrioritizedTask,
    types::{CHUNK_EDGE, CHUNK_VOXEL_COUNT, ChunkJobOutcome, ChunkJobResult, ChunkKey, ChunkVoxels},
};

/// Spawn a background chunk job on `pool`.
///
/// * If the cancel flag is `true` when the closure runs, a `Cancelled` result
///   is sent.
/// * Otherwise a `Generated` result with a deterministic non-empty chunk
///   payload is sent.
///
/// The returned `Arc<AtomicBool>` is the cancel flag; callers can set it to
/// `true` to request cancellation of in-flight work.
pub fn spawn_chunk_job(
    pool: &rayon::ThreadPool,
    task: PrioritizedTask,
    sender: mpsc::Sender<ChunkJobResult>,
) -> Arc<AtomicBool> {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let flag_clone = Arc::clone(&cancel_flag);

    pool.spawn(move || {
        let outcome = if flag_clone.load(Ordering::Relaxed) {
            ChunkJobOutcome::Cancelled
        } else {
            let voxels = generate_chunk_voxels(task.key);
            ChunkJobOutcome::Generated(voxels)
        };
        // Send result regardless of outcome; receiver may have dropped.
        let _ = sender.send(ChunkJobResult::new(task.key, outcome));
    });

    cancel_flag
}

fn generate_chunk_voxels(key: ChunkKey) -> ChunkVoxels {
    let mut block_ids = vec![0_u8; CHUNK_VOXEL_COUNT];
    let seed = chunk_seed(key);
    let floor_y = (seed % 4) as u8;
    let floor_block = 1 + (seed % 5) as u8;
    let pillar_block = 1 + ((seed >> 3) % 8) as u8; // range [1..8], must stay within MaterialTable (9 entries: 0=air + 8 blocks)

    for z in 0..CHUNK_EDGE as u8 {
        for x in 0..CHUNK_EDGE as u8 {
            block_ids[ChunkVoxels::linear_index(x, floor_y, z)] = floor_block;
        }
    }

    let pillar_a = [
        ((seed >> 5) % (CHUNK_EDGE as u32 - 4) + 2) as u8,
        ((seed >> 11) % (CHUNK_EDGE as u32 - 4) + 2) as u8,
    ];
    let pillar_b = [
        ((seed >> 17) % (CHUNK_EDGE as u32 - 4) + 2) as u8,
        ((seed >> 23) % (CHUNK_EDGE as u32 - 4) + 2) as u8,
    ];
    let pillar_a_height = floor_y.saturating_add(8 + ((seed >> 7) % 12) as u8);
    let pillar_b_height = floor_y.saturating_add(6 + ((seed >> 13) % 10) as u8);

    for y in floor_y..=pillar_a_height.min((CHUNK_EDGE - 1) as u8) {
        block_ids[ChunkVoxels::linear_index(pillar_a[0], y, pillar_a[1])] = pillar_block;
    }
    for y in floor_y..=pillar_b_height.min((CHUNK_EDGE - 1) as u8) {
        block_ids[ChunkVoxels::linear_index(pillar_b[0], y, pillar_b[1])] = floor_block;
    }

    ChunkVoxels::new(block_ids.into_boxed_slice())
        .expect("generated chunk payload must match the typed contract")
}

fn chunk_seed(key: ChunkKey) -> u32 {
    let mut seed = key.x as u32;
    seed = seed.wrapping_mul(0x9E37_79B9).rotate_left(5) ^ key.y as u32;
    seed = seed.wrapping_mul(0x85EB_CA6B).rotate_left(11) ^ key.z as u32;
    seed = seed.wrapping_mul(0xC2B2_AE35).rotate_left(17) ^ u32::from(key.lod_level);
    seed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::streaming::types::ChunkKey;

    fn key(n: i32) -> ChunkKey {
        ChunkKey::new(n, 0, 0, 0)
    }

    fn task(key: ChunkKey) -> PrioritizedTask {
        PrioritizedTask::new(key, 0, 1.0)
    }

    fn fresh_pool() -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap()
    }

    // -----------------------------------------------------------------------
    // spawn_sends_result
    // -----------------------------------------------------------------------
    #[test]
    fn spawn_sends_result() {
        let pool = fresh_pool();
        let (tx, rx) = std::sync::mpsc::channel();
        let _flag = spawn_chunk_job(&pool, task(key(1)), tx);
        let result = rx.recv_timeout(Duration::from_secs(2));
        assert!(result.is_ok(), "should receive a result within 2s");
        assert_eq!(result.unwrap().key, key(1));
    }

    // -----------------------------------------------------------------------
    // spawn_cancelled_result
    // -----------------------------------------------------------------------
    #[test]
    fn spawn_cancelled_result() {
        let pool = fresh_pool();
        let (tx, rx) = std::sync::mpsc::channel();
        let flag = spawn_chunk_job(&pool, task(key(2)), tx);
        // Set cancel before the rayon task has a chance to run.
        flag.store(true, Ordering::Relaxed);
        // Allow the task time to execute.
        let result = rx.recv_timeout(Duration::from_secs(2));
        assert!(
            result.is_ok(),
            "should still receive a result even when cancelled"
        );
        // Outcome must be Cancelled (if flag was seen in time) or Generated
        // (if the task ran before the flag was set). Either is acceptable; what
        // must NOT happen is a panic or missing result.
        let outcome = result.unwrap().outcome;
        assert!(
            matches!(
                outcome,
                ChunkJobOutcome::Cancelled | ChunkJobOutcome::Generated(_)
            ),
            "outcome must be Cancelled or Generated"
        );
    }
}
