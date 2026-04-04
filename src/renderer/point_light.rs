//! Point light system for emissive blocks (LGHT-01).
//!
//! `PointLightManager` owns a double-buffered SSBO at binding 22 containing
//! up to MAX_VISIBLE_POINT_LIGHTS point lights sorted by distance to camera.

use std::cmp::Ordering;
use std::collections::HashMap;

use anyhow::Result;
use ash::vk;
use glam::Vec3;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocation;

use super::Renderer;
use super::bindless::BINDING_POINT_LIGHT_SSBO;
use super::helpers::create_allocated_buffer;
use super::material::emissive_block_info;
use crate::streaming::types::{CHUNK_EDGE, ChunkKey, ChunkVoxels};

/// GPU-side point light data (matches GLSL PointLight struct).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLight {
    pub position: [f32; 3],
    pub radius: f32,
    pub color: [f32; 3],
    pub intensity: f32,
}

/// Header for the point light SSBO (binding 22).
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PointLightHeader {
    pub count: u32,
    pub max_lights: u32,
    pub _pad: [u32; 2],
}

/// Maximum number of visible point lights uploaded per frame.
pub const MAX_VISIBLE_POINT_LIGHTS: usize = 64;
const BLOCK_SIZE: f32 = crate::renderer::coords::BLOCK_SIZE;
const MAX_POINT_LIGHT_DISTANCE: f32 = 24.0;

/// SSBO size: header + max lights.
const SSBO_SIZE: u64 = (std::mem::size_of::<PointLightHeader>()
    + MAX_VISIBLE_POINT_LIGHTS * std::mem::size_of::<PointLight>()) as u64;

/// Manages point lights from emissive blocks.
pub struct PointLightManager {
    pub ssbo_buffers: [vk::Buffer; 2],
    pub ssbo_allocs: [Option<Allocation>; 2],
    pub visible_lights: Vec<PointLight>,
}

impl PointLightManager {
    /// Create PointLightManager with double-buffered SSBOs at binding 22.
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let (buf0, alloc0) = create_allocated_buffer(
            renderer,
            SSBO_SIZE,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "point-light-ssbo-0",
        )?;
        let (buf1, alloc1) = create_allocated_buffer(
            renderer,
            SSBO_SIZE,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "point-light-ssbo-1",
        )?;

        // Write empty header to both buffers.
        let empty_header = PointLightHeader {
            count: 0,
            max_lights: MAX_VISIBLE_POINT_LIGHTS as u32,
            _pad: [0; 2],
        };
        let header_bytes = bytemuck::bytes_of(&empty_header);
        for alloc in [&alloc0, &alloc1] {
            if let Some(mapped) = alloc.mapped_ptr() {
                let ptr = mapped.as_ptr() as *mut u8;
                unsafe {
                    std::ptr::copy_nonoverlapping(header_bytes.as_ptr(), ptr, header_bytes.len());
                }
            }
        }

        // Register frame 0's buffer initially.
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_buffer(
                &renderer.device_ctx.device,
                BINDING_POINT_LIGHT_SSBO,
                buf0,
                SSBO_SIZE,
            );
        }

        Ok(Self {
            ssbo_buffers: [buf0, buf1],
            ssbo_allocs: [Some(alloc0), Some(alloc1)],
            visible_lights: Vec::with_capacity(MAX_VISIBLE_POINT_LIGHTS),
        })
    }

    pub fn rebuild_from_payloads(
        &mut self,
        payloads: &HashMap<ChunkKey, ChunkVoxels>,
        camera_pos: [f32; 3],
    ) {
        self.visible_lights.clear();

        let camera = Vec3::from_array(camera_pos);
        let max_dist_sq = MAX_POINT_LIGHT_DISTANCE * MAX_POINT_LIGHT_DISTANCE;
        let mut candidates = Vec::new();

        for (key, chunk) in payloads {
            if key.lod_level != 0 {
                continue;
            }

            let block_world_size = BLOCK_SIZE * (1_u32 << key.lod_level) as f32;
            let chunk_world_edge = CHUNK_EDGE as f32 * block_world_size;
            let chunk_origin = Vec3::new(
                key.x as f32 * chunk_world_edge,
                key.y as f32 * chunk_world_edge,
                key.z as f32 * chunk_world_edge,
            );
            let chunk_center = chunk_origin + Vec3::splat(chunk_world_edge * 0.5);
            if chunk_center.distance_squared(camera)
                > (MAX_POINT_LIGHT_DISTANCE + chunk_world_edge).powi(2)
            {
                continue;
            }

            for z in 0..CHUNK_EDGE as u8 {
                for y in 0..CHUNK_EDGE as u8 {
                    for x in 0..CHUNK_EDGE as u8 {
                        let block_id = chunk.block(x, y, z);
                        let Some(info) = emissive_block_info(block_id) else {
                            continue;
                        };

                        let position = chunk_origin
                            + Vec3::new(
                                (x as f32 + 0.5) * block_world_size,
                                (y as f32 + 0.5) * block_world_size,
                                (z as f32 + 0.5) * block_world_size,
                            );
                        let dist_sq = position.distance_squared(camera);
                        if dist_sq > max_dist_sq {
                            continue;
                        }

                        candidates.push((
                            dist_sq,
                            PointLight {
                                position: position.to_array(),
                                radius: info.radius,
                                color: info.color,
                                intensity: info.intensity,
                            },
                        ));
                    }
                }
            }
        }

        candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
        self.visible_lights.extend(
            candidates
                .into_iter()
                .take(MAX_VISIBLE_POINT_LIGHTS)
                .map(|(_, light)| light),
        );
    }

    /// Upload current visible lights to the current frame's SSBO.
    pub fn upload(&self, renderer: &Renderer, current_frame: usize) {
        let count = self.visible_lights.len().min(MAX_VISIBLE_POINT_LIGHTS);
        let header = PointLightHeader {
            count: count as u32,
            max_lights: MAX_VISIBLE_POINT_LIGHTS as u32,
            _pad: [0; 2],
        };

        let alloc = &self.ssbo_allocs[current_frame];
        if let Some(alloc) = alloc {
            if let Some(mapped) = alloc.mapped_ptr() {
                let ptr = mapped.as_ptr() as *mut u8;
                let header_bytes = bytemuck::bytes_of(&header);
                unsafe {
                    // Write header
                    std::ptr::copy_nonoverlapping(header_bytes.as_ptr(), ptr, header_bytes.len());
                    // Write light data after header
                    if count > 0 {
                        let lights_ptr = ptr.add(std::mem::size_of::<PointLightHeader>());
                        let lights_data: &[u8] =
                            bytemuck::cast_slice(&self.visible_lights[..count]);
                        std::ptr::copy_nonoverlapping(
                            lights_data.as_ptr(),
                            lights_ptr,
                            lights_data.len(),
                        );
                    }
                }
            }
        }

        // Register current frame's buffer at binding 22.
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_buffer(
                &renderer.device_ctx.device,
                BINDING_POINT_LIGHT_SSBO,
                self.ssbo_buffers[current_frame],
                SSBO_SIZE,
            );
        }
    }

    /// Clean up GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        for i in 0..2 {
            if let Some(alloc) = self.ssbo_allocs[i].take() {
                super::helpers::destroy_allocated_buffer(renderer, self.ssbo_buffers[i], alloc)?;
            }
        }
        Ok(())
    }
}
