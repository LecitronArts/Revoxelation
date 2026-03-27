use anyhow::{Result, anyhow};
use ash::vk;
use gpu_allocator::{MemoryLocation, vulkan::{Allocation, AllocationScheme}};

use super::Renderer;
use super::helpers::{create_allocated_buffer, destroy_allocated_buffer};

/// A sub-allocation within the staging ring buffer.
pub struct StagingAllocation {
    /// The Vulkan buffer handle (same for all allocations from this ring).
    pub buffer: vk::Buffer,
    /// Byte offset within the staging ring buffer.
    pub offset: vk::DeviceSize,
    /// Size of this allocation in bytes.
    pub size: vk::DeviceSize,
    /// Mapped pointer to the start of this allocation (null if layout-only).
    mapped_ptr: *mut u8,
}

// SAFETY: StagingAllocation's `mapped_ptr` (*mut u8) points into gpu-allocator CpuToGpu mapped
// memory. Send is safe because: (1) write_bytes requires &mut self (exclusive access), (2) each
// allocation is a unique non-overlapping sub-range, (3) the StagingRing outlives all allocations.
unsafe impl Send for StagingAllocation {}

impl StagingAllocation {
    /// Write data into this staging allocation.
    pub fn write_bytes(&mut self, data: &[u8]) {
        assert!(
            (data.len() as u64) <= self.size,
            "write exceeds staging allocation size: {} > {}",
            data.len(),
            self.size,
        );
        if !self.mapped_ptr.is_null() {
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), self.mapped_ptr, data.len());
            }
        }
    }
}

/// Ring-buffer staging allocator for per-frame GPU uploads.
///
/// A single large `CpuToGpu` buffer is divided into N regions (one per in-flight frame).
/// Each frame uses its own region exclusively. Fence-based reclamation (handled externally
/// by `submit_frame` waiting on the in-flight fence) ensures a region is not overwritten
/// while the GPU is still reading from it.
pub struct StagingRing {
    buffer: vk::Buffer,
    allocation: Option<Allocation>,
    mapped_base: *mut u8,
    total_size: u64,
    frame_size: u64,
    frame_count: usize,
    current_frame: usize,
    /// Byte cursor within the current frame's region (relative to region start).
    cursor: u64,
}

// SAFETY: StagingRing's `mapped_base` (*mut u8) points to gpu-allocator CpuToGpu mapped memory.
// Send is safe because: (1) writes go through allocate(&mut self) → StagingAllocation::write_bytes,
// (2) per-frame regions are isolated by fence waits, (3) only accessed from the main render thread.
unsafe impl Send for StagingRing {}

impl StagingRing {
    /// Create a real StagingRing backed by a Vulkan CpuToGpu buffer.
    pub fn new(renderer: &mut Renderer, total_size: u64, frame_count: usize) -> Result<Self> {
        assert!(frame_count > 0, "frame_count must be > 0");
        let frame_size = total_size / frame_count as u64;
        let (buffer, allocation) = create_allocated_buffer(
            renderer,
            total_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "staging-ring",
        )?;
        let mapped_base = allocation
            .mapped_ptr()
            .map(|p| p.as_ptr() as *mut u8)
            .unwrap_or(std::ptr::null_mut());

        Ok(Self {
            buffer,
            allocation: Some(allocation),
            mapped_base,
            total_size,
            frame_size,
            frame_count,
            current_frame: 0,
            cursor: 0,
        })
    }

    /// Create a StagingRing for testing allocation logic without GPU resources.
    #[doc(hidden)]
    pub fn new_layout_only(total_size: u64, frame_count: usize) -> Self {
        assert!(frame_count > 0, "frame_count must be > 0");
        Self {
            buffer: vk::Buffer::null(),
            allocation: None,
            mapped_base: std::ptr::null_mut(),
            total_size,
            frame_size: total_size / frame_count as u64,
            frame_count,
            current_frame: 0,
            cursor: 0,
        }
    }

    /// Allocate `size` bytes with the given alignment from the current frame's region.
    ///
    /// Returns a `StagingAllocation` with the buffer, absolute offset, and a mapped pointer
    /// for writing data.
    pub fn allocate(&mut self, size: u64, alignment: u64) -> Result<StagingAllocation> {
        // Align cursor up
        let aligned_cursor = if alignment > 0 {
            (self.cursor + alignment - 1) & !(alignment - 1)
        } else {
            self.cursor
        };

        if aligned_cursor + size > self.frame_size {
            return Err(anyhow!(
                "staging ring frame region exhausted: need {} bytes at cursor {}, frame_size={}",
                size,
                aligned_cursor,
                self.frame_size,
            ));
        }

        let frame_base = self.current_frame as u64 * self.frame_size;
        let absolute_offset = frame_base + aligned_cursor;

        let mapped_ptr = if !self.mapped_base.is_null() {
            unsafe { self.mapped_base.add(absolute_offset as usize) }
        } else {
            std::ptr::null_mut()
        };

        self.cursor = aligned_cursor + size;

        Ok(StagingAllocation {
            buffer: self.buffer,
            offset: absolute_offset,
            size,
            mapped_ptr,
        })
    }

    /// Advance to the next frame's staging region.
    ///
    /// This resets the allocation cursor. The caller must ensure the previous use
    /// of the target frame's region is complete (via fence wait).
    pub fn advance_frame(&mut self) {
        self.current_frame = (self.current_frame + 1) % self.frame_count;
        self.cursor = 0;
    }

    /// Reset the cursor for the current frame (e.g. at frame start after fence wait).
    pub fn reset_current_frame(&mut self) {
        self.cursor = 0;
    }

    /// Return the underlying Vulkan buffer handle.
    pub fn buffer(&self) -> vk::Buffer {
        self.buffer
    }

    /// Destroy the staging ring, freeing GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        if let Some(allocation) = self.allocation.take() {
            destroy_allocated_buffer(renderer, self.buffer, allocation)?;
        }
        Ok(())
    }
}
