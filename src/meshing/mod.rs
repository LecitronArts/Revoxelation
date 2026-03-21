pub mod packing;

use crate::streaming::types::ChunkKey;

pub use crate::streaming::types::ChunkVoxels;
pub use packing::{PackedMesh, PackedVertex, pack_vertex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GreedyQuad {
    pub axis: u8,
    pub positive_face: bool,
    pub origin: [u8; 3],
    pub size: [u8; 2],
    pub block_id: u16,
    pub is_skirt: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshingJobResult {
    pub key: ChunkKey,
    pub mesh: PackedMesh,
    pub source_revision: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChunkNeighborSet<'a> {
    pub px: Option<&'a ChunkVoxels>,
    pub nx: Option<&'a ChunkVoxels>,
    pub py: Option<&'a ChunkVoxels>,
    pub ny: Option<&'a ChunkVoxels>,
    pub pz: Option<&'a ChunkVoxels>,
    pub nz: Option<&'a ChunkVoxels>,
    pub finer_neighbor_face_mask: u8,
}
