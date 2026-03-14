use std::sync::Arc;

use log::warn;

use crate::renderer::light_sampler::{LightSamplerTables, build_light_sampler};
use crate::renderer::protocol::{ChunkMapEntryGpu, ChunkMetaGpu, EmissiveVoxelGpu};
use crate::world::VoxelWorld;
use crate::world::{CHUNK_SIZE_I32, Chunk, ChunkCoord};

const MAX_EMISSIVE_CANDIDATES: usize = 16_384;
const CHUNK_MAP_MIN_SIZE: u32 = 64;
const CHUNK_MAP_SOFT_MAX_SIZE: u32 = 1 << 20;

#[derive(Debug, Default)]
pub struct GpuWorldPayload {
    pub voxel_words: Vec<u32>,
    pub chunk_meta: Vec<ChunkMetaGpu>,
    pub chunk_map: Vec<ChunkMapEntryGpu>,
    pub chunk_map_size: u32,
    pub chunk_map_mask: u32,
    pub chunk_map_max_probe: u32,
    pub chunk_map_avg_probe: f32,
    pub chunk_map_max_probe_observed: u32,
    pub chunk_map_load_factor: f32,
    pub chunk_map_dropped_entries: u32,
    pub emissive_voxels: Vec<EmissiveVoxelGpu>,
    pub emissive_cdf: Vec<f32>,
    pub emissive_signatures: Vec<u32>,
    pub emissive_count: u32,
    pub importance_map_dims: [u32; 3],
    pub importance_map_texels: Vec<f32>,
    pub world_min: [i32; 3],
    pub world_max: [i32; 3],
}

impl GpuWorldPayload {
    pub fn chunk_count(&self) -> u32 {
        self.chunk_meta.len() as u32
    }
}

pub fn build_payload(world: &VoxelWorld) -> GpuWorldPayload {
    build_payload_with_chunk_map_soft_max(world, CHUNK_MAP_SOFT_MAX_SIZE)
}

