pub mod greedy;
pub mod invalidation;
pub mod packing;

use std::sync::Arc;

use crate::streaming::types::ChunkKey;

pub use crate::streaming::types::ChunkVoxels;
pub use greedy::build_greedy_mesh;
pub use invalidation::{
    ALL_FACE_MASK, FACE_NEG_X, FACE_NEG_Y, FACE_NEG_Z, FACE_POS_X, FACE_POS_Y, FACE_POS_Z,
    MeshDirtyCause, MeshDirtyRecord, MeshingState, fine_chunk_boundary_mask,
};
pub use packing::{
    MeshletDescriptor, MeshletMesh, PackedMesh, PackedVertex, build_meshlets_from_packed,
    pack_quad, pack_vertex,
};

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

/// Owned variant of [`ChunkNeighborSet`] that holds `Arc<ChunkVoxels>` instead
/// of references, allowing it to be sent to rayon worker threads.
#[derive(Debug, Clone)]
pub struct OwnedChunkNeighborSet {
    pub px: Option<Arc<ChunkVoxels>>,
    pub nx: Option<Arc<ChunkVoxels>>,
    pub py: Option<Arc<ChunkVoxels>>,
    pub ny: Option<Arc<ChunkVoxels>>,
    pub pz: Option<Arc<ChunkVoxels>>,
    pub nz: Option<Arc<ChunkVoxels>>,
    pub finer_neighbor_face_mask: u8,
}

impl OwnedChunkNeighborSet {
    /// Create a borrowed [`ChunkNeighborSet`] referencing the `Arc` payloads.
    pub fn as_borrowed(&self) -> ChunkNeighborSet<'_> {
        ChunkNeighborSet {
            px: self.px.as_deref(),
            nx: self.nx.as_deref(),
            py: self.py.as_deref(),
            ny: self.ny.as_deref(),
            pz: self.pz.as_deref(),
            nz: self.nz.as_deref(),
            finer_neighbor_face_mask: self.finer_neighbor_face_mask,
        }
    }
}

/// Result of an off-thread mesh build job (greedy meshing + meshlet construction).
pub struct MeshBuildResult {
    pub key: ChunkKey,
    pub meshlet_mesh: MeshletMesh,
    pub packed_mesh: PackedMesh,
    pub dirty_record: MeshDirtyRecord,
}
