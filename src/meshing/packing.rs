use std::cmp::Ordering;

use super::GreedyQuad;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedVertex(pub [u32; 2]);

/// Per-meshlet metadata: offsets into the global vertex/triangle buffers,
/// counts, bounding sphere, orientation cone, and LOD info (D-03, MSHL-05).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshletDescriptor {
    pub vertex_offset: u32,
    pub triangle_offset: u32,
    pub vertex_count: u32,
    pub triangle_count: u32,
    /// Bounding sphere center (local-space).
    pub center: [f32; 3],
    /// Bounding sphere radius.
    pub radius: f32,
    /// Orientation cone axis (normalized).
    pub cone_axis: [f32; 3],
    /// Orientation cone cutoff (cos(half-angle)).
    pub cone_cutoff: f32,
    /// LOD level: 0 = original, 1 = simplified parent (MSHL-05).
    pub lod_level: u8,
    /// LOD group ID — links LOD0 children and LOD1 parent meshlets (MSHL-05).
    pub group_id: u32,
    /// Simplification error metric for LOD selection (MSHL-05).
    /// For LOD0: error of the parent (LOD1) relative to this level.
    /// For LOD1: same group error (used by GPU for LOD transition).
    /// f32::MAX means no LOD parent exists (always render this level).
    pub parent_error: f32,
}

/// Meshlet-split mesh output, replacing PackedMesh as the pipeline payload (D-02).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshletMesh {
    /// Per-meshlet descriptors with offsets, counts, bounds.
    pub meshlets: Vec<MeshletDescriptor>,
    /// Re-indexed vertex data in meshlet-local order.
    pub vertices: Vec<PackedVertex>,
    /// Local triangle indices (3 bytes per triangle, meshoptimizer output format).
    pub triangles: Vec<u8>,
    /// Chunk-level AABB min (retained from PackedMesh).
    pub aabb_min: [f32; 3],
    /// Chunk-level AABB max (retained from PackedMesh).
    pub aabb_max: [f32; 3],
}

impl MeshletMesh {
    /// Reconstruct a flat global index list from meshlet-local u8 indices.
    ///
    /// Each meshlet's local triangle indices (0..vertex_count) are remapped to
    /// global vertex indices relative to `self.vertices`.  Used by ChunkPool's
    /// legacy VB/IB path until meshlet rendering is fully wired.
    pub fn flat_indices(&self) -> Vec<u32> {
        let mut indices = Vec::with_capacity(self.triangles.len());
        for m in &self.meshlets {
            let t_off = m.triangle_offset as usize;
            let t_count = m.triangle_count as usize;
            let v_off = m.vertex_offset;
            for &local_idx in &self.triangles[t_off..t_off + t_count * 3] {
                indices.push(v_off + u32::from(local_idx));
            }
        }
        indices
    }

