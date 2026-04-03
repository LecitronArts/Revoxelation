use anyhow::{Context, Result};
use ash::vk;
use gpu_allocator::{MemoryLocation, vulkan::Allocation, vulkan::AllocationScheme};

use super::Renderer;
use super::helpers::{create_allocated_buffer, destroy_allocated_buffer};

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

/// Double-buffered GPU readback for meshlet visible count (POLISH-06).
///
/// Each in-flight frame has its own u32 slot in a CPU-readable buffer.
/// At the end of each frame, a `vkCmdCopyBuffer` copies the GPU-side
/// meshlet count into this frame's slot. After the next fence wait,
/// we read the *previous* frame's slot to get the count without stalling.
pub struct GpuReadbackCounters {
    buffer: vk::Buffer,
    allocation: Option<Allocation>,
    mapped_ptr: *const u32,
    frame_count: usize,
}

// SAFETY: mapped_ptr points to gpu-allocator GpuToCpu mapped memory.
// Only read after fence wait (previous frame), written by GPU via copy command.
unsafe impl Send for GpuReadbackCounters {}

impl GpuReadbackCounters {
    /// Allocate a CPU-readable buffer with one u32 per in-flight frame.
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let frame_count = renderer.frames.len();
        let size = (frame_count * std::mem::size_of::<u32>()) as u64;

        let (buffer, allocation) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuToCpu,
            AllocationScheme::GpuAllocatorManaged,
            "readback-counters",
        )?;

        let mapped_ptr = allocation
            .mapped_ptr()
            .context("readback buffer not mapped")?
            .as_ptr() as *const u32;

        Ok(Self {
            buffer,
            allocation: Some(allocation),
            mapped_ptr,
            frame_count,
        })
    }

    /// Read the visible meshlet count from the previous frame's readback slot.
    pub fn read_previous_frame(&self, current_frame: usize) -> u32 {
        let prev = if current_frame == 0 {
            self.frame_count - 1
        } else {
            current_frame - 1
        };
        unsafe { *self.mapped_ptr.add(prev) }
    }

    /// Record a GPU copy from the meshlet count buffer into this frame's readback slot.
    pub fn record_copy(
        &self,
        device: &ash::Device,
        command_buffer: vk::CommandBuffer,
        current_frame: usize,
        src_buffer: vk::Buffer,
    ) {
        let dst_offset = (current_frame * std::mem::size_of::<u32>()) as u64;
        let region = vk::BufferCopy {
            src_offset: 0,
            dst_offset,
            size: std::mem::size_of::<u32>() as u64,
        };
        unsafe {
            device.cmd_copy_buffer(command_buffer, src_buffer, self.buffer, &[region]);
        }
    }

    /// Destroy GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        if let Some(alloc) = self.allocation.take() {
            destroy_allocated_buffer(renderer, self.buffer, alloc)?;
        }
        Ok(())
    }
}
