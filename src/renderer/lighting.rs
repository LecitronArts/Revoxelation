//! Directional lighting state and GPU parameter management (LGHT-01).
//!
//! `LightingState` owns a double-buffered SSBO for `LightingParams` at binding 18.
//! Each frame, the CPU computes sun direction from time_of_day and uploads to the
//! current frame's buffer before any draw commands.

use anyhow::Result;
use ash::vk;
use gpu_allocator::vulkan::Allocation;
use gpu_allocator::MemoryLocation;

use super::Renderer;
use super::helpers::create_allocated_buffer;
use super::bindless::BINDING_LIGHTING_UBO;

/// GPU-side lighting parameters (uploaded to binding 18 SSBO).
///
/// Layout matches the GLSL `LightingParams` struct in common.glsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightingParams {
    pub sun_direction: [f32; 3],    // normalized world-space
    pub sun_intensity: f32,
    pub sun_color: [f32; 3],        // linear RGB
    pub ambient_intensity: f32,
    pub ambient_color: [f32; 3],    // linear RGB
    pub time_of_day: f32,           // 0.0-1.0 (0=midnight, 0.25=sunrise, 0.5=noon, 0.75=sunset)
    // CSM shadow matrices (4 cascades) — filled by Plan 07-02
    pub shadow_matrices: [[f32; 16]; 4], // mat4 x 4
    pub cascade_splits: [f32; 4],
    // Fog params — filled by Plan 07-05
    pub fog_color: [f32; 3],
    pub fog_density: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_type: u32,              // 0=linear, 1=exp, 2=exp2, 3=height
    pub _pad: u32,
}

/// CPU-side lighting control state.
pub struct LightingState {
    /// Double-buffered SSBOs for LightingParams (one per in-flight frame).
    pub ssbo_buffers: [vk::Buffer; 2],
    pub ssbo_allocs: [Option<Allocation>; 2],
    /// Sun elevation angle in degrees (0-90).
    pub sun_elevation: f32,
    /// Sun azimuth angle in degrees (0-360).
    pub sun_azimuth: f32,
    /// Sun intensity multiplier.
    pub sun_intensity: f32,
    /// Sun color (linear RGB).
    pub sun_color: [f32; 3],
    /// Ambient intensity multiplier.
    pub ambient_intensity: f32,
    /// Ambient color (linear RGB).
    pub ambient_color: [f32; 3],
    /// Time of day (0.0-1.0).
    pub time_of_day: f32,
}

impl LightingState {
    /// Create LightingState with double-buffered SSBOs registered at binding 18.
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let size = std::mem::size_of::<LightingParams>() as u64;

        let (buf0, alloc0) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "lighting-ssbo-0",
        )?;
        let (buf1, alloc1) = create_allocated_buffer(
            renderer,
            size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            gpu_allocator::vulkan::AllocationScheme::GpuAllocatorManaged,
            "lighting-ssbo-1",
        )?;

        // Register frame 0's buffer initially.
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_buffer(
                &renderer.device_ctx.device,
                BINDING_LIGHTING_UBO,
                buf0,
                size,
            );
        }

        let state = Self {
            ssbo_buffers: [buf0, buf1],
            ssbo_allocs: [Some(alloc0), Some(alloc1)],
            sun_elevation: 45.0,
            sun_azimuth: 135.0,
            sun_intensity: 2.0,
            sun_color: [1.0, 0.95, 0.9],
            ambient_intensity: 0.3,
            ambient_color: [0.15, 0.18, 0.25],
            time_of_day: 0.5, // noon
        };

        Ok(state)
    }

    /// Compute sun direction from elevation and azimuth angles.
    fn compute_sun_direction(&self) -> [f32; 3] {
        let elev_rad = self.sun_elevation.to_radians();
        let azim_rad = self.sun_azimuth.to_radians();
        let cos_elev = elev_rad.cos();
        let dir = [
            cos_elev * azim_rad.sin(),
            elev_rad.sin(),
            cos_elev * azim_rad.cos(),
        ];
        // Normalize
        let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if len > 1e-6 {
            [dir[0] / len, dir[1] / len, dir[2] / len]
        } else {
            [0.0, 1.0, 0.0]
        }
    }

    /// Build current LightingParams from CPU state.
    fn build_params(&self) -> LightingParams {
        let sun_direction = self.compute_sun_direction();

        LightingParams {
            sun_direction,
            sun_intensity: self.sun_intensity,
            sun_color: self.sun_color,
            ambient_intensity: self.ambient_intensity,
            ambient_color: self.ambient_color,
            time_of_day: self.time_of_day,
            // CSM shadow matrices — zeroed, filled by Plan 07-02.
            shadow_matrices: [[0.0; 16]; 4],
            cascade_splits: [0.0; 4],
            // Fog params — zeroed, filled by Plan 07-05.
            fog_color: [0.7, 0.75, 0.85],
            fog_density: 0.0,
            fog_start: 100.0,
            fog_end: 500.0,
            fog_type: 0,
            _pad: 0,
        }
    }

    /// Upload current lighting parameters to the current frame's SSBO and
    /// register it at binding 18.
    pub fn update(&self, renderer: &Renderer, current_frame: usize) {
        let params = self.build_params();
        let data = bytemuck::bytes_of(&params);

        let alloc = &self.ssbo_allocs[current_frame];
        if let Some(alloc) = alloc {
            if let Some(mapped) = alloc.mapped_ptr() {
                let ptr = mapped.as_ptr() as *mut u8;
                unsafe {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
                }
            }
        }

        // Register current frame's buffer at binding 18.
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_buffer(
                &renderer.device_ctx.device,
                BINDING_LIGHTING_UBO,
                self.ssbo_buffers[current_frame],
                std::mem::size_of::<LightingParams>() as u64,
            );
        }
    }

    /// Clean up GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        for i in 0..2 {
            if let Some(alloc) = self.ssbo_allocs[i].take() {
                super::helpers::destroy_allocated_buffer(
                    renderer,
                    self.ssbo_buffers[i],
                    alloc,
                )?;
            }
        }
        Ok(())
    }
}
