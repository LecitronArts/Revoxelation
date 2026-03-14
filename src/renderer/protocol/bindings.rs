pub mod trace {
    pub const OUTPUT_VIEW: u32 = 0;
    pub const ACCUMULATION: u32 = 1;
    pub const CAMERA: u32 = 2;
    pub const TRACER_UNIFORM: u32 = 3;
    pub const VOXELS: u32 = 4;
    pub const CHUNK_META: u32 = 5;
    pub const CHUNK_MAP: u32 = 6;
    pub const EMISSIVE_VOXELS: u32 = 7;
    pub const DI_RESERVOIR_A: u32 = 8;
    pub const DI_RESERVOIR_B: u32 = 9;
    pub const PREVIOUS_CAMERA: u32 = 10;
    pub const SURFACE_HISTORY: u32 = 11;
    pub const EMISSIVE_CDF: u32 = 12;
    pub const EMISSIVE_REMAP: u32 = 13;
    pub const GI_RESERVOIR_A: u32 = 14;
    pub const GI_RESERVOIR_B: u32 = 15;
    pub const IMPORTANCE_MAP: u32 = 16;
    pub const COUNT: usize = 17;

    #[cfg(test)]
    pub const ORDER: [u32; COUNT] = [
        OUTPUT_VIEW,
        ACCUMULATION,
        CAMERA,
        TRACER_UNIFORM,
        VOXELS,
        CHUNK_META,
        CHUNK_MAP,
        EMISSIVE_VOXELS,
        DI_RESERVOIR_A,
        DI_RESERVOIR_B,
        PREVIOUS_CAMERA,
        SURFACE_HISTORY,
        EMISSIVE_CDF,
        EMISSIVE_REMAP,
        GI_RESERVOIR_A,
        GI_RESERVOIR_B,
        IMPORTANCE_MAP,
    ];
}

pub mod svgf {
    pub const OUTPUT_VIEW: u32 = 0;
    pub const ACCUMULATION: u32 = 1;
    pub const TRACER_UNIFORM: u32 = 2;
    pub const SURFACE_HISTORY: u32 = 3;
    pub const UNIFORM: u32 = 4;
    pub const PING: u32 = 5;
    pub const PONG: u32 = 6;
    pub const CAMERA: u32 = 7;
    pub const PREVIOUS_CAMERA: u32 = 8;
    pub const DEBUG_DATA: u32 = 9;
    pub const COUNT: usize = 10;

    #[cfg(test)]
    pub const ORDER: [u32; COUNT] = [
        OUTPUT_VIEW,
        ACCUMULATION,
        TRACER_UNIFORM,
        SURFACE_HISTORY,
        UNIFORM,
        PING,
        PONG,
        CAMERA,
        PREVIOUS_CAMERA,
        DEBUG_DATA,
    ];
}

#[cfg(test)]
mod tests {
    use super::{svgf, trace};

    #[test]
    fn trace_binding_order_is_contiguous_and_starts_at_zero() {
        let expected = (0..trace::COUNT as u32).collect::<Vec<_>>();
        assert_eq!(trace::ORDER.to_vec(), expected);
    }

    #[test]
    fn svgf_binding_order_is_contiguous_and_starts_at_zero() {
        let expected = (0..svgf::COUNT as u32).collect::<Vec<_>>();
        assert_eq!(svgf::ORDER.to_vec(), expected);
    }

    #[test]
    fn trace_binding_count_matches_last_index_plus_one() {
        assert_eq!(
            trace::COUNT,
            trace::ORDER.last().copied().unwrap_or_default() as usize + 1
        );
    }

    #[test]
    fn svgf_binding_count_matches_last_index_plus_one() {
        assert_eq!(
            svgf::COUNT,
            svgf::ORDER.last().copied().unwrap_or_default() as usize + 1
        );
    }

    #[test]
    fn trace_binding_order_has_no_duplicates() {
        let mut sorted = trace::ORDER.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), trace::COUNT);
    }

    #[test]
    fn svgf_binding_order_has_no_duplicates() {
        let mut sorted = svgf::ORDER.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), svgf::COUNT);
    }
}