fn build_payload_with_chunk_map_soft_max(
    world: &VoxelWorld,
    chunk_map_soft_max_size: u32,
) -> GpuWorldPayload {
    let mut chunks: Vec<(ChunkCoord, Arc<Chunk>)> = world
        .chunks
        .iter()
        .map(|entry| (*entry.key(), Arc::clone(entry.value())))
        .collect();
    chunks.sort_by_key(|(coord, _)| (coord.y, coord.z, coord.x));

    if chunks.is_empty() {
        return GpuWorldPayload {
            voxel_words: vec![0_u32],
            chunk_meta: vec![ChunkMetaGpu::empty()],
            chunk_map: vec![ChunkMapEntryGpu::empty()],
            chunk_map_size: 1,
            chunk_map_mask: 0,
            chunk_map_max_probe: 1,
            chunk_map_avg_probe: 0.0,
            chunk_map_max_probe_observed: 0,
            chunk_map_load_factor: 0.0,
            chunk_map_dropped_entries: 0,
            emissive_voxels: vec![EmissiveVoxelGpu::empty()],
            emissive_cdf: vec![1.0],
            emissive_signatures: vec![0],
            emissive_count: 0,
            importance_map_dims: [1, 1, 1],
            importance_map_texels: vec![0.0],
            world_min: [0, 0, 0],
            world_max: [1, 1, 1],
        };
    }

    let mut voxel_words = Vec::new();
    let mut chunk_meta = Vec::with_capacity(chunks.len());
    let mut chunk_map_entries = Vec::with_capacity(chunks.len());
    let mut emissive_voxels = Vec::new();
    let mut world_min = [i32::MAX; 3];
    let mut world_max = [i32::MIN; 3];

    for (index, (coord, chunk)) in chunks.into_iter().enumerate() {
        let _chunk_coord_match = chunk.coord == coord;

        let chunk_origin = coord.world_origin();
        world_min[0] = world_min[0].min(chunk_origin[0]);
        world_min[1] = world_min[1].min(chunk_origin[1]);
        world_min[2] = world_min[2].min(chunk_origin[2]);

        world_max[0] = world_max[0].max(chunk_origin[0] + CHUNK_SIZE_I32);
        world_max[1] = world_max[1].max(chunk_origin[1] + CHUNK_SIZE_I32);
        world_max[2] = world_max[2].max(chunk_origin[2] + CHUNK_SIZE_I32);

        let offset = voxel_words.len() as u32;
        voxel_words.extend_from_slice(&chunk.voxels);
        chunk_meta.push(ChunkMetaGpu {
            coord_size: [coord.x, coord.y, coord.z, CHUNK_SIZE_I32],
            voxel_offset: offset,
            voxel_count: chunk.voxels.len() as u32,
            _pad: [0; 2],
        });

        if emissive_voxels.len() < MAX_EMISSIVE_CANDIDATES {
            let chunk_origin = coord.world_origin();
            collect_emissive_voxels(
                &chunk,
                chunk_origin,
                &mut emissive_voxels,
                MAX_EMISSIVE_CANDIDATES,
            );
        }

        let chunk_index_plus_one = (index as i32).saturating_add(1);
        chunk_map_entries.push((coord, chunk_index_plus_one));
    }

    let initial_map_size = next_map_size(chunk_meta.len());
    let (chunk_map, map_size, map_mask, probe_stats, dropped_entries) = build_chunk_map_entries(
        &chunk_map_entries,
        initial_map_size,
        chunk_map_soft_max_size,
    );
    if dropped_entries > 0 {
        warn!(
            "chunk map dropped {dropped_entries} entries (chunk_count={}, map_size={map_size}); continuing with partial map",
            chunk_meta.len()
        );
    }
    let emissive_count = emissive_voxels.len() as u32;
    let chunk_map_max_probe = probe_stats.max_probe_distance.saturating_add(1).max(1);
    let chunk_map_load_factor = chunk_meta.len() as f32 / map_size as f32;
    let lights: LightSamplerTables = build_light_sampler(&emissive_voxels, world_min, world_max);

    GpuWorldPayload {
        voxel_words,
        chunk_meta,
        chunk_map,
        chunk_map_size: map_size,
        chunk_map_mask: map_mask,
        chunk_map_max_probe,
        chunk_map_avg_probe: probe_stats.average_probe_distance(),
        chunk_map_max_probe_observed: probe_stats.max_probe_distance,
        chunk_map_load_factor,
        chunk_map_dropped_entries: dropped_entries,
        emissive_voxels: if emissive_voxels.is_empty() {
            vec![EmissiveVoxelGpu::empty()]
        } else {
            emissive_voxels
        },
        emissive_cdf: lights.emissive_cdf,
        emissive_signatures: if lights.emissive_signatures.is_empty() {
            vec![0]
        } else {
            lights.emissive_signatures
        },
        emissive_count,
        importance_map_dims: lights.importance_map_dims,
        importance_map_texels: lights.importance_map_texels,
        world_min,
        world_max,
    }
}

fn next_map_size(chunk_count: usize) -> u32 {
    let desired = chunk_count.max(1).saturating_mul(4);
    let desired = desired
        .checked_next_power_of_two()
        .unwrap_or(CHUNK_MAP_SOFT_MAX_SIZE as usize);
    desired.clamp(
        CHUNK_MAP_MIN_SIZE as usize,
        CHUNK_MAP_SOFT_MAX_SIZE as usize,
    ) as u32
}

