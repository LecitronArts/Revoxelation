use super::GreedyQuad;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedVertex(pub [u32; 2]);

/// Per-meshlet metadata: offsets into the global vertex/triangle buffers,
/// counts, bounding sphere, and orientation cone (D-03).
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

pub fn pack_vertex(local_xyz: [u8; 3], face: u8, block_id: u16, uv_local: [u8; 2]) -> PackedVertex {
    let word0 = u32::from(local_xyz[0])
        | (u32::from(local_xyz[1]) << 7)
        | (u32::from(local_xyz[2]) << 14)
        | (u32::from(face) << 21);
    let word1 = u32::from(block_id)
        | (u32::from(uv_local[0]) << 16)
        | (u32::from(uv_local[1]) << 24);

    PackedVertex([word0, word1])
}

pub fn pack_quad(quad: &GreedyQuad, vertices: &mut Vec<PackedVertex>, indices: &mut Vec<u32>) {
    let base_index = vertices.len() as u32;
    let face = face_index(quad.axis, quad.positive_face);
    let corners = [
        [0u8, 0u8],
        [quad.size[0], 0u8],
        [quad.size[0], quad.size[1]],
        [0u8, quad.size[1]],
    ];

    let (u_axis, v_axis) = plane_axes(quad.axis);
    for &[du, dv] in &corners {
        let mut pos = quad.origin;
        pos[u_axis] = quad.origin[u_axis] + du;
        pos[v_axis] = quad.origin[v_axis] + dv;
        // For side faces, texture V must map to the world-Y direction so that
        // "top of texture" = "top of block" (e.g. grass side green strip).
        //   X faces (axis 0): u_axis=Y, v_axis=Z → swap UV so V = du (Y).
        //   Z faces (axis 2): u_axis=X, v_axis=Y → V = dv (Y), already correct.
        //   Y faces (axis 1): top/bottom, UV orientation doesn't matter for Y-based lookup.
        let uv = if quad.axis == 0 {
            [dv, du]
        } else {
            [du, dv]
        };
        let mut vertex = pack_vertex(pos, face, quad.block_id, uv);
        if quad.is_skirt {
            vertex.0[0] |= 1 << 24;
        }
        vertices.push(vertex);
    }

    indices.extend_from_slice(&[
        base_index,
        base_index + 1,
        base_index + 2,
        base_index,
        base_index + 2,
        base_index + 3,
    ]);
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
        });
    }

    MeshletMesh {
        meshlets: out_meshlets,
        vertices: out_vertices,
        triangles: out_triangles,
        aabb_min: packed.aabb_min,
        aabb_max: packed.aabb_max,
    }
}
