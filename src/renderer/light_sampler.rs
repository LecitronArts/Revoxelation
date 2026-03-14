#![allow(dead_code)]

use std::collections::HashMap;

use super::protocol::EmissiveVoxelGpu;

pub const IMPORTANCE_MAP_DIMS: [u32; 3] = [64, 32, 64];
pub const INVALID_EMITTER_INDEX: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct LightSamplerTables {
    pub emissive_cdf: Vec<f32>,
    pub emissive_signatures: Vec<u32>,
    pub importance_map_dims: [u32; 3],
    pub importance_map_texels: Vec<f32>,
}

pub fn build_light_sampler(
    emissive_voxels: &[EmissiveVoxelGpu],
    world_min: [i32; 3],
    world_max: [i32; 3],
) -> LightSamplerTables {
    let importance_map_dims = IMPORTANCE_MAP_DIMS;
    let mut importance_map_texels = vec![
        0.0;
        (importance_map_dims[0] * importance_map_dims[1] * importance_map_dims[2])
            as usize
    ];
    let mut signatures = Vec::with_capacity(emissive_voxels.len());
    let mut weights = Vec::with_capacity(emissive_voxels.len());

    for voxel in emissive_voxels {
        let pos = [
            voxel.position_power[0],
            voxel.position_power[1],
            voxel.position_power[2],
        ];
        let power = voxel.position_power[3].max(0.0);
        let texel = importance_map_coord(pos, world_min, world_max, importance_map_dims);
        let linear = importance_linear_index(texel, importance_map_dims);
        importance_map_texels[linear] += power;

        let signature = signature_from_emitter(*voxel);
        signatures.push(signature);
        weights.push(power);
    }

    normalize_importance_map(&mut importance_map_texels);
    let emissive_cdf = build_cdf(&weights);
    LightSamplerTables {
        emissive_cdf,
        emissive_signatures: signatures,
        importance_map_dims,
        importance_map_texels,
    }
}

pub fn build_shift_mapping(previous_signatures: &[u32], current_signatures: &[u32]) -> Vec<u32> {
    if previous_signatures.is_empty() {
        return vec![INVALID_EMITTER_INDEX];
    }

    let mut current_lookup = HashMap::with_capacity(current_signatures.len());
    for (idx, sig) in current_signatures.iter().copied().enumerate() {
        current_lookup.entry(sig).or_insert(idx as u32);
    }

    let mut remap = Vec::with_capacity(previous_signatures.len());
    for sig in previous_signatures.iter().copied() {
        let mapped = current_lookup
            .get(&sig)
            .copied()
            .unwrap_or(INVALID_EMITTER_INDEX);
        remap.push(mapped);
    }
    remap
}

pub fn sample_cdf(cdf: &[f32], u: f32) -> u32 {
    if cdf.is_empty() {
        return 0;
    }
    let target = u.clamp(0.0, 0.999_999_94);
    let mut low = 0usize;
    let mut high = cdf.len() - 1;
    while low < high {
        let mid = (low + high) >> 1;
        if cdf[mid] < target {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    low as u32
}

pub fn proposal_pdf(cdf: &[f32], index: u32) -> f32 {
    if cdf.is_empty() {
        return 1.0;
    }
    let idx = (index as usize).min(cdf.len() - 1);
    let curr = cdf[idx];
    let prev = if idx == 0 { 0.0 } else { cdf[idx - 1] };
    (curr - prev).max(1.0e-6)
}

pub fn signature_from_emitter(voxel: EmissiveVoxelGpu) -> u32 {
    let px = (voxel.position_power[0] * 4.0).round() as i32;
    let py = (voxel.position_power[1] * 4.0).round() as i32;
    let pz = (voxel.position_power[2] * 4.0).round() as i32;
    let pw = (voxel.position_power[3] * 8.0).round() as i32;
    let mut h = 0x9e37_79b9u32 ^ (px as u32).wrapping_mul(0x85eb_ca6b);
    h ^= (py as u32).wrapping_mul(0xc2b2_ae35);
    h ^= (pz as u32).wrapping_mul(0x27d4_eb2d);
    h ^= (pw as u32).wrapping_mul(0x1656_67b1);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^ (h >> 16)
}

fn build_cdf(weights: &[f32]) -> Vec<f32> {
    if weights.is_empty() {
        return vec![1.0];
    }

    let sum: f32 = weights.iter().copied().sum();
    if sum <= 1.0e-6 {
        let inv = 1.0 / weights.len() as f32;
        let mut accum = 0.0;
        return weights
            .iter()
            .map(|_| {
                accum += inv;
                accum
            })
            .collect();
    }

    let mut accum = 0.0;
    let mut cdf = Vec::with_capacity(weights.len());
    for w in weights {
        accum += *w / sum;
        cdf.push(accum.min(1.0));
    }
    if let Some(last) = cdf.last_mut() {
        *last = 1.0;
    }
    cdf
}

fn importance_map_coord(
    world_pos: [f32; 3],
    world_min: [i32; 3],
    world_max: [i32; 3],
    dims: [u32; 3],
) -> [u32; 3] {
    let span = [
        (world_max[0] - world_min[0]).max(1) as f32,
        (world_max[1] - world_min[1]).max(1) as f32,
        (world_max[2] - world_min[2]).max(1) as f32,
    ];
    let rel = [
        ((world_pos[0] - world_min[0] as f32) / span[0]).clamp(0.0, 0.999_999),
        ((world_pos[1] - world_min[1] as f32) / span[1]).clamp(0.0, 0.999_999),
        ((world_pos[2] - world_min[2] as f32) / span[2]).clamp(0.0, 0.999_999),
    ];
    [
        (rel[0] * dims[0] as f32) as u32,
        (rel[1] * dims[1] as f32) as u32,
        (rel[2] * dims[2] as f32) as u32,
    ]
}

fn importance_linear_index(coord: [u32; 3], dims: [u32; 3]) -> usize {
    (coord[0] + coord[1] * dims[0] + coord[2] * dims[0] * dims[1]) as usize
}

fn normalize_importance_map(texels: &mut [f32]) {
    let mut max_v = 0.0f32;
    for v in texels.iter().copied() {
        max_v = max_v.max(v);
    }
    if max_v <= 1.0e-6 {
        for v in texels.iter_mut() {
            *v = 0.0;
        }
        return;
    }

    for v in texels.iter_mut() {
        *v = (*v / max_v).clamp(0.0, 1.0);
    }
}
