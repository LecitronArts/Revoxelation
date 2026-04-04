//! coords.rs — Unified coordinate-system constants for the entire engine.
//!
//! **Single source of truth** for world-space conversions (CRIT-05).
//!
//! All modules that need to convert between chunk-space / voxel-space / world-
//! space MUST import from here instead of defining local constants.
//!
//! Coordinate hierarchy:
//!   - 1 block (voxel) = `BLOCK_SIZE` metres = 1/16 m = 6.25 cm
//!   - 1 chunk = `CHUNK_EDGE` blocks per axis = 64
//!   - LOD N: each block is 2^N times larger than LOD 0
//!
//! GPU vertex shader formula (unchanged):
//!   `world_position = chunk_origin + decode_position(packed) * chunk_scale`

pub use crate::streaming::types::CHUNK_EDGE;

/// World-space edge length of a single block (voxel) at LOD 0, in metres.
///
/// 1 block = 1/16 m = 6.25 cm.
pub const BLOCK_SIZE: f32 = 1.0 / 16.0;

/// LOD scale factor: LOD N represents `2^N` LOD-0 blocks per voxel.
#[inline]
pub fn lod_scale(lod_level: u8) -> f32 {
    (1_u32 << lod_level) as f32
}

/// GPU chunk_scale sent as `GpuChunkInstance::chunk_scale`.
///
/// Converts a voxel-local coordinate (0..64 range from `decode_position`) into
/// world-space units.  `chunk_scale = BLOCK_SIZE × 2^lod_level`.
///
/// - LOD 0: 0.0625
/// - LOD 1: 0.125
/// - LOD 2: 0.25
#[inline]
pub fn chunk_scale(lod_level: u8) -> f32 {
    BLOCK_SIZE * lod_scale(lod_level)
}

/// World-space edge length of one chunk, in metres.
///
/// - LOD 0: 64 × 1/16 × 1 = 4.0 m
/// - LOD 1: 64 × 1/16 × 2 = 8.0 m
/// - LOD 2: 64 × 1/16 × 4 = 16.0 m
#[inline]
pub fn chunk_world_edge(lod_level: u8) -> f32 {
    CHUNK_EDGE as f32 * BLOCK_SIZE * lod_scale(lod_level)
}

/// World-space origin of a chunk identified by its grid coordinates and LOD.
///
/// `origin = (x, y, z) × chunk_world_edge(lod)`.
#[inline]
pub fn chunk_origin(x: i32, y: i32, z: i32, lod_level: u8) -> [f32; 3] {
    let edge = chunk_world_edge(lod_level);
    [x as f32 * edge, y as f32 * edge, z as f32 * edge]
}

/// Convert a local-space AABB coordinate to world-space.
///
/// `world = origin + local × scale`.
#[inline]
pub fn world_aabb(local: [f32; 3], origin: [f32; 3], scale: f32) -> [f32; 3] {
    [
        origin[0] + local[0] * scale,
        origin[1] + local[1] * scale,
        origin[2] + local[2] * scale,
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod0_chunk_world_edge() {
        let edge = chunk_world_edge(0);
        assert!((edge - 4.0).abs() < 1e-6, "LOD0 chunk should be 4m, got {edge}");
    }

    #[test]
    fn lod1_chunk_world_edge() {
        let edge = chunk_world_edge(1);
        assert!((edge - 8.0).abs() < 1e-6, "LOD1 chunk should be 8m, got {edge}");
    }

    #[test]
    fn lod0_chunk_scale() {
        let scale = chunk_scale(0);
        assert!(
            (scale - BLOCK_SIZE).abs() < 1e-6,
            "LOD0 chunk_scale should be BLOCK_SIZE={BLOCK_SIZE}, got {scale}"
        );
    }

    #[test]
    fn chunk_origin_lod0() {
        let origin = chunk_origin(1, 0, 0, 0);
        assert!((origin[0] - 4.0).abs() < 1e-6, "chunk(1,0,0,LOD0) origin.x should be 4m, got {}", origin[0]);
    }

    #[test]
    fn chunk_origin_lod1() {
        let origin = chunk_origin(1, 0, 0, 1);
        assert!((origin[0] - 8.0).abs() < 1e-6, "chunk(1,0,0,LOD1) origin.x should be 8m, got {}", origin[0]);
    }

    #[test]
    fn world_aabb_identity() {
        let local = [32.0, 32.0, 32.0]; // center of a 64-block chunk
        let origin = chunk_origin(0, 0, 0, 0);
        let scale = chunk_scale(0);
        let world = world_aabb(local, origin, scale);
        // 32 * 0.0625 = 2.0m from origin
        assert!((world[0] - 2.0).abs() < 1e-6, "expected 2.0, got {}", world[0]);
    }
}
