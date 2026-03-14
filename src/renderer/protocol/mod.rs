pub mod bindings;
pub mod types;

pub use types::{
    CameraGpu, ChunkMapEntryGpu, ChunkMetaGpu, EmissiveVoxelGpu, GiReservoirGpu, ReservoirGpu,
    SurfaceSampleGpu, SvgfUniform, TracerUniform,
};

pub const fn history_read_slot_from_frame(frame_index: u32) -> u32 {
    frame_index & 1
}

pub const fn history_write_slot_from_frame(frame_index: u32) -> u32 {
    (frame_index + 1) & 1
}

pub const fn encode_history_flags(history_read_slot: u32, history_write_slot: u32) -> u32 {
    (history_read_slot & 1) | ((history_write_slot & 1) << 1)
}

#[cfg(test)]
pub const fn decode_history_read_slot(flags_word: u32) -> u32 {
    flags_word & 1
}

#[cfg(test)]
pub const fn decode_history_write_slot(flags_word: u32) -> u32 {
    (flags_word >> 1) & 1
}

pub const fn svgf_atrous_source_slot(history_write_slot: u32, pass_index: u32) -> u32 {
    (history_write_slot + pass_index) & 1
}

pub const fn svgf_resolve_source_slot(history_write_slot: u32, svgf_pass_count: u32) -> u32 {
    (history_write_slot + svgf_pass_count) & 1
}

#[cfg(test)]
mod tests {
    use super::types::{MotionVectorGpu, PackedVoxel};
    use super::*;
    use std::mem::{align_of, size_of};

    #[test]
    fn camera_gpu_layout_matches_protocol() {
        assert_eq!(size_of::<CameraGpu>(), 96);
        assert_eq!(align_of::<CameraGpu>(), 16);
    }

    #[test]
    fn tracer_uniform_layout_matches_protocol() {
        assert_eq!(size_of::<TracerUniform>(), 208);
        assert_eq!(align_of::<TracerUniform>(), 16);
    }

    #[test]
    fn svgf_uniform_layout_matches_protocol() {
        assert_eq!(size_of::<SvgfUniform>(), 48);
        assert_eq!(align_of::<SvgfUniform>(), 16);
    }

    #[test]
    fn chunk_and_emissive_layout_matches_protocol() {
        assert_eq!(size_of::<ChunkMetaGpu>(), 32);
        assert_eq!(align_of::<ChunkMetaGpu>(), 16);
        assert_eq!(size_of::<ChunkMapEntryGpu>(), 32);
        assert_eq!(align_of::<ChunkMapEntryGpu>(), 16);
        assert_eq!(size_of::<EmissiveVoxelGpu>(), 16);
        assert_eq!(align_of::<EmissiveVoxelGpu>(), 16);
    }

    #[test]
    fn reservoir_and_surface_layout_matches_protocol() {
        assert_eq!(size_of::<ReservoirGpu>(), 16);
        assert_eq!(align_of::<ReservoirGpu>(), 16);
        assert_eq!(size_of::<GiReservoirGpu>(), 48);
        assert_eq!(align_of::<GiReservoirGpu>(), 16);
        assert_eq!(size_of::<SurfaceSampleGpu>(), 16);
        assert_eq!(align_of::<SurfaceSampleGpu>(), 16);
        assert_eq!(size_of::<MotionVectorGpu>(), 16);
        assert_eq!(align_of::<MotionVectorGpu>(), 16);
    }

    #[test]
    fn packed_voxel_pack_unpack_roundtrip() {
        let voxel = PackedVoxel::new(42, 7, 0xBEEF);
        assert_eq!(voxel.material_or_color(), 42);
        assert_eq!(voxel.emissive(), 7);
        assert_eq!(voxel.payload(), 0xBEEF);
    }

    #[test]
    fn packed_voxel_boundary_values_roundtrip() {
        let min = PackedVoxel::new(0, 0, 0);
        assert_eq!(min.material_or_color(), 0);
        assert_eq!(min.emissive(), 0);
        assert_eq!(min.payload(), 0);

        let max = PackedVoxel::new(u8::MAX, u8::MAX, u16::MAX);
        assert_eq!(max.material_or_color(), u8::MAX);
        assert_eq!(max.emissive(), u8::MAX);
        assert_eq!(max.payload(), u16::MAX);
    }

    #[test]
    fn history_slot_and_flag_encoding_matches_shader_contract() {
        assert_eq!(history_read_slot_from_frame(0), 0);
        assert_eq!(history_write_slot_from_frame(0), 1);
        assert_eq!(history_read_slot_from_frame(1), 1);
        assert_eq!(history_write_slot_from_frame(1), 0);

        let flags_even = encode_history_flags(
            history_read_slot_from_frame(0),
            history_write_slot_from_frame(0),
        );
        assert_eq!(flags_even, 2);
        assert_eq!(decode_history_read_slot(flags_even), 0);
        assert_eq!(decode_history_write_slot(flags_even), 1);

        let flags_odd = encode_history_flags(
            history_read_slot_from_frame(1),
            history_write_slot_from_frame(1),
        );
        assert_eq!(flags_odd, 1);
        assert_eq!(decode_history_read_slot(flags_odd), 1);
        assert_eq!(decode_history_write_slot(flags_odd), 0);

        assert_eq!(svgf_atrous_source_slot(0, 0), 0);
        assert_eq!(svgf_atrous_source_slot(0, 1), 1);
        assert_eq!(svgf_atrous_source_slot(1, 2), 1);
        assert_eq!(svgf_resolve_source_slot(0, 3), 1);
        assert_eq!(svgf_resolve_source_slot(1, 4), 1);
    }

    #[test]
    fn history_flag_encoding_masks_non_binary_inputs() {
        let flags = encode_history_flags(6, 9);
        assert_eq!(flags, 2);
        assert_eq!(decode_history_read_slot(flags), 0);
        assert_eq!(decode_history_write_slot(flags), 1);

        let flags = encode_history_flags(5, 4);
        assert_eq!(flags, 1);
        assert_eq!(decode_history_read_slot(flags), 1);
        assert_eq!(decode_history_write_slot(flags), 0);
    }

    #[test]
    fn history_flag_decoding_ignores_unrelated_bits() {
        let flags_word = 0b1011_0101u32;
        assert_eq!(decode_history_read_slot(flags_word), 1);
        assert_eq!(decode_history_write_slot(flags_word), 0);
    }

    #[test]
    fn svgf_slot_helpers_wrap_large_indices() {
        assert_eq!(svgf_atrous_source_slot(1, 65), 0);
        assert_eq!(svgf_atrous_source_slot(0, 130), 0);
        assert_eq!(svgf_resolve_source_slot(1, 255), 0);
        assert_eq!(svgf_resolve_source_slot(0, 254), 0);
    }
}
