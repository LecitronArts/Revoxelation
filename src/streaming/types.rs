//! Shared contracts for the streaming subsystem.
//!
//! All types in this module are the single source of truth consumed by
//! state_store, octree, sse, and the job dispatcher in later plans.

use std::collections::HashSet;

pub const CHUNK_EDGE: usize = 64;
pub const CHUNK_VOXEL_COUNT: usize = CHUNK_EDGE * CHUNK_EDGE * CHUNK_EDGE;

// ---------------------------------------------------------------------------
// ChunkKey
// ---------------------------------------------------------------------------

/// Uniquely identifies a chunk at a given LOD level.
///
/// `(x, y, z)` are chunk-space coordinates; `lod_level` distinguishes the
/// resolution tier (0 = finest, higher = coarser).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkKey {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub lod_level: u8,
}

impl ChunkKey {
    pub fn new(x: i32, y: i32, z: i32, lod_level: u8) -> Self {
        Self { x, y, z, lod_level }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkVoxels {
    pub block_ids: Box<[u8]>,
}

impl ChunkVoxels {
    pub fn new(block_ids: Box<[u8]>) -> Result<Self, String> {
        if block_ids.len() != CHUNK_VOXEL_COUNT {
            return Err(format!(
                "chunk payload length mismatch: expected {}, got {}",
                CHUNK_VOXEL_COUNT,
                block_ids.len()
            ));
        }

        Ok(Self { block_ids })
    }

    pub fn block(&self, x: u8, y: u8, z: u8) -> u8 {
        self.block_ids[Self::linear_index(x, y, z)]
    }

    pub fn linear_index(x: u8, y: u8, z: u8) -> usize {
        let x = usize::from(x);
        let y = usize::from(y);
        let z = usize::from(z);
        debug_assert!(x < CHUNK_EDGE);
        debug_assert!(y < CHUNK_EDGE);
        debug_assert!(z < CHUNK_EDGE);
        x + (y * CHUNK_EDGE) + (z * CHUNK_EDGE * CHUNK_EDGE)
    }
}

// ---------------------------------------------------------------------------
// ChunkState
// ---------------------------------------------------------------------------

/// Seven canonical states of a chunk's lifecycle, plus an Error variant.
///
/// Valid transitions (enforced by `ChunkStateStore`):
/// ```text
/// Inactive  → Queued
/// Queued    → Loading | Inactive
/// Loading   → Active  | Error
/// Active    → Upgrading | Downgrading | Unloading
/// Upgrading → Active  | Unloading
/// Downgrading→ Active | Unloading
/// Unloading → Inactive
/// Error     → Queued  | Inactive
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChunkState {
    Inactive,
    Queued,
    Loading,
    Active,
    Upgrading,
    Downgrading,
    Unloading,
    /// Background job failed; tracks retry bookkeeping.
    Error {
        retry_count: u32,
        next_retry_frame: u64,
    },
}

// ---------------------------------------------------------------------------
// ChunkEntry
// ---------------------------------------------------------------------------

/// A single record tracked by `ChunkStateStore`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkEntry {
    pub key: ChunkKey,
    pub state: ChunkState,
    /// Incremented only when state enters `Active` or `Inactive`.
    pub revision: u64,
}

impl ChunkEntry {
    pub fn new(key: ChunkKey) -> Self {
        Self {
            key,
            state: ChunkState::Inactive,
            revision: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// LodConfig
// ---------------------------------------------------------------------------

/// Per-level geometric error and chunk world-space size.
#[derive(Debug, Clone)]
pub struct LodConfig {
    /// World-space geometric error in metres for this LOD level.
    pub geometric_error: f32,
    /// Edge length of one chunk at this LOD level in metres.
    pub chunk_world_size: f32,
}

impl LodConfig {
    pub fn new(geometric_error: f32, chunk_world_size: f32) -> Self {
        Self {
            geometric_error,
            chunk_world_size,
        }
    }
}

// ---------------------------------------------------------------------------
// SseConfig
// ---------------------------------------------------------------------------

/// Camera and threshold parameters needed by the SSE formula.
#[derive(Debug, Clone)]
pub struct SseConfig {
    /// Screen height in pixels.
    pub screen_height: f32,
    /// Vertical field-of-view in radians.
    pub fov_y_radians: f32,
    /// Pixel threshold: nodes with SSE >= this value need finer detail.
    pub threshold_px: f32,
    /// Whether to cull chunks outside the view frustum (SSE treated as 0).
    pub frustum_culling: bool,
}

impl SseConfig {
    pub fn new(
        screen_height: f32,
        fov_y_radians: f32,
        threshold_px: f32,
        frustum_culling: bool,
    ) -> Self {
        Self {
            screen_height,
            fov_y_radians,
            threshold_px,
            frustum_culling,
        }
    }
}

// ---------------------------------------------------------------------------
// Job result types
// ---------------------------------------------------------------------------

/// Outcome of an individual chunk load/unload background task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkJobOutcome {
    /// Background generation completed successfully; payload is typed chunk data.
    Generated(ChunkVoxels),
    Loaded,
    Unloaded,
    Cancelled,
    Failed(String),
}

/// Result envelope returned from a background job to the main thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkJobResult {
    pub key: ChunkKey,
    pub outcome: ChunkJobOutcome,
}

impl ChunkJobResult {
    pub fn new(key: ChunkKey, outcome: ChunkJobOutcome) -> Self {
        Self { key, outcome }
    }
}

// ---------------------------------------------------------------------------
// Active-set convenience alias
// ---------------------------------------------------------------------------

/// The set of chunk keys currently considered active (visible / in use).
pub type ActiveSet = HashSet<ChunkKey>;
