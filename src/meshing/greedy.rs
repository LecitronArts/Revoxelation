use super::{ChunkNeighborSet, ChunkVoxels, GreedyQuad, MeshDirtyRecord, PackedMesh, pack_quad};
use crate::renderer::material::{face_visible_against, is_transparent_block};

// -----------------------------------------------------------------------
// Voxel Ambient Occlusion (LGHT-04)
// -----------------------------------------------------------------------

/// Check if a block at the given position is opaque for AO purposes.
/// Air (block_id == 0) is non-occluding. Out-of-chunk positions are checked
/// via neighbor chunks; if no neighbor data is available, treat as non-occluding.
fn is_opaque_for_ao(chunk: &ChunkVoxels, neighbors: &ChunkNeighborSet<'_>, pos: [i32; 3]) -> bool {
    let block = sample_with_halo(chunk, neighbors, pos[0], pos[1], pos[2]);
    block != 0 && !is_transparent_block(block)
}

/// Compute voxel AO for a single vertex corner on a face.
///
/// For a face on axis `a` at position face_pos, the two tangent axes are u,v.
/// For a corner at (du, dv) where du/dv are -1 or +1 offsets along u,v:
///   side1 = block at face_pos offset by du along u-axis
///   side2 = block at face_pos offset by dv along v-axis
///   corner = block at face_pos offset by du along u-axis AND dv along v-axis
///
/// Returns AO level: 0 (fully occluded/dark) to 3 (fully open/bright).
fn compute_corner_ao(
    chunk: &ChunkVoxels,
    neighbors: &ChunkNeighborSet<'_>,
    face_pos: [i32; 3],
    axis: u8,
    du: i32, // -1 or +1 along u-tangent
    dv: i32, // -1 or +1 along v-tangent
) -> u8 {
    let (u_axis, v_axis) = plane_axes(axis);

    let mut s1_pos = face_pos;
    s1_pos[u_axis] += du;

    let mut s2_pos = face_pos;
    s2_pos[v_axis] += dv;

    let mut c_pos = face_pos;
    c_pos[u_axis] += du;
    c_pos[v_axis] += dv;

    let side1 = is_opaque_for_ao(chunk, neighbors, s1_pos);
    let side2 = is_opaque_for_ao(chunk, neighbors, s2_pos);
    let corner = is_opaque_for_ao(chunk, neighbors, c_pos);

    if side1 && side2 {
        0 // fully occluded — corner is hidden behind two solid blocks
    } else {
        3 - (side1 as u8 + side2 as u8 + corner as u8)
    }
}

/// Compute AO values for all 4 corners of a greedy quad.
///
/// The face position is the block face exposed to air. For each corner vertex,
/// we determine the du/dv offsets (+1 or -1) along the tangent axes and
/// sample the 3 AO neighbors (side1, side2, diagonal).
///
/// For greedy-merged quads (size > 1), we sample AO at the actual corner
/// positions of the merged quad rather than individual voxels, which gives
/// visually correct results.
fn compute_quad_ao(
    chunk: &ChunkVoxels,
    neighbors: &ChunkNeighborSet<'_>,
    quad: &GreedyQuad,
) -> [u8; 4] {
    let (u_axis, v_axis) = plane_axes(quad.axis);
    let axis_idx = quad.axis as usize;

    // The face is on the surface of the block. For positive faces, the face
    // is at origin[axis] + 1; for negative faces, at origin[axis].
    // The AO sampling position is one step into the air side of the face.
    let mut face_base = [
        quad.origin[0] as i32,
        quad.origin[1] as i32,
        quad.origin[2] as i32,
    ];
    if quad.positive_face {
        face_base[axis_idx] += 1;
    } else {
        face_base[axis_idx] -= 1;
    }

    // Corner order matches pack_quad corners:
    //   0: (origin_u, origin_v)         → du=-1, dv=-1
    //   1: (origin_u + size_u, origin_v) → du=+1, dv=-1
    //   2: (origin_u + size_u, origin_v + size_v) → du=+1, dv=+1
    //   3: (origin_u, origin_v + size_v) → du=-1, dv=+1
    //
    // For each corner, the face_pos is at the corner's actual world position.
    let corners: [(i32, i32, i32, i32); 4] = [
        (0, 0, -1, -1),
        (quad.size[0] as i32, 0, 1, -1),
        (quad.size[0] as i32, quad.size[1] as i32, 1, 1),
        (0, quad.size[1] as i32, -1, 1),
    ];

    let mut ao = [3u8; 4];
    for (i, &(u_off, v_off, du, dv)) in corners.iter().enumerate() {
        let mut face_pos = face_base;
        face_pos[u_axis] = quad.origin[u_axis] as i32 + u_off;
        face_pos[v_axis] = quad.origin[v_axis] as i32 + v_off;
        ao[i] = compute_corner_ao(chunk, neighbors, face_pos, quad.axis, du, dv);
    }
    ao
}

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

    // Border skirt emission disabled (MSHL-05): meshlet LOD DAG handles LOD transitions
    // via alpha dithering instead of geometry skirts. Commented skirt code removed (REFAC-07).
    let _ = &dirty; // suppress unused warning

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
        let ao = compute_quad_ao(chunk, neighbors, quad);
        pack_quad(quad, ao, &mut vertices, &mut indices);
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

                if face_visible_against(current_block, neighbor_block) {
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