fn build_chunk_map_entries(
    entries: &[(ChunkCoord, i32)],
    initial_map_size: u32,
    map_soft_max_size: u32,
) -> (Vec<ChunkMapEntryGpu>, u32, u32, ChunkMapProbeStats, u32) {
    let map_soft_max_size = map_soft_max_size.max(CHUNK_MAP_MIN_SIZE);
    let mut map_size = initial_map_size
        .max(CHUNK_MAP_MIN_SIZE)
        .min(map_soft_max_size);
    if !map_size.is_power_of_two() {
        map_size = map_size.next_power_of_two();
    }

    loop {
        let map_mask = map_size.saturating_sub(1);
        let max_probe = map_size.saturating_sub(1);
        let mut chunk_map = vec![ChunkMapEntryGpu::empty(); map_size as usize];
        let mut probe_stats = ChunkMapProbeStats::default();
        let mut dropped_entries = 0_u32;

        for &(coord, chunk_index_plus_one) in entries {
            let inserted = insert_chunk_map_entry(
                &mut chunk_map,
                map_mask,
                coord,
                chunk_index_plus_one,
                max_probe,
                &mut probe_stats,
            );
            if !inserted {
                dropped_entries = dropped_entries.saturating_add(1);
            }
        }

        if dropped_entries == 0 {
            return (chunk_map, map_size, map_mask, probe_stats, 0);
        }

        if map_size < map_soft_max_size {
            let grown_size = map_size.saturating_mul(2).min(map_soft_max_size);
            if grown_size > map_size {
                map_size = grown_size;
                continue;
            }
        }

        return (chunk_map, map_size, map_mask, probe_stats, dropped_entries);
    }
}

fn insert_chunk_map_entry(
    chunk_map: &mut [ChunkMapEntryGpu],
    map_mask: u32,
    coord: ChunkCoord,
    chunk_index_plus_one: i32,
    max_probe: u32,
    probe_stats: &mut ChunkMapProbeStats,
) -> bool {
    let mut incoming = ChunkMapSlot {
        coord,
        chunk_index_plus_one,
        hash: hash_chunk_coord(coord),
        probe_distance: 0,
    };
    let mut slot = incoming.hash & map_mask;
    for _ in 0..=max_probe {
        let entry = &mut chunk_map[slot as usize];
        if entry.key_value[3] == 0 {
            write_chunk_map_slot(entry, incoming);
            probe_stats.record(incoming.probe_distance);
            return true;
        }

        let resident_probe = entry.meta[1];
        if resident_probe < incoming.probe_distance {
            let displaced = ChunkMapSlot {
                coord: ChunkCoord::new(entry.key_value[0], entry.key_value[1], entry.key_value[2]),
                chunk_index_plus_one: entry.key_value[3],
                hash: entry.meta[0],
                probe_distance: resident_probe,
            };
            write_chunk_map_slot(entry, incoming);
            incoming = displaced;
        }

        incoming.probe_distance = incoming.probe_distance.saturating_add(1);
        if incoming.probe_distance > max_probe {
            break;
        }
        slot = (slot + 1) & map_mask;
    }
    false
}

fn hash_chunk_coord(coord: ChunkCoord) -> u32 {
    let x = coord.x as u32;
    let y = coord.y as u32;
    let z = coord.z as u32;
    let mut h =
        x.wrapping_mul(0x9E37_79B9) ^ y.wrapping_mul(0x85EB_CA6B) ^ z.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846C_A68B);
    h ^ (h >> 16)
}

#[derive(Debug, Clone, Copy)]
struct ChunkMapSlot {
    coord: ChunkCoord,
    chunk_index_plus_one: i32,
    hash: u32,
    probe_distance: u32,
}

#[derive(Debug, Default)]
struct ChunkMapProbeStats {
    inserted_count: u32,
    total_probe_distance: u64,
    max_probe_distance: u32,
}

impl ChunkMapProbeStats {
    fn record(&mut self, probe_distance: u32) {
        self.inserted_count = self.inserted_count.saturating_add(1);
        self.total_probe_distance = self
            .total_probe_distance
            .saturating_add(probe_distance as u64);
        self.max_probe_distance = self.max_probe_distance.max(probe_distance);
    }

    fn average_probe_distance(&self) -> f32 {
        if self.inserted_count == 0 {
            0.0
        } else {
            self.total_probe_distance as f32 / self.inserted_count as f32
        }
    }
}

fn write_chunk_map_slot(entry: &mut ChunkMapEntryGpu, slot: ChunkMapSlot) {
    entry.key_value = [
        slot.coord.x,
        slot.coord.y,
        slot.coord.z,
        slot.chunk_index_plus_one,
    ];
    entry.meta = [slot.hash, slot.probe_distance, 0, 0];
}

