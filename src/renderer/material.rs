//! Block material system with per-face texture indices.
//!
//! Each block type has a `BlockMaterial` (8 bytes) specifying which texture
//! array layer to use for the top (+Y), side (±X/±Z), and bottom (-Y) faces.
//! `MaterialTable` wraps the full array indexed by block_id.

use anyhow::Result;
use ash::vk;
use gpu_allocator::vulkan::Allocation;
use gpu_allocator::MemoryLocation;

use super::Renderer;
use super::helpers::{create_allocated_buffer, submit_one_shot_commands};

/// Per-block material data uploaded to the material SSBO (binding 8).
///
/// Layout (D-01): 4 × u16 = 8 bytes total.
/// - `top_texture`: texture array layer for +Y faces
/// - `side_texture`: texture array layer for ±X/±Z faces
/// - `bottom_texture`: texture array layer for -Y faces
/// - `flags`: bit flags (0x01 = emissive, 0x02 = transparent, reserved)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlockMaterial {
    pub top_texture: u16,
    pub side_texture: u16,
    pub bottom_texture: u16,
    pub flags: u16,
}

/// Flag bit: block emits light.
#[allow(dead_code)]
pub const FLAG_EMISSIVE: u16 = 0x01;
/// Flag bit: block is transparent / alpha-tested.
pub const FLAG_TRANSPARENT: u16 = 0x02;

/// Collection of `BlockMaterial` entries indexed by `block_id`.
pub struct MaterialTable {
    entries: Vec<BlockMaterial>,
}

impl MaterialTable {
    /// Build the default table with 8 block types + air (D-03).
    ///
    /// Texture layer assignments (must match `TextureArray` generation order):
    ///   0 = (unused / air placeholder)
    ///   1 = dirt
    ///   2 = grass_top
    ///   3 = grass_side
    ///   4 = stone
    ///   5 = sand
    ///   6 = log_bark
    ///   7 = log_end
    ///   8 = planks
    ///   9 = leaves
    ///  10 = water
    pub fn default_table() -> Self {
        let entries = vec![
            // 0: Air — never rendered
            BlockMaterial::default(),
            // 1: Dirt — all faces: dirt (layer 1)
            BlockMaterial {
                top_texture: 1,
                side_texture: 1,
                bottom_texture: 1,
                flags: 0,
            },
            // 2: Grass — top: grass_top (2), side: grass_side (3), bottom: dirt (1)
            BlockMaterial {
                top_texture: 2,
                side_texture: 3,
                bottom_texture: 1,
                flags: 0,
            },
            // 3: Stone — all faces: stone (layer 4)
            BlockMaterial {
                top_texture: 4,
                side_texture: 4,
                bottom_texture: 4,
                flags: 0,
            },
            // 4: Sand — all faces: sand (layer 5)
            BlockMaterial {
                top_texture: 5,
                side_texture: 5,
                bottom_texture: 5,
                flags: 0,
            },
            // 5: Log — top/bottom: log_end (7), side: log_bark (6)
            BlockMaterial {
                top_texture: 7,
                side_texture: 6,
                bottom_texture: 7,
                flags: 0,
            },
            // 6: Planks — all faces: planks (layer 8)
            BlockMaterial {
                top_texture: 8,
                side_texture: 8,
                bottom_texture: 8,
                flags: 0,
            },
            // 7: Leaves — all faces: leaves (layer 9), transparent
            BlockMaterial {
                top_texture: 9,
                side_texture: 9,
                bottom_texture: 9,
                flags: FLAG_TRANSPARENT,
            },
            // 8: Water — all faces: water (layer 10), transparent
            BlockMaterial {
                top_texture: 10,
                side_texture: 10,
                bottom_texture: 10,
                flags: FLAG_TRANSPARENT,
            },
        ];

        Self { entries }
    }

    /// Access the underlying entries slice.
    pub fn entries(&self) -> &[BlockMaterial] {
        &self.entries
    }

    /// Return the raw bytes for GPU upload (SSBO contents).
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.entries)
    }

    /// Create a GpuOnly SSBO from this table, upload via staging, and register
    /// at bindless binding 8. Returns the buffer + allocation for cleanup.
    pub fn upload(
        &self,
        renderer: &mut Renderer,
    ) -> Result<(vk::Buffer, Allocation)> {
        let data = self.as_bytes();
        let size = data.len() as u64;

        // Create GpuOnly SSBO
        let (buffer, alloc) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::TRANSFER_DST,
            MemoryLocation::GpuOnly,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "material-ssbo",
        )?;

        // Create staging buffer with material data
        let (staging_buf, staging_alloc) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "material-staging",
        )?;

        // Copy data to staging
        if let Some(mapped) = staging_alloc.mapped_ptr() {
            let ptr = mapped.as_ptr() as *mut u8;
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
            }
        }

        // Record copy command
        let dst_buffer = buffer;
        submit_one_shot_commands(renderer, |device, cmd| {
            let region = vk::BufferCopy::default()
                .src_offset(0)
                .dst_offset(0)
                .size(size);
            unsafe {
                device.cmd_copy_buffer(cmd, staging_buf, dst_buffer, &[region]);
            }
            Ok(())
        })?;

        // Clean up staging
        super::helpers::destroy_allocated_buffer(renderer, staging_buf, staging_alloc)?;

        // Register at bindless binding 8
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_buffer(&renderer.device_ctx.device, super::bindless::BINDING_MATERIAL, buffer, size);
        }

        Ok((buffer, alloc))
    }
}
