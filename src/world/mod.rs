use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use dashmap::DashMap;
use noise::{NoiseFn, OpenSimplex};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;

pub const CHUNK_SIZE: usize = 32;
pub const CHUNK_SIZE_I32: i32 = CHUNK_SIZE as i32;
pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl ChunkCoord {
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub const fn world_origin(self) -> [i32; 3] {
        [
            self.x * CHUNK_SIZE_I32,
            self.y * CHUNK_SIZE_I32,
            self.z * CHUNK_SIZE_I32,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub coord: ChunkCoord,
    pub voxels: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct WorldGenConfig {
    pub radius_xz_chunks: i32,
    pub half_height_chunks: i32,
    pub seed: u32,
    pub terrain_scale: f64,
    pub cave_scale: f64,
}

impl Default for WorldGenConfig {
    fn default() -> Self {
        Self {
            radius_xz_chunks: 4,
            half_height_chunks: 2,
            seed: 0xC0FFEE,
            terrain_scale: 0.022,
            cave_scale: 0.065,
        }
    }
}

impl WorldGenConfig {
    fn enumerate_coords(self) -> Vec<ChunkCoord> {
        let mut coords = Vec::new();
        for y in -self.half_height_chunks..=self.half_height_chunks {
            for z in -self.radius_xz_chunks..=self.radius_xz_chunks {
                for x in -self.radius_xz_chunks..=self.radius_xz_chunks {
                    coords.push(ChunkCoord::new(x, y, z));
                }
            }
        }
        coords
    }
}

pub struct VoxelWorld {
    pub chunks: DashMap<ChunkCoord, Arc<Chunk>>,
    dirty: AtomicBool,
    generation_epoch: AtomicU64,
    generation_finished: AtomicBool,
    generated_chunks: AtomicU32,
    total_chunks: AtomicU32,
}

impl VoxelWorld {
    pub fn new() -> Self {
        Self {
            chunks: DashMap::new(),
            dirty: AtomicBool::new(false),
            generation_epoch: AtomicU64::new(0),
            generation_finished: AtomicBool::new(false),
            generated_chunks: AtomicU32::new(0),
            total_chunks: AtomicU32::new(0),
        }
    }

    pub fn spawn_generation(self: &Arc<Self>, config: WorldGenConfig) {
        let epoch = self.generation_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.chunks.clear();
        self.dirty.store(true, Ordering::Release);
        self.generation_finished.store(false, Ordering::Release);
        self.generated_chunks.store(0, Ordering::Release);

        let coords = config.enumerate_coords();
        self.total_chunks
            .store(coords.len() as u32, Ordering::Release);

        let world = Arc::clone(self);
        std::thread::spawn(move || {
            coords.into_par_iter().for_each(|coord| {
                if !world.is_epoch_current(epoch) {
                    return;
                }
                let generation =
                    generate_chunk_cancelable(coord, config, || !world.is_epoch_current(epoch));
                let Some(chunk) = generation.chunk else {
                    return;
                };
                if !world.is_epoch_current(epoch) {
                    return;
                }

                let inserted_chunk = Arc::new(chunk);
                world.chunks.insert(coord, Arc::clone(&inserted_chunk));
                if !world.is_epoch_current(epoch) {
                    let _ = world
                        .chunks
                        .remove_if(&coord, |_, existing| Arc::ptr_eq(existing, &inserted_chunk));
                    return;
                }
                let produced = world.generated_chunks.fetch_add(1, Ordering::AcqRel) + 1;
                if produced & 31 == 0 {
                    world.dirty.store(true, Ordering::Release);
                }
            });

            if world.is_epoch_current(epoch) {
                world.dirty.store(true, Ordering::Release);
                world.generation_finished.store(true, Ordering::Release);
            }
        });
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn generation_snapshot(&self) -> (u32, u32, bool) {
        loop {
            let epoch = self.generation_epoch.load(Ordering::Acquire);
            let total = self.total_chunks.load(Ordering::Acquire);
            let finished = self.generation_finished.load(Ordering::Acquire);
            let generated = (self.chunks.len() as u32).min(total);
            if self.generation_epoch.load(Ordering::Acquire) == epoch {
                return (generated, total, finished);
            }
        }
    }

    #[cfg(test)]
    pub fn generation_epoch(&self) -> u64 {
        self.generation_epoch.load(Ordering::Acquire)
    }

    fn is_epoch_current(&self, epoch: u64) -> bool {
        self.generation_epoch.load(Ordering::Acquire) == epoch
    }
}

pub const fn pack_voxel(material_or_color: u8, emissive: u8, reserved: u16) -> u32 {
    (material_or_color as u32) | ((emissive as u32) << 8) | ((reserved as u32) << 16)
}

#[derive(Debug)]
struct ChunkGeneration {
    chunk: Option<Chunk>,
    #[allow(dead_code)]
    z_layers_processed: usize,
}

fn generate_chunk_cancelable(
    coord: ChunkCoord,
    config: WorldGenConfig,
    should_cancel: impl Fn() -> bool,
) -> ChunkGeneration {
    let mut voxels = vec![0_u32; CHUNK_VOLUME];
    let terrain_noise = OpenSimplex::new(config.seed);
    let cave_noise = OpenSimplex::new(config.seed ^ 0x9E37_79B9);

    let seed = ((coord.x as i64 as u64) << 40)
        ^ ((coord.y as i64 as u64) << 20)
        ^ (coord.z as i64 as u64)
        ^ (config.seed as u64);
    let mut rng = StdRng::seed_from_u64(seed);

    let origin = coord.world_origin();
    let mut z_layers_processed = 0_usize;
    for z in 0..CHUNK_SIZE {
        if should_cancel() {
            return ChunkGeneration {
                chunk: None,
                z_layers_processed,
            };
        }
        z_layers_processed = z_layers_processed.saturating_add(1);
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let world_x = origin[0] + x as i32;
                let world_y = origin[1] + y as i32;
                let world_z = origin[2] + z as i32;

                let terrain = terrain_noise.get([
                    world_x as f64 * config.terrain_scale,
                    world_z as f64 * config.terrain_scale,
                    0.0,
                ]) as f32;
                let terrain_height = (terrain * 22.0 + 34.0) as i32;

                let caves = cave_noise.get([
                    world_x as f64 * config.cave_scale,
                    world_y as f64 * config.cave_scale,
                    world_z as f64 * config.cave_scale,
                ]) as f32;

                let solid = world_y <= terrain_height && caves > -0.35;
                if !solid {
                    continue;
                }

                let material = if world_y > terrain_height - 2 {
                    2
                } else if world_y > terrain_height - 8 {
                    1
                } else {
                    3
                };

                let emissive = if world_y < 12 && rng.random::<f32>() > 0.998 {
                    40
                } else {
                    0
                };

                let linear_index = x + y * CHUNK_SIZE + z * CHUNK_SIZE * CHUNK_SIZE;
                voxels[linear_index] = pack_voxel(material, emissive, 0);
            }
        }
    }

    ChunkGeneration {
        chunk: Some(Chunk { coord, voxels }),
        z_layers_processed,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{CHUNK_SIZE, ChunkCoord, VoxelWorld, WorldGenConfig, generate_chunk_cancelable};

    fn wait_generation_finished(world: &VoxelWorld, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        loop {
            let (_, _, finished) = world.generation_snapshot();
            if finished {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "generation did not finish within {:?}",
                timeout
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn spawn_generation_cancels_previous_epoch_results() {
        let world = Arc::new(VoxelWorld::new());
        let mut large = WorldGenConfig::default();
        large.radius_xz_chunks = 10;
        large.half_height_chunks = 2;
        let mut small = WorldGenConfig::default();
        small.radius_xz_chunks = 0;
        small.half_height_chunks = 0;
        small.seed = 0xBADC0DE;

        world.spawn_generation(large);
        world.spawn_generation(small);
        wait_generation_finished(&world, Duration::from_secs(10));

        let (generated, total, finished) = world.generation_snapshot();
        assert!(finished);
        assert_eq!(total, 1);
        assert_eq!(generated, 1);
        assert_eq!(world.generation_epoch(), 2);
        assert_eq!(world.chunks.len(), 1);
        assert!(world.chunks.contains_key(&ChunkCoord::new(0, 0, 0)));

        thread::sleep(Duration::from_millis(50));
        assert_eq!(world.chunks.len(), 1);
    }

    #[test]
    fn chunk_generation_cancels_before_full_chunk_work() {
        let checks = AtomicUsize::new(0);
        let result =
            generate_chunk_cancelable(ChunkCoord::new(0, 0, 0), WorldGenConfig::default(), || {
                let observed = checks.fetch_add(1, Ordering::AcqRel);
                observed >= 2
            });

        assert!(result.chunk.is_none());
        assert_eq!(result.z_layers_processed, 2);
        assert!(result.z_layers_processed < CHUNK_SIZE);
    }
}
