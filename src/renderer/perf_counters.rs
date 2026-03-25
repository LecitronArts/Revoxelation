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
}
