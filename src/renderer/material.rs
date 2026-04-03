//! Block material system with per-face texture indices and PBR properties.
//!
//! Each block type has a `BlockMaterial` (32 bytes) specifying which texture
//! array layer to use for the top (+Y), side (±X/±Z), and bottom (-Y) faces,
//! plus PBR texture indices (metallic-roughness, normal map, emissive).
//! `MaterialTable` wraps the full array indexed by block_id.

use anyhow::Result;
use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocation;

use super::Renderer;
use super::helpers::{create_allocated_buffer, submit_one_shot_commands};

/// Per-block material data uploaded to the material SSBO (binding 8).
///
/// Layout (D-01, expanded LGHT-01): 16 × u16 = 32 bytes total.
/// - `top_texture`: texture array layer for +Y faces
/// - `side_texture`: texture array layer for ±X/±Z faces
/// - `bottom_texture`: texture array layer for -Y faces
/// - `flags`: bit flags (0x01 = emissive, 0x02 = transparent, 0x04 = has_mr,
///            0x08 = has_normal, 0x10 = has_emissive_map, 0x20 = is_32x32)
/// - PBR texture indices (0xFFFF = no texture, use shader defaults)
/// - `emissive_intensity`: fixed-point 8.8 emissive strength
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BlockMaterial {
    pub top_texture: u16,
    pub side_texture: u16,
    pub bottom_texture: u16,
    pub flags: u16,
    // PBR texture indices (0xFFFF = no texture, use defaults)
    pub top_mr: u16,             // metallic-roughness texture layer (top face)
    pub side_mr: u16,            // metallic-roughness texture layer (side faces)
    pub bottom_mr: u16,          // metallic-roughness texture layer (bottom face)
    pub top_normal: u16,         // normal map texture layer (top face)
    pub side_normal: u16,        // normal map texture layer (side faces)
    pub bottom_normal: u16,      // normal map texture layer (bottom face)
    pub top_emissive: u16,       // emissive texture layer (top face)
    pub side_emissive: u16,      // emissive texture layer (side faces)
    pub bottom_emissive: u16,    // emissive texture layer (bottom face)
    pub emissive_intensity: u16, // emissive strength (fixed-point 8.8)
    pub _pad0: u16,
    pub _pad1: u16,
}

// Compile-time size assertion: BlockMaterial must be exactly 32 bytes.
const _: () = assert!(std::mem::size_of::<BlockMaterial>() == 32);

/// Flag bit: block emits light.
pub const FLAG_EMISSIVE: u16 = 0x01;
/// Flag bit: block is transparent / alpha-tested.
pub const FLAG_TRANSPARENT: u16 = 0x02;
/// Flag bit: block has metallic-roughness texture.
#[allow(dead_code)]
pub const FLAG_HAS_MR: u16 = 0x04;
/// Flag bit: block has normal map texture.
#[allow(dead_code)]
pub const FLAG_HAS_NORMAL: u16 = 0x08;
/// Flag bit: block has emissive map texture.
#[allow(dead_code)]
pub const FLAG_HAS_EMISSIVE_MAP: u16 = 0x10;
/// Flag bit: PBR maps (MR/normal/emissive) sample from 32x32 texture arrays.
#[allow(dead_code)]
pub const FLAG_IS_32X32: u16 = 0x20;

/// No PBR texture assigned — shader uses defaults.
const NO_TEX: u16 = 0xFFFF;

pub const BLOCK_ID_LEAVES: u8 = 7;
pub const BLOCK_ID_WATER: u8 = 8;
pub const BLOCK_ID_LAMP: u8 = 9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmissiveBlockInfo {
    pub color: [f32; 3],
    pub intensity: f32,
    pub radius: f32,
}

pub fn is_transparent_block(block_id: u8) -> bool {
    matches!(block_id, BLOCK_ID_LEAVES | BLOCK_ID_WATER)
}

pub fn face_visible_against(current_block: u8, neighbor_block: u8) -> bool {
    if current_block == 0 {
        return false;
    }
    if neighbor_block == 0 {
        return true;
    }

    let current_transparent = is_transparent_block(current_block);
    let neighbor_transparent = is_transparent_block(neighbor_block);

    if current_transparent {
        neighbor_transparent && current_block != neighbor_block
    } else {
        neighbor_transparent
    }
}

pub fn emissive_block_info(block_id: u8) -> Option<EmissiveBlockInfo> {
    match block_id {
        BLOCK_ID_LAMP => Some(EmissiveBlockInfo {
            color: [1.0, 0.85, 0.35],
            intensity: 6.0,
            radius: 1.25,
        }),
        _ => None,
    }
}

/// Collection of `BlockMaterial` entries indexed by `block_id`.
pub struct MaterialTable {
    entries: Vec<BlockMaterial>,
}

impl MaterialTable {
    /// Build the default table with 9 block types + air (D-03).
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
    ///  11 = lamp
    ///
    /// All existing blocks get 0xFFFF for PBR texture indices (use shader defaults:
    /// metallic=0.0, roughness=0.8, flat normal, no emissive).
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
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 2: Grass — top: grass_top (2), side: grass_side (3), bottom: dirt (1)
            BlockMaterial {
                top_texture: 2,
                side_texture: 3,
                bottom_texture: 1,
                flags: 0,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 3: Stone — all faces: stone (layer 4)
            BlockMaterial {
                top_texture: 4,
                side_texture: 4,
                bottom_texture: 4,
                flags: 0,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 4: Sand — all faces: sand (layer 5)
            BlockMaterial {
                top_texture: 5,
                side_texture: 5,
                bottom_texture: 5,
                flags: 0,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 5: Log — top/bottom: log_end (7), side: log_bark (6)
            BlockMaterial {
                top_texture: 7,
                side_texture: 6,
                bottom_texture: 7,
                flags: 0,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 6: Planks — all faces: planks (layer 8)
            BlockMaterial {
                top_texture: 8,
                side_texture: 8,
                bottom_texture: 8,
                flags: 0,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 7: Leaves — all faces: leaves (layer 9), transparent
            BlockMaterial {
                top_texture: 9,
                side_texture: 9,
                bottom_texture: 9,
                flags: FLAG_TRANSPARENT,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 8: Water — all faces: water (layer 10), transparent
            BlockMaterial {
                top_texture: 10,
                side_texture: 10,
                bottom_texture: 10,
                flags: FLAG_TRANSPARENT,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 0,
                _pad0: 0,
                _pad1: 0,
            },
            // 9: Lamp — all faces: lamp (layer 11), emissive
            BlockMaterial {
                top_texture: 11,
                side_texture: 11,
                bottom_texture: 11,
                flags: FLAG_EMISSIVE,
                top_mr: NO_TEX,
                side_mr: NO_TEX,
                bottom_mr: NO_TEX,
                top_normal: NO_TEX,
                side_normal: NO_TEX,
                bottom_normal: NO_TEX,
                top_emissive: NO_TEX,
                side_emissive: NO_TEX,
                bottom_emissive: NO_TEX,
                emissive_intensity: 2 << 8,
                _pad0: 0,
                _pad1: 0,
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
    pub fn upload(&self, renderer: &mut Renderer) -> Result<(vk::Buffer, Allocation)> {
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
            bindless.register_buffer(
                &renderer.device_ctx.device,
                super::bindless::BINDING_MATERIAL,
                buffer,
                size,
            );
        }

        Ok((buffer, alloc))
    }
}
