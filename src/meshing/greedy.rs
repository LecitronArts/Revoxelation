use super::{
    ChunkNeighborSet, ChunkVoxels, FACE_NEG_X, FACE_NEG_Y, FACE_NEG_Z, FACE_POS_X, FACE_POS_Y,
    FACE_POS_Z, GreedyQuad, MeshDirtyRecord, PackedMesh, pack_quad,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MergeKey {
    axis: u8,
    positive_face: bool,
    block_id: u16,
    is_skirt: bool,
}

pub fn build_greedy_mesh(
    chunk: &ChunkVoxels,
    neighbors: &ChunkNeighborSet<'_>,
    dirty: &MeshDirtyRecord,
) -> PackedMesh {
    let mut quads = Vec::new();
    for axis in 0..3u8 {
        emit_quads_for_axis(chunk, neighbors, axis, false, false, &mut quads);
        emit_quads_for_axis(chunk, neighbors, axis, true, false, &mut quads);
    }

    let skirt_mask = neighbors.finer_neighbor_face_mask | dirty.finer_neighbor_face_mask;
    for (face_mask, axis, positive_face) in [
        (FACE_POS_X, 0u8, true),
        (FACE_NEG_X, 0u8, false),
        (FACE_POS_Y, 1u8, true),
        (FACE_NEG_Y, 1u8, false),
        (FACE_POS_Z, 2u8, true),
        (FACE_NEG_Z, 2u8, false),
    ] {
        if skirt_mask & face_mask != 0 {
            emit_quads_for_axis(chunk, neighbors, axis, positive_face, true, &mut quads);
        }
    }

    let mut vertices = Vec::with_capacity(quads.len() * 4);
    let mut indices = Vec::with_capacity(quads.len() * 6);
    let mut aabb_min = [f32::INFINITY; 3];
    let mut aabb_max = [f32::NEG_INFINITY; 3];

    for quad in &quads {
        let (quad_min, quad_max) = quad_bounds(quad);
        for axis in 0..3 {
            aabb_min[axis] = aabb_min[axis].min(quad_min[axis]);
            aabb_max[axis] = aabb_max[axis].max(quad_max[axis]);
        }
        pack_quad(quad, &mut vertices, &mut indices);
    }

    if quads.is_empty() {
        aabb_min = [0.0; 3];
        aabb_max = [0.0; 3];
    }

    PackedMesh {
        vertices: vertices.into_boxed_slice(),
        indices: indices.into_boxed_slice(),
        quad_count: quads.len() as u32,
        aabb_min,
        aabb_max,
    }
}

fn emit_quads_for_axis(
    chunk: &ChunkVoxels,
    neighbors: &ChunkNeighborSet<'_>,
    axis: u8,
    positive_face: bool,
    is_skirt: bool,
    quads: &mut Vec<GreedyQuad>,
) {
    let boundary_slice = if positive_face { 63usize } else { 0usize };
    let mut mask = vec![None; 64 * 64];
    for slice in 0..64usize {
        if is_skirt && slice != boundary_slice {
            continue;
        }

        mask.fill(None);
        for v in 0..64usize {
            for u in 0..64usize {
                let coords = coords_for_cell(axis, slice, u, v);
                let current_block = chunk.block(coords[0], coords[1], coords[2]);
                if current_block == 0 {
                    continue;
                }

                let [x, y, z] = [coords[0] as i32, coords[1] as i32, coords[2] as i32];
                let neighbor_block = match axis {
                    0 if positive_face => sample_with_halo(chunk, neighbors, x + 1, y, z),
                    0 => sample_with_halo(chunk, neighbors, x - 1, y, z),
                    1 if positive_face => sample_with_halo(chunk, neighbors, x, y + 1, z),
                    1 => sample_with_halo(chunk, neighbors, x, y - 1, z),
                    2 if positive_face => sample_with_halo(chunk, neighbors, x, y, z + 1),
                    2 => sample_with_halo(chunk, neighbors, x, y, z - 1),
                    _ => 0,
                };

                if neighbor_block == 0 {
                    mask[(v * 64) + u] = Some(MergeKey {
                        axis,
                        positive_face,
                        block_id: current_block as u16,
                        is_skirt,
                    });
                }
            }
        }

        greedy_merge_mask(axis, slice, &mut mask, quads);
    }
}

fn greedy_merge_mask(
    axis: u8,
    slice: usize,
    mask: &mut [Option<MergeKey>],
    quads: &mut Vec<GreedyQuad>,
) {
    for v in 0..64usize {
        for u in 0..64usize {
            let idx = (v * 64) + u;
            let Some(key) = mask[idx] else {
                continue;
            };

            let mut width = 1usize;
            while u + width < 64 && mask[idx + width] == Some(key) {
                width += 1;
            }

            let mut height = 1usize;
            'height: while v + height < 64 {
                for du in 0..width {
                    if mask[((v + height) * 64) + u + du] != Some(key) {
                        break 'height;
                    }
                }
                height += 1;
            }

            for dv in 0..height {
                for du in 0..width {
                    mask[((v + dv) * 64) + u + du] = None;
                }
            }

            quads.push(GreedyQuad {
                axis,
                positive_face: key.positive_face,
                origin: coords_for_cell(axis, slice, u, v),
                size: [width as u8, height as u8],
                block_id: key.block_id,
                is_skirt: key.is_skirt,
            });
        }
    }
}

