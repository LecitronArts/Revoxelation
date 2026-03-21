use std::collections::{HashMap, VecDeque};

use crate::streaming::types::ChunkKey;

use super::{ChunkVoxels, MeshingJobResult};

pub const FACE_POS_X: u8 = 1 << 0;
pub const FACE_NEG_X: u8 = 1 << 1;
pub const FACE_POS_Y: u8 = 1 << 2;
pub const FACE_NEG_Y: u8 = 1 << 3;
pub const FACE_POS_Z: u8 = 1 << 4;
pub const FACE_NEG_Z: u8 = 1 << 5;
pub const ALL_FACE_MASK: u8 =
    FACE_POS_X | FACE_NEG_X | FACE_POS_Y | FACE_NEG_Y | FACE_POS_Z | FACE_NEG_Z;

const FACE_DIRECTIONS: &[(u8, (i32, i32, i32))] = &[
    (FACE_POS_X, (1, 0, 0)),
    (FACE_NEG_X, (-1, 0, 0)),
    (FACE_POS_Y, (0, 1, 0)),
    (FACE_NEG_Y, (0, -1, 0)),
    (FACE_POS_Z, (0, 0, 1)),
    (FACE_NEG_Z, (0, 0, -1)),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeshDirtyCause {
    GeneratedPayload,
    BorderTouched { face_mask: u8 },
    FinerNeighborMaskChanged { face_mask: u8, active: bool },
    RevisionMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshDirtyRecord {
    pub causes: Vec<MeshDirtyCause>,
    pub source_revision: u64,
    pub finer_neighbor_face_mask: u8,
}

#[derive(Debug, Default)]
pub struct MeshingState {
    pub payloads: HashMap<ChunkKey, ChunkVoxels>,
    pub dirty: HashMap<ChunkKey, MeshDirtyRecord>,
    pub queued: VecDeque<ChunkKey>,
    pub completed_meshes: Vec<MeshingJobResult>,
}

impl MeshingState {
    pub fn mark_dirty(&mut self, key: ChunkKey, cause: MeshDirtyCause, source_revision: u64) {
        let should_queue = !self.dirty.contains_key(&key);
        let entry = self
            .dirty
            .entry(key)
            .or_insert_with(|| MeshDirtyRecord {
                causes: Vec::new(),
                source_revision,
                finer_neighbor_face_mask: 0,
            });
        if entry.source_revision != source_revision {
            entry.causes.push(MeshDirtyCause::RevisionMismatch);
            entry.source_revision = source_revision;
        }
        entry.causes.push(cause);
        if should_queue {
            self.queued.push_back(key);
        }
    }

    pub fn mark_face_neighbors_dirty(&mut self, key: ChunkKey, face_mask: u8, source_revision: u64) {
        for &(face, (dx, dy, dz)) in FACE_DIRECTIONS {
            if face_mask & face == 0 {
                continue;
            }

            let neighbor = ChunkKey::new(key.x + dx, key.y + dy, key.z + dz, key.lod_level);
            self.mark_dirty(
                neighbor,
                MeshDirtyCause::BorderTouched {
                    face_mask: opposite_face(face),
                },
                source_revision,
            );
        }
    }

    pub fn update_finer_neighbor_face_mask(
        &mut self,
        coarse_key: ChunkKey,
        face_mask: u8,
        active: bool,
        source_revision: u64,
    ) {
        let should_queue = !self.dirty.contains_key(&coarse_key);
        let entry = self
            .dirty
            .entry(coarse_key)
            .or_insert_with(|| MeshDirtyRecord {
                causes: Vec::new(),
                source_revision,
                finer_neighbor_face_mask: 0,
            });
        let next_mask = if active {
            entry.finer_neighbor_face_mask | face_mask
        } else {
            entry.finer_neighbor_face_mask & !face_mask
        };
        if next_mask == entry.finer_neighbor_face_mask {
            return;
        }

        if entry.source_revision != source_revision {
            entry.causes.push(MeshDirtyCause::RevisionMismatch);
            entry.source_revision = source_revision;
        }
        entry.finer_neighbor_face_mask = next_mask;
        entry
            .causes
            .push(MeshDirtyCause::FinerNeighborMaskChanged { face_mask, active });
        if should_queue {
            self.queued.push_back(coarse_key);
        }
    }

    pub fn mark_coarse_lod_neighbors_dirty(
        &mut self,
        key: ChunkKey,
        finer_face_mask: u8,
        activated: bool,
        source_revision: u64,
    ) {
        let coarse_lod = match key.lod_level.checked_add(1) {
            Some(level) => level,
            None => return,
        };
        let parent_x = key.x.div_euclid(2);
        let parent_y = key.y.div_euclid(2);
        let parent_z = key.z.div_euclid(2);

        let mappings = [
            (FACE_NEG_X, ChunkKey::new(parent_x - 1, parent_y, parent_z, coarse_lod), FACE_POS_X),
            (FACE_POS_X, ChunkKey::new(parent_x + 1, parent_y, parent_z, coarse_lod), FACE_NEG_X),
            (FACE_NEG_Y, ChunkKey::new(parent_x, parent_y - 1, parent_z, coarse_lod), FACE_POS_Y),
            (FACE_POS_Y, ChunkKey::new(parent_x, parent_y + 1, parent_z, coarse_lod), FACE_NEG_Y),
            (FACE_NEG_Z, ChunkKey::new(parent_x, parent_y, parent_z - 1, coarse_lod), FACE_POS_Z),
            (FACE_POS_Z, ChunkKey::new(parent_x, parent_y, parent_z + 1, coarse_lod), FACE_NEG_Z),
        ];

        for (finer_face, coarse_key, coarse_face) in mappings {
            if finer_face_mask & finer_face == 0 {
                continue;
            }
            self.update_finer_neighbor_face_mask(
                coarse_key,
                coarse_face,
                activated,
                source_revision,
            );
        }
    }

    pub fn take_dirty_batch(&mut self, max: usize) -> Vec<ChunkKey> {
        let mut batch = Vec::with_capacity(max.min(self.queued.len()));
        while batch.len() < max {
            let Some(key) = self.queued.pop_front() else {
                break;
            };
            if self.dirty.contains_key(&key) {
                batch.push(key);
            }
        }
        batch
    }
}

pub fn fine_chunk_boundary_mask(key: ChunkKey) -> u8 {
    let mut face_mask = 0;
    if key.x.rem_euclid(2) == 0 {
        face_mask |= FACE_NEG_X;
    } else {
        face_mask |= FACE_POS_X;
    }
    if key.y.rem_euclid(2) == 0 {
        face_mask |= FACE_NEG_Y;
    } else {
        face_mask |= FACE_POS_Y;
    }
    if key.z.rem_euclid(2) == 0 {
        face_mask |= FACE_NEG_Z;
    } else {
        face_mask |= FACE_POS_Z;
    }
    face_mask
}

fn opposite_face(face_mask: u8) -> u8 {
    match face_mask {
        FACE_POS_X => FACE_NEG_X,
        FACE_NEG_X => FACE_POS_X,
        FACE_POS_Y => FACE_NEG_Y,
        FACE_NEG_Y => FACE_POS_Y,
        FACE_POS_Z => FACE_NEG_Z,
        FACE_NEG_Z => FACE_POS_Z,
        _ => 0,
    }
}
