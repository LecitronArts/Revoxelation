use super::GreedyQuad;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedVertex(pub [u32; 2]);

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
        | (u32::from(local_xyz[1]) << 6)
        | (u32::from(local_xyz[2]) << 12)
        | (u32::from(face) << 18);
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

    for uv in corners {
        let mut vertex = pack_vertex(quad.origin, face, quad.block_id, uv);
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
