use bytemuck::{Pod, Zeroable};

/// Packed voxel layout (u32):
/// - bits 00..07: material id / albedo index
/// - bits 08..15: emissive intensity
/// - bits 16..31: user payload / flags
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq, Eq)]
pub struct PackedVoxel(pub u32);

impl PackedVoxel {
    pub const fn new(material_or_color: u8, emissive: u8, payload: u16) -> Self {
        Self((material_or_color as u32) | ((emissive as u32) << 8) | ((payload as u32) << 16))
    }

    pub const fn material_or_color(self) -> u8 {
        (self.0 & 0xff) as u8
    }

    pub const fn emissive(self) -> u8 {
        ((self.0 >> 8) & 0xff) as u8
    }

    pub const fn payload(self) -> u16 {
        (self.0 >> 16) as u16
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct CameraGpu {
    pub position_lens: [f32; 4],
    pub forward_fov: [f32; 4],
    pub right_aspect: [f32; 4],
    pub up_focus: [f32; 4],
    pub clip_depth: [f32; 4],
    pub resolution_frame: [u32; 4],
}

impl Default for CameraGpu {
    fn default() -> Self {
        Self {
            position_lens: [0.0, 0.0, 0.0, 0.0],
            forward_fov: [0.0, 0.0, 1.0, 1.047_197_6],
            right_aspect: [1.0, 0.0, 0.0, 1.0],
            up_focus: [0.0, 1.0, 0.0, 1.0],
            clip_depth: [0.01, 1000.0, 0.2, 0.0],
            resolution_frame: [1, 1, 0, 0],
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct TracerUniform {
    pub resolution_frame_chunks: [u32; 4],
    pub chunk_map_info: [u32; 4],
    pub emissive_info: [u32; 4],
    pub importance_info: [u32; 4],
    pub debug_map_stats: [f32; 4],
    pub world_min: [i32; 4],
    pub world_max: [i32; 4],
    pub integrator: [f32; 4],
    pub sun_dir: [f32; 4],
    pub tuning_a: [f32; 4],
    pub tuning_b: [f32; 4],
    pub tuning_c: [f32; 4],
    pub flags: [u32; 4],
}

impl Default for TracerUniform {
    fn default() -> Self {
        Self {
            resolution_frame_chunks: [1, 1, 0, 1],
            chunk_map_info: [1, 0, 1, 0],
            emissive_info: [0, 0, 0, 0],
            importance_info: [1, 1, 1, 0],
            debug_map_stats: [0.0, 0.0, 0.0, 0.0],
            world_min: [-64, -64, -64, 0],
            world_max: [64, 64, 64, 0],
            integrator: [4.0, 8.0, 1.2, 1.0],
            sun_dir: [0.35, 0.8, 0.2, 0.0],
            tuning_a: [24.0, 3.0, 0.1, 0.95],
            tuning_b: [1.0, 1.0, 512.0, 0.2],
            tuning_c: [8.0, 24.0, 0.25, 3.0],
            flags: [1, 1, 0, 0],
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SvgfUniform {
    pub resolution_step: [u32; 4],
    pub params: [f32; 4],
    pub extras: [f32; 4],
}

impl Default for SvgfUniform {
    fn default() -> Self {
        Self {
            resolution_step: [1, 1, 1, 0],
            params: [1.5, 96.0, 2.0, 2.25],
            extras: [3.5, 4.0, 0.0, 0.0],
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ChunkMetaGpu {
    pub coord_size: [i32; 4],
    pub voxel_offset: u32,
    pub voxel_count: u32,
    pub _pad: [u32; 2],
}

impl ChunkMetaGpu {
    pub const fn empty() -> Self {
        Self {
            coord_size: [0, 0, 0, 32],
            voxel_offset: 0,
            voxel_count: 1,
            _pad: [0; 2],
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct ChunkMapEntryGpu {
    pub key_value: [i32; 4],
    pub meta: [u32; 4],
}

impl ChunkMapEntryGpu {
    pub const fn empty() -> Self {
        Self {
            key_value: [i32::MIN, i32::MIN, i32::MIN, 0],
            meta: [0; 4],
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct EmissiveVoxelGpu {
    pub position_power: [f32; 4],
}

impl EmissiveVoxelGpu {
    pub const fn empty() -> Self {
        Self {
            position_power: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct DiReservoirGpu {
    pub z_i: u32,
    pub w_sum: f32,
    pub m_i: f32,
    pub w_var: f32,
}

impl DiReservoirGpu {
    pub const fn empty() -> Self {
        Self {
            z_i: 0x00ff_ffff,
            w_sum: 0.0,
            m_i: 0.0,
            w_var: 0.0,
        }
    }
}

pub type ReservoirGpu = DiReservoirGpu;

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct GiReservoirGpu {
    pub head: [u32; 4],
    pub accum: [f32; 4],
    pub sample: [f32; 4],
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SurfaceSampleGpu {
    pub normal_material: [f32; 4],
}

impl SurfaceSampleGpu {
    pub const fn empty() -> Self {
        Self {
            normal_material: [0.0, 0.0, 0.0, -1.0],
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct MotionVectorGpu {
    pub velocity_depth: [f32; 4],
}
