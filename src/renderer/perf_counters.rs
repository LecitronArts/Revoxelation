/// GPU performance counters collected per-frame for HUD display.
///
/// `visible_chunks` and `total_chunks` are read back from the GPU draw count
/// buffer (previous frame's result to avoid stalling).  `frame_time_ms` is
/// measured CPU-side with `std::time::Instant`.  `gpu_time_ms` is reserved for
/// future timestamp-query support.
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuPerfCounters {
    /// Number of chunks that passed culling and are drawn.
    pub visible_chunks: u32,
    /// Total number of chunks in the active set (before culling).
    pub total_chunks: u32,
    /// CPU-measured frame time in milliseconds.
    pub frame_time_ms: f32,
    /// GPU-measured frame time in milliseconds (0.0 if unavailable).
    pub gpu_time_ms: f32,
    /// Current chunk pool capacity (dynamic, grows by 2x).
    pub chunk_capacity: u32,
    // -- Meshlet statistics (Phase 6) --
    /// Total meshlets across all active chunks.
    pub total_meshlets: u32,
    /// Meshlets that passed culling and are drawn.
    pub visible_meshlets: u32,
    /// Meshlet cull rate: (total - visible) / total, in [0.0, 1.0].
    pub meshlet_cull_rate: f32,
    /// Total bytes used by meshlet SSBOs (meta + vertex + tri).
    pub meshlet_ssbo_bytes: u64,
    // -- LOD statistics (MSHL-05) --
    /// Number of LOD0 (original) meshlets in the active set.
    pub lod0_meshlets: u32,
    /// Number of LOD1 (simplified) meshlets in the active set.
    pub lod1_meshlets: u32,
    /// Current SSE threshold for LOD selection (pixels).
    pub sse_threshold: f32,
}
