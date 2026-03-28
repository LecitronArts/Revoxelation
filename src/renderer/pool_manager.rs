//! PoolManager sub-struct — GPU resource pools and managers (REFAC-01).
//!
//! Groups GPU resource pools (chunks, meshlets), staging ring, bindless
//! descriptors, and texture arrays. These manage the GPU-side storage for
//! game world data.

use super::{bindless, chunk_pool, staging_ring, texture_array};

/// GPU resource pools and managers (REFAC-01).
///
/// Logical view into the renderer's pool handles. Used as a borrow-friendly
/// reference bundle when functions need pool access without borrowing
/// the entire Renderer.
#[allow(dead_code)]
pub struct PoolManager<'a> {
    pub chunk_pool: Option<&'a chunk_pool::ChunkPool>,
    pub meshlet_pool: Option<&'a chunk_pool::MeshletPool>,
    pub staging_ring: Option<&'a staging_ring::StagingRing>,
    pub bindless: Option<&'a bindless::BindlessTable>,
    pub texture_array: Option<&'a texture_array::TextureArray>,
}