fn collect_emissive_voxels(
    chunk: &Chunk,
    chunk_origin: [i32; 3],
    output: &mut Vec<EmissiveVoxelGpu>,
    max_count: usize,
) {
    for (index, &packed) in chunk.voxels.iter().enumerate() {
        if output.len() >= max_count {
            break;
        }

        let emissive = ((packed >> 8) & 0xff) as u8;
        if emissive == 0 {
            continue;
        }

        let x = (index & 31) as i32;
        let y = ((index >> 5) & 31) as i32;
        let z = ((index >> 10) & 31) as i32;

        output.push(EmissiveVoxelGpu {
            position_power: [
                chunk_origin[0] as f32 + x as f32 + 0.5,
                chunk_origin[1] as f32 + y as f32 + 0.5,
                chunk_origin[2] as f32 + z as f32 + 0.5,
                (emissive as f32 / 255.0) * 18.0,
            ],
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::world::{CHUNK_VOLUME, Chunk, VoxelWorld};

    fn lookup_chunk_index(
        chunk_map: &[ChunkMapEntryGpu],
        map_mask: u32,
        max_probe: u32,
        coord: ChunkCoord,
    ) -> Option<i32> {
        let mut slot = hash_chunk_coord(coord) & map_mask;
        for _ in 0..=max_probe {
            let entry = &chunk_map[slot as usize];
            if entry.key_value[3] == 0 {
                return None;
            }
            if entry.key_value[0] == coord.x
                && entry.key_value[1] == coord.y
                && entry.key_value[2] == coord.z
            {
                return Some(entry.key_value[3]);
            }
            slot = (slot + 1) & map_mask;
        }
        None
    }

    fn collect_colliding_coords(mask: u32, wanted: usize) -> Vec<ChunkCoord> {
        let mut coords = Vec::with_capacity(wanted);
        let mut candidate = 0_i32;
        while coords.len() < wanted && candidate < 2_000_000 {
            let coord = ChunkCoord::new(candidate, candidate.wrapping_mul(17), candidate / 7);
            if (hash_chunk_coord(coord) & mask) == 0 {
                coords.push(coord);
            }
            candidate = candidate.saturating_add(1);
        }
        assert_eq!(
            coords.len(),
            wanted,
            "failed to find enough colliding coords"
        );
        coords
    }

    #[test]
    fn chunk_map_build_recovers_under_high_collision() {
        let colliding_coords = collect_colliding_coords(1023, 320);
        let entries = colliding_coords
            .iter()
            .enumerate()
            .map(|(index, coord)| (*coord, (index as i32) + 1))
            .collect::<Vec<_>>();
        let (chunk_map, map_size, map_mask, probe_stats, dropped_entries) =
            build_chunk_map_entries(&entries, CHUNK_MAP_MIN_SIZE, CHUNK_MAP_SOFT_MAX_SIZE);

        assert_eq!(dropped_entries, 0);
        assert!(map_size >= CHUNK_MAP_MIN_SIZE);
        assert!(probe_stats.max_probe_distance > 0);

        for (index, coord) in colliding_coords.iter().enumerate() {
            let expected = (index as i32) + 1;
            let resolved =
                lookup_chunk_index(&chunk_map, map_mask, probe_stats.max_probe_distance, *coord);
            assert_eq!(resolved, Some(expected));
        }
    }

    #[test]
    fn chunk_map_dropped_entries_reported_in_payload() {
        let world = VoxelWorld::new();
        let colliding_coords = collect_colliding_coords(63, 96);
        for coord in &colliding_coords {
            world.chunks.insert(
                *coord,
                Arc::new(Chunk {
                    coord: *coord,
                    voxels: vec![0_u32; CHUNK_VOLUME],
                }),
            );
        }

        let payload = build_payload_with_chunk_map_soft_max(&world, 64);
        assert!(payload.chunk_map_dropped_entries > 0);
        assert_eq!(payload.chunk_map_size, 64);
        assert!(payload.chunk_map_dropped_entries < colliding_coords.len() as u32);
    }
}
