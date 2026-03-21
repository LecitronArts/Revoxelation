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