    /// Reconstruct a `PackedMesh` for legacy ChunkPool compatibility.
    pub fn to_packed_mesh(&self) -> PackedMesh {
        let indices = self.flat_indices();
        let quad_count = (indices.len() / 6) as u32;
        PackedMesh {
            vertices: self.vertices.clone().into_boxed_slice(),
            indices: indices.into_boxed_slice(),
            quad_count,
            aabb_min: self.aabb_min,
            aabb_max: self.aabb_max,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackedMesh {
    pub vertices: Box<[PackedVertex]>,
    pub indices: Box<[u32]>,
    pub quad_count: u32,
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
}

/// Pack a single vertex into 8 bytes (uvec2).
///
/// Layout of word0: x(7) | y(7) | z(7) | face(3) | ao(2) | unused(6)
/// Layout of word1: block_id(16) | u(8) | v(8)
pub fn pack_vertex(
    local_xyz: [u8; 3],
    face: u8,
    block_id: u16,
    uv_local: [u8; 2],
    ao: u8,
) -> PackedVertex {
    let word0 = u32::from(local_xyz[0])
        | (u32::from(local_xyz[1]) << 7)
        | (u32::from(local_xyz[2]) << 14)
        | (u32::from(face) << 21)
        | (u32::from(ao & 0x3) << 24); // LGHT-04: 2 bits AO (0=dark, 3=bright)
    let word1 =
        u32::from(block_id) | (u32::from(uv_local[0]) << 16) | (u32::from(uv_local[1]) << 24);

    PackedVertex([word0, word1])
}

/// Pack a quad into vertices and indices.
///
/// `ao` contains per-corner AO values (0=fully occluded/dark, 3=fully open/bright)
/// in order: [corner00, corner_u0, corner_uv, corner_0v].
///
/// When opposite-corner AO sums differ, the quad diagonal is flipped to produce
/// correct AO interpolation (Minecraft-style fix for interpolation anisotropy).
pub fn pack_quad(
    quad: &GreedyQuad,
    ao: [u8; 4],
    vertices: &mut Vec<PackedVertex>,
    indices: &mut Vec<u32>,
) {
    let base_index = vertices.len() as u32;
    let face = face_index(quad.axis, quad.positive_face);
    let corners = [
        [0u8, 0u8],
        [quad.size[0], 0u8],
        [quad.size[0], quad.size[1]],
        [0u8, quad.size[1]],
    ];

    let (u_axis, v_axis) = plane_axes(quad.axis);
    for (i, &[du, dv]) in corners.iter().enumerate() {
        let mut pos = quad.origin;
        pos[u_axis] = quad.origin[u_axis] + du;
        pos[v_axis] = quad.origin[v_axis] + dv;
        // For side faces, texture V must map to the world-Y direction so that
        // "top of texture" = "top of block" (e.g. grass side green strip).
        //   X faces (axis 0): u_axis=Y, v_axis=Z → swap UV so V = du (Y).
        //   Z faces (axis 2): u_axis=X, v_axis=Y → V = dv (Y), already correct.
        //   Y faces (axis 1): top/bottom, UV orientation doesn't matter for Y-based lookup.
        let uv = if quad.axis == 0 { [dv, du] } else { [du, dv] };
        let vertex = pack_vertex(pos, face, quad.block_id, uv, ao[i]);
        vertices.push(vertex);
    }

    // LGHT-04: Flip quad diagonal when ao[0]+ao[2] < ao[1]+ao[3] to fix
    // AO interpolation anisotropy (Minecraft-style diagonal flip).
    let diag_a = u16::from(ao[0]) + u16::from(ao[2]);
    let diag_b = u16::from(ao[1]) + u16::from(ao[3]);
    if diag_a < diag_b {
        // Flipped diagonal: triangles (0,1,3) and (1,2,3)
        indices.extend_from_slice(&[
            base_index,
            base_index + 1,
            base_index + 3,
            base_index + 1,
            base_index + 2,
            base_index + 3,
        ]);
    } else {
        // Default diagonal: triangles (0,1,2) and (0,2,3)
        indices.extend_from_slice(&[
            base_index,
            base_index + 1,
            base_index + 2,
            base_index,
            base_index + 2,
            base_index + 3,
        ]);
    }
}

fn face_index(axis: u8, positive_face: bool) -> u8 {
    match (axis, positive_face) {
        (0, true) => 0,
        (0, false) => 1,
        (1, true) => 2,
        (1, false) => 3,
        (2, true) => 4,
        (2, false) => 5,
        _ => 0,
    }
}

fn plane_axes(axis: u8) -> (usize, usize) {
    match axis {
        0 => (1, 2), // X face: u=Y, v=Z
        1 => (0, 2), // Y face: u=X, v=Z
        2 => (0, 1), // Z face: u=X, v=Y
        _ => unreachable!("axis must be 0..=2"),
    }
}

// ---------------------------------------------------------------------------
// Meshlet generation via meshoptimizer (MSHL-01)
// ---------------------------------------------------------------------------

/// Unpack 7-bit x/y/z from PackedVertex word0 to temporary f32 positions.
fn unpack_position(v: &PackedVertex) -> [f32; 3] {
    let word0 = v.0[0];
    let x = (word0 & 0x7F) as f32;
    let y = ((word0 >> 7) & 0x7F) as f32;
    let z = ((word0 >> 14) & 0x7F) as f32;
    [x, y, z]
}

/// Split a `PackedMesh` into meshlets using meshoptimizer (D-11).
///
/// Unpacks 7-bit x/y/z to temporary f32 positions for spatial clustering,
/// calls `meshopt::build_meshlets` with max_vertices=64, max_triangles=124,
/// cone_weight=0.5, then computes bounding spheres and orientation cones
/// per meshlet via `meshopt::compute_meshlet_bounds`.
///
/// The output `MeshletMesh` retains `PackedVertex` data re-indexed in
/// meshlet-local order, with triangle indices stored as u8 (meshoptimizer
/// output format). AABB is copied from the input `PackedMesh`.
pub fn build_meshlets_from_packed(packed: &PackedMesh) -> MeshletMesh {
    // Degenerate case: no geometry.
    if packed.vertices.is_empty() || packed.indices.is_empty() {
        return MeshletMesh {
            meshlets: Vec::new(),
            vertices: Vec::new(),
            triangles: Vec::new(),
            aabb_min: packed.aabb_min,
            aabb_max: packed.aabb_max,
        };
    }

    // Build float position array for meshoptimizer (3 floats per vertex, tightly packed).
    let vertex_count = packed.vertices.len();
    let mut positions: Vec<f32> = Vec::with_capacity(vertex_count * 3);
    for v in packed.vertices.iter() {
        let [x, y, z] = unpack_position(v);
        positions.push(x);
        positions.push(y);
        positions.push(z);
    }

    // Build a VertexDataAdapter over the f32 position data.
    let pos_bytes: &[u8] = bytemuck::cast_slice(&positions);
    let vertex_stride = std::mem::size_of::<f32>() * 3; // 12 bytes per vertex
    let vertices_adapter = meshopt::VertexDataAdapter::new(pos_bytes, vertex_stride, 0)
        .expect("VertexDataAdapter construction should not fail");

    // Call meshoptimizer to split into meshlets.
    let meshlets = meshopt::build_meshlets(
        &packed.indices,
        &vertices_adapter,
        64,  // max_vertices
        124, // max_triangles
        0.5, // cone_weight (D-10)
    );

    // Build output MeshletMesh.
    let mut out_meshlets = Vec::with_capacity(meshlets.len());
    let mut out_vertices = Vec::new();
    let mut out_triangles = Vec::new();

    for (idx, m) in meshlets.iter().enumerate() {
        let raw = &meshlets.meshlets[idx];

        // Compute bounding sphere + orientation cone using the Meshlet view.
        let bounds = meshopt::compute_meshlet_bounds(m, &vertices_adapter);

        // Record output offsets.
        let out_vertex_offset = out_vertices.len() as u32;
        let out_triangle_offset = out_triangles.len() as u32;

        let v_count = raw.vertex_count as usize;
        let t_count = raw.triangle_count as usize;

        // Copy re-indexed PackedVertex data.
        for &global_vertex_index in m.vertices {
            out_vertices.push(packed.vertices[global_vertex_index as usize]);
        }

        // Copy triangle indices as u8 (meshoptimizer output format).
        out_triangles.extend_from_slice(m.triangles);

        out_meshlets.push(MeshletDescriptor {
            vertex_offset: out_vertex_offset,
            triangle_offset: out_triangle_offset,
            vertex_count: v_count as u32,
            triangle_count: t_count as u32,
            center: bounds.center,
            radius: bounds.radius,
            cone_axis: bounds.cone_axis,
            cone_cutoff: bounds.cone_cutoff,
            lod_level: 0,
            group_id: 0,
            parent_error: f32::MAX,
        });
    }

    // -----------------------------------------------------------------------
    // LOD DAG: generate LOD1 simplified meshlets from groups of ~4 LOD0 meshlets
    // (MSHL-05, D-01 through D-04).
    // -----------------------------------------------------------------------
    let lod0_count = out_meshlets.len();

    // Small meshes (<4 meshlets) get LOD0 only — no simplification worthwhile.
    if lod0_count >= 4 {
        build_lod1(&mut out_meshlets, &mut out_vertices, &mut out_triangles);
    }

    MeshletMesh {
        meshlets: out_meshlets,
        vertices: out_vertices,
        triangles: out_triangles,
        aabb_min: packed.aabb_min,
        aabb_max: packed.aabb_max,
    }
}

// ---------------------------------------------------------------------------
// LOD1 DAG generation helpers (MSHL-05)
// ---------------------------------------------------------------------------

/// Group LOD0 meshlets by spatial proximity. Returns vec of (group_id, member_indices).
fn group_meshlets_spatially(
    meshlets: &[MeshletDescriptor],
    group_size: usize,
) -> Vec<(u32, Vec<usize>)> {
    let mut indices: Vec<usize> = (0..meshlets.len()).collect();
    indices.sort_by(|&a, &b| {
        let ca = meshlets[a].center;
        let cb = meshlets[b].center;
        ca[0]
            .partial_cmp(&cb[0])
            .unwrap_or(Ordering::Equal)
            .then(ca[1].partial_cmp(&cb[1]).unwrap_or(Ordering::Equal))
            .then(ca[2].partial_cmp(&cb[2]).unwrap_or(Ordering::Equal))
    });

    indices
        .chunks(group_size)
        .enumerate()
        .map(|(gid, chunk)| (gid as u32, chunk.to_vec()))
        .collect()
}

/// Build LOD1 meshlets from groups of LOD0 meshlets.
///
/// For each group of ~4 LOD0 meshlets:
/// 1. Merge triangles into a unified index/vertex space.
/// 2. Identify boundary vertices (shared with other groups) and lock them.
/// 3. Simplify to ~4× fewer triangles via meshopt::simplify.
/// 4. Re-split into LOD1 meshlets via build_meshlets.
/// 5. Record parent_error on LOD0 meshlets.
fn build_lod1(
    meshlets: &mut Vec<MeshletDescriptor>,
    out_vertices: &mut Vec<PackedVertex>,
    out_triangles: &mut Vec<u8>,
) {
    let lod0_count = meshlets.len();
    let groups = group_meshlets_spatially(&meshlets[..lod0_count], 4);

    // Assign group_id to each LOD0 meshlet.
    for (group_id, members) in &groups {
        for &mi in members {
            meshlets[mi].group_id = *group_id;
        }
    }

    // Build a mapping of output-vertex-index → set of group_ids that use it,
    // to detect boundary vertices (vertices shared between groups).
    let mut vertex_to_groups: std::collections::HashMap<u32, Vec<u32>> =
        std::collections::HashMap::new();
    for (group_id, members) in &groups {
        for &mi in members {
            let m = &meshlets[mi];
            let v_off = m.vertex_offset as usize;
            let t_off = m.triangle_offset as usize;
            let t_count = m.triangle_count as usize;
            for &local_idx in &out_triangles[t_off..t_off + t_count * 3] {
                let global_idx = v_off as u32 + u32::from(local_idx);
                vertex_to_groups
                    .entry(global_idx)
                    .or_default()
                    .push(*group_id);
            }
        }
    }
    for groups_list in vertex_to_groups.values_mut() {
        groups_list.sort_unstable();
        groups_list.dedup();
    }

    // Process each group: merge → simplify → re-split → append LOD1 meshlets.
    for (group_id, members) in &groups {
        if members.len() < 2 {
            // Single-meshlet group: skip simplification.
            continue;
        }

        // Merge: remap all group triangles into a shared local vertex space.
        let mut merged_indices: Vec<u32> = Vec::new();
        let mut global_to_local: std::collections::HashMap<u32, u32> =
            std::collections::HashMap::new();
        let mut merged_positions: Vec<f32> = Vec::new();
        let mut merged_packed: Vec<PackedVertex> = Vec::new();

        for &mi in members {
            let m = &meshlets[mi];
            let v_off = m.vertex_offset as usize;
            let t_off = m.triangle_offset as usize;
            let t_count = m.triangle_count as usize;

            for &local_idx in &out_triangles[t_off..t_off + t_count * 3] {
                let global_idx = v_off as u32 + u32::from(local_idx);
                let local = *global_to_local.entry(global_idx).or_insert_with(|| {
                    let idx = merged_packed.len() as u32;
                    let pv = out_vertices[global_idx as usize];
                    let [x, y, z] = unpack_position(&pv);
                    merged_positions.push(x);
                    merged_positions.push(y);
                    merged_positions.push(z);
                    merged_packed.push(pv);
                    idx
                });
                merged_indices.push(local);
            }
        }

        let merged_vertex_count = merged_packed.len();
        // Target: ~4× reduction in index count.
        let target_index_count = (merged_indices.len() / 4).max(3);

        // Identify boundary vertices: those used by multiple groups.
        let mut vertex_lock = vec![false; merged_vertex_count];
        for (&global_idx, &local_idx) in &global_to_local {
            if let Some(glist) = vertex_to_groups.get(&global_idx)
                && glist.len() > 1
            {
                vertex_lock[local_idx as usize] = true;
            }
        }

        // Build VertexDataAdapter for meshopt.
        let pos_bytes: &[u8] = bytemuck::cast_slice(&merged_positions);
        let vertex_stride = std::mem::size_of::<f32>() * 3;
        let adapter = match meshopt::VertexDataAdapter::new(pos_bytes, vertex_stride, 0) {
            Ok(a) => a,
            Err(_) => continue,
        };

        // Simplify with locked boundary vertices.
        let simplified_indices = meshopt::simplify_with_locks(
            &merged_indices,
            &adapter,
            &vertex_lock,
            target_index_count,
            0.1,
            meshopt::SimplifyOptions::None,
        );

        // Skip if simplification was not effective (< 10% reduction) or empty.
        if simplified_indices.is_empty()
            || simplified_indices.len() >= merged_indices.len() * 9 / 10
        {
            continue;
        }

        // Compute simplification error for LOD selection on the GPU.
        let mut simplify_error: f32 = 0.0;
        let _ = meshopt::simplify(
            &merged_indices,
            &adapter,
            target_index_count,
            0.1,
            meshopt::SimplifyOptions::None,
            Some(&mut simplify_error),
        );
        let scale = meshopt::simplify_scale(&adapter);
        let world_error = simplify_error * scale;
        let parent_error = if world_error.is_finite() && world_error >= 0.0 {
            world_error
        } else {
            0.001
        };

        // Write parent_error back to all LOD0 meshlets in this group.
        for &mi in members {
            meshlets[mi].parent_error = parent_error;
        }

        // Re-split simplified geometry into LOD1 meshlets.
        let simplified_meshlets =
            meshopt::build_meshlets(&simplified_indices, &adapter, 64, 124, 0.5);

        // Append LOD1 meshlets' vertices and triangles to the output buffers.
        for (idx, sm) in simplified_meshlets.iter().enumerate() {
            let raw = &simplified_meshlets.meshlets[idx];
            let bounds = meshopt::compute_meshlet_bounds(sm, &adapter);

            let v_offset = out_vertices.len() as u32;
            let t_offset = out_triangles.len() as u32;
            let v_count = raw.vertex_count as u32;
            let t_count = raw.triangle_count as u32;

            // sm.vertices are indices into merged_packed.
            for &vi in sm.vertices {
                out_vertices.push(merged_packed[vi as usize]);
            }
            out_triangles.extend_from_slice(sm.triangles);

            meshlets.push(MeshletDescriptor {
                vertex_offset: v_offset,
                triangle_offset: t_offset,
                vertex_count: v_count,
                triangle_count: t_count,
                center: bounds.center,
                radius: bounds.radius,
                cone_axis: bounds.cone_axis,
                cone_cutoff: bounds.cone_cutoff,
                lod_level: 1,
                group_id: *group_id,
                parent_error,
            });
        }
    }
}
