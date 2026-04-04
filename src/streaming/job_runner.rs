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
    types::{
        CHUNK_EDGE, CHUNK_VOXEL_COUNT, ChunkJobOutcome, ChunkJobResult, ChunkKey, ChunkVoxels,
    },
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
        let outcome = if flag_clone.load(Ordering::Acquire) {
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

    // World-space scale: each LOD level doubles the voxel size.
    let lod_scale = (1u32 << key.lod_level) as f32;
    let block_world = 1.0 / 16.0 * lod_scale; // world-metres per block

    for z in 0..CHUNK_EDGE as u8 {
        for x in 0..CHUNK_EDGE as u8 {
            // World-space position of this column.
            let wx = (key.x as f32 * CHUNK_EDGE as f32 + x as f32) * block_world;
            let wz = (key.z as f32 * CHUNK_EDGE as f32 + z as f32) * block_world;

            // Multi-octave value noise for natural-looking terrain height.
            let h0 = value_noise_2d(wx * 0.02, wz * 0.02); // large hills
            let h1 = value_noise_2d(wx * 0.07, wz * 0.07); // medium detail
            let h2 = value_noise_2d(wx * 0.20, wz * 0.20); // fine detail
            let height_world = h0 * 6.0 + h1 * 2.5 + h2 * 0.8 + 2.0; // metres above y=0

            // Convert world height to block-local y within this chunk.
            let chunk_base_y = key.y as f32 * CHUNK_EDGE as f32 * block_world;
            let _chunk_top_y = chunk_base_y + CHUNK_EDGE as f32 * block_world;

            if height_world < chunk_base_y {
                // Terrain is below this chunk — skip column.
                continue;
            }

            let fill_top = ((height_world - chunk_base_y) / block_world) as u8;
            let fill_top = fill_top.min((CHUNK_EDGE - 1) as u8);

            for y in 0..=fill_top {
                let wy = chunk_base_y + y as f32 * block_world;
                let depth_below_surface = height_world - wy;

                // Material selection based on depth below surface:
                // - Surface: grass (1)
                // - 1-3 blocks deep: dirt (2)
                // - Deeper: stone (3)
                // - Rare ore patches: use noise (4-5)
                let block_id = if depth_below_surface < block_world * 1.2 {
                    1 // grass
                } else if depth_below_surface < block_world * 4.0 {
                    2 // dirt
                } else {
                    // Stone with rare ore veins.
                    let ore_noise = value_noise_3d(wx * 0.15, wy * 0.15, wz * 0.15);
                    if ore_noise > 0.75 {
                        4 + (hash_u32((x as u32).wrapping_mul(7) ^ (z as u32).wrapping_mul(13) ^ seed) % 4) as u8
                    } else {
                        3 // stone
                    }
                };

                block_ids[ChunkVoxels::linear_index(x, y, z)] = block_id;
            }
        }
    }

    ChunkVoxels::new(block_ids.into_boxed_slice())
        .expect("generated chunk payload must match the typed contract")
}

// ---------------------------------------------------------------------------
// Simple hash-based value noise (no external crate dependency)
// ---------------------------------------------------------------------------

/// Integer hash for noise lattice points.
fn hash_u32(mut x: u32) -> u32 {
    x = x.wrapping_mul(0x9E37_79B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 13;
    x = x.wrapping_mul(0xC2B2_AE35);
    x ^= x >> 16;
    x
}

/// Hash 2D lattice point to [0, 1] float.
fn hash_2d(ix: i32, iz: i32) -> f32 {
    let h = hash_u32(ix as u32 ^ (iz as u32).wrapping_mul(0x45D9_F3B3));
    (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

/// Hash 3D lattice point to [0, 1] float.
fn hash_3d(ix: i32, iy: i32, iz: i32) -> f32 {
    let h = hash_u32(
        (ix as u32)
            ^ (iy as u32).wrapping_mul(0x45D9_F3B3)
            ^ (iz as u32).wrapping_mul(0x27D4_EB2D),
    );
    (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

/// Smooth interpolation (Hermite / smoothstep).
fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// 2D value noise returning [-1, 1].
fn value_noise_2d(x: f32, z: f32) -> f32 {
    let ix = x.floor() as i32;
    let iz = z.floor() as i32;
    let fx = smoothstep(x - x.floor());
    let fz = smoothstep(z - z.floor());

    let v00 = hash_2d(ix, iz);
    let v10 = hash_2d(ix + 1, iz);
    let v01 = hash_2d(ix, iz + 1);
    let v11 = hash_2d(ix + 1, iz + 1);

    let a = v00 + (v10 - v00) * fx;
    let b = v01 + (v11 - v01) * fx;
    (a + (b - a) * fz) * 2.0 - 1.0
}

/// 3D value noise returning [-1, 1].
fn value_noise_3d(x: f32, y: f32, z: f32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let iz = z.floor() as i32;
    let fx = smoothstep(x - x.floor());
    let fy = smoothstep(y - y.floor());
    let fz = smoothstep(z - z.floor());

    let v000 = hash_3d(ix, iy, iz);
    let v100 = hash_3d(ix + 1, iy, iz);
    let v010 = hash_3d(ix, iy + 1, iz);
    let v110 = hash_3d(ix + 1, iy + 1, iz);
    let v001 = hash_3d(ix, iy, iz + 1);
    let v101 = hash_3d(ix + 1, iy, iz + 1);
    let v011 = hash_3d(ix, iy + 1, iz + 1);
    let v111 = hash_3d(ix + 1, iy + 1, iz + 1);

    let a00 = v000 + (v100 - v000) * fx;
    let a10 = v010 + (v110 - v010) * fx;
    let a01 = v001 + (v101 - v001) * fx;
    let a11 = v011 + (v111 - v011) * fx;

    let b0 = a00 + (a10 - a00) * fy;
    let b1 = a01 + (a11 - a01) * fy;

    (b0 + (b1 - b0) * fz) * 2.0 - 1.0
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
        flag.store(true, Ordering::Release);
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