fn coords_for_cell(axis: u8, slice: usize, u: usize, v: usize) -> [u8; 3] {
    let mut coords = [0u8; 3];
    let (u_axis, v_axis) = plane_axes(axis);
    coords[axis as usize] = slice as u8;
    coords[u_axis] = u as u8;
    coords[v_axis] = v as u8;
    coords
}

fn plane_axes(axis: u8) -> (usize, usize) {
    match axis {
        0 => (1, 2),
        1 => (0, 2),
        2 => (0, 1),
        _ => unreachable!("axis must be 0..=2"),
    }
}

fn sample_with_halo(
    chunk: &ChunkVoxels,
    neighbors: &ChunkNeighborSet<'_>,
    x: i32,
    y: i32,
    z: i32,
) -> u8 {
    // Interior: all coords in [0, 64)
    if (0..64).contains(&x) && (0..64).contains(&y) && (0..64).contains(&z) {
        return chunk.block(x as u8, y as u8, z as u8);
    }

    // Single-axis halo lookups (only ±1 on one axis while the other two remain in [0, 64)).
    // Edge/corner halo (multiple axes out-of-range) returns air.

    // X halo
    if x == -1 && (0..64).contains(&y) && (0..64).contains(&z) {
        return neighbors
            .nx
            .map_or(0, |neighbor| neighbor.block(63, y as u8, z as u8));
    }
    if x == 64 && (0..64).contains(&y) && (0..64).contains(&z) {
        return neighbors
            .px
            .map_or(0, |neighbor| neighbor.block(0, y as u8, z as u8));
    }

    // Y halo
    if y == -1 && (0..64).contains(&x) && (0..64).contains(&z) {
        return neighbors
            .ny
            .map_or(0, |neighbor| neighbor.block(x as u8, 63, z as u8));
    }
    if y == 64 && (0..64).contains(&x) && (0..64).contains(&z) {
        return neighbors
            .py
            .map_or(0, |neighbor| neighbor.block(x as u8, 0, z as u8));
    }

    // Z halo
    if z == -1 && (0..64).contains(&x) && (0..64).contains(&y) {
        return neighbors
            .nz
            .map_or(0, |neighbor| neighbor.block(x as u8, y as u8, 63));
    }
    if z == 64 && (0..64).contains(&x) && (0..64).contains(&y) {
        return neighbors
            .pz
            .map_or(0, |neighbor| neighbor.block(x as u8, y as u8, 0));
    }

    // Edge/corner halo or wildly out-of-range → air
    0
}

fn quad_bounds(quad: &GreedyQuad) -> ([f32; 3], [f32; 3]) {
    let axis = quad.axis as usize;
    let (u_axis, v_axis) = plane_axes(quad.axis);
    let mut min = [0.0; 3];
    let mut max = [0.0; 3];
    for i in 0..3 {
        min[i] = quad.origin[i] as f32;
        max[i] = quad.origin[i] as f32;
    }
    min[u_axis] = quad.origin[u_axis] as f32;
    max[u_axis] = quad.origin[u_axis] as f32 + f32::from(quad.size[0]);
    min[v_axis] = quad.origin[v_axis] as f32;
    max[v_axis] = quad.origin[v_axis] as f32 + f32::from(quad.size[1]);

    let plane = quad.origin[axis] as f32 + if quad.positive_face { 1.0 } else { 0.0 };
    min[axis] = plane;
    max[axis] = plane;
    if quad.is_skirt {
        if quad.positive_face {
            max[axis] = plane + 1.0;
        } else {
            min[axis] = plane - 1.0;
        }
    }

    (min, max)
}
