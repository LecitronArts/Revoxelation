//! Directional lighting state and GPU parameter management (LGHT-01, LGHT-05).
//!
//! `LightingState` owns a double-buffered SSBO for `LightingParams` at binding 18.
//! Each frame, the CPU computes sun direction from time_of_day and uploads to the
//! current frame's buffer before any draw commands.
//!
//! `DayNightCycle` drives the sun orbit, color temperature, moonlight transition,
//! and fog color tracking across the full day cycle (LGHT-05).

use anyhow::Result;
use ash::vk;
use gpu_allocator::MemoryLocation;
use gpu_allocator::vulkan::Allocation;

use super::Renderer;
use super::bindless::BINDING_LIGHTING_UBO;
use super::helpers::create_allocated_buffer;

/// GPU-side lighting parameters (uploaded to binding 18 SSBO).
///
/// Layout matches the GLSL `LightingParams` struct in common.glsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightingParams {
    pub sun_direction: [f32; 3], // normalized world-space
    pub sun_intensity: f32,
    pub sun_color: [f32; 3], // linear RGB
    pub ambient_intensity: f32,
    pub ambient_color: [f32; 3], // linear RGB
    pub time_of_day: f32,        // 0.0-1.0 (0=midnight, 0.25=sunrise, 0.5=noon, 0.75=sunset)
    // CSM shadow matrices (4 cascades) — filled by Plan 07-02
    pub shadow_matrices: [[f32; 16]; 4], // mat4 x 4
    pub cascade_splits: [f32; 4],
    // Fog params (LGHT-05)
    pub fog_color: [f32; 3],
    pub fog_density: f32,
    pub fog_start: f32,
    pub fog_end: f32,
    pub _pad_align: [f32; 2],    // std430 padding: vec4 render_params needs 16-byte alignment
    pub render_params: [f32; 4], // x=screen_width, y=screen_height, z=shadow_resolution
    pub fog_type: u32,           // 0=linear, 1=exp, 2=exp2, 3=height
    pub _pad: u32,
    pub _pad2: [u32; 2],        // pad to match GLSL struct total size (376 bytes)
}

// ---------------------------------------------------------------------------
// Fog configuration (LGHT-05)
// ---------------------------------------------------------------------------

/// Distance fog type selector.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FogType {
    Linear = 0,
    Exponential = 1,
    ExponentialSquared = 2,
    Height = 3,
}

impl FogType {
    pub fn as_str(&self) -> &'static str {
        match self {
            FogType::Linear => "Linear",
            FogType::Exponential => "Exponential",
            FogType::ExponentialSquared => "Exp Squared",
            FogType::Height => "Height",
        }
    }

    pub fn all() -> &'static [FogType] {
        &[
            FogType::Linear,
            FogType::Exponential,
            FogType::ExponentialSquared,
            FogType::Height,
        ]
    }
}

/// Runtime fog configuration (egui-adjustable).
#[derive(Clone, Debug)]
pub struct FogConfig {
    pub enabled: bool,
    pub fog_type: FogType,
    pub density: f32, // 0.001-0.1
    pub start: f32,   // linear fog start distance
    pub end: f32,     // linear fog end distance
}

impl Default for FogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            fog_type: FogType::Exponential,
            density: 0.008,
            start: 100.0,
            end: 500.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Day-Night Cycle (LGHT-05)
// ---------------------------------------------------------------------------

/// Drives the full day-night cycle: sun orbit, color temperature, moonlight.
pub struct DayNightCycle {
    /// Time of day (0.0-1.0 continuous). 0=midnight, 0.25=sunrise, 0.5=noon, 0.75=sunset.
    pub time_of_day: f32,
    /// Real seconds per game day (default 600.0 = 10 minutes).
    pub day_speed: f32,
    /// Whether the cycle is paused.
    pub paused: bool,
}

impl Default for DayNightCycle {
    fn default() -> Self {
        Self {
            time_of_day: 0.375, // ~9:00 AM — lower sun angle for more visible directional lighting
            day_speed: 600.0,
            paused: true, // start paused so user controls first
        }
    }
}

impl DayNightCycle {
    /// Advance time by delta_time real seconds.
    pub fn tick(&mut self, delta_time: f32) {
        if self.paused || self.day_speed <= 0.0 {
            return;
        }
        self.time_of_day += delta_time / self.day_speed;
        // Wrap around [0, 1)
        self.time_of_day -= self.time_of_day.floor();
    }

    /// Compute sun direction from time_of_day.
    ///
    /// Sun orbits in the XY plane: rises at 0.25 (east), noon at 0.5 (zenith),
    /// sets at 0.75 (west). Night: sun is below the horizon.
    pub fn sun_direction(&self) -> [f32; 3] {
        let angle = (self.time_of_day - 0.25) * std::f32::consts::TAU;
        let y = angle.sin(); // height
        let x = angle.cos(); // horizontal
        let dir = glam::Vec3::new(x, y, 0.3).normalize();
        dir.into()
    }

    /// Compute sun elevation (sine of vertical angle, -1 to 1).
    pub fn sun_elevation(&self) -> f32 {
        let angle = (self.time_of_day - 0.25) * std::f32::consts::TAU;
        angle.sin()
    }

    /// Compute sun color and intensity based on elevation.
    ///
    /// Noon: white (6500K), dawn/dusk: warm (3000K), night: moonlight (blue, dim).
    pub fn sun_color_and_intensity(&self) -> ([f32; 3], f32) {
        let elevation = self.sun_elevation();

        if elevation > 0.05 {
            // Daytime: lerp from warm to white based on elevation
            let t = elevation.clamp(0.0, 1.0);
            let color = lerp_color([1.0, 0.6, 0.3], [1.0, 0.98, 0.95], t);
            let intensity = 1.5 + t * 1.0; // 1.5 at horizon, 2.5 at noon
            (color, intensity)
        } else if elevation > -0.1 {
            // Twilight: crossfade sun → moon
            let t = ((elevation + 0.1) / 0.15).clamp(0.0, 1.0);
            let color = lerp_color([0.3, 0.4, 0.6], [1.0, 0.6, 0.3], t);
            let intensity = 0.1 + t * 1.4;
            (color, intensity)
        } else {
            // Night: moonlight (dim, blue-shifted)
            ([0.3, 0.4, 0.6], 0.1)
        }
    }

    /// Compute ambient color and intensity for the current time of day.
    pub fn ambient_color_and_intensity(&self) -> ([f32; 3], f32) {
        let elevation = self.sun_elevation();

        if elevation > 0.05 {
            // Daytime ambient
            let t = elevation.clamp(0.0, 1.0);
            let color = lerp_color([0.2, 0.2, 0.3], [0.15, 0.18, 0.25], t);
            let intensity = 0.15 + t * 0.2;
            (color, intensity)
        } else if elevation > -0.1 {
            // Twilight ambient
            let t = ((elevation + 0.1) / 0.15).clamp(0.0, 1.0);
            let color = lerp_color([0.05, 0.05, 0.1], [0.2, 0.2, 0.3], t);
            let intensity = 0.05 + t * 0.1;
            (color, intensity)
        } else {
            // Night ambient (very dark blue)
            ([0.05, 0.05, 0.1], 0.05)
        }
    }

    /// Compute fog color from sky horizon color at the current time.
    ///
    /// The fog color should match the sky horizon to create seamless blending.
    pub fn fog_color(&self) -> [f32; 3] {
        let elevation = self.sun_elevation();

        if elevation > 0.1 {
            // Daytime: bright horizon
            let t = elevation.clamp(0.0, 1.0);
            lerp_color([0.8, 0.65, 0.5], [0.7, 0.75, 0.85], t)
        } else if elevation > -0.05 {
            // Twilight: warm/orange horizon
            let t = ((elevation + 0.05) / 0.15).clamp(0.0, 1.0);
            lerp_color([0.1, 0.1, 0.15], [0.8, 0.65, 0.5], t)
        } else {
            // Night: dark blue
            [0.05, 0.05, 0.1]
        }
    }

    /// Get time of day as HH:MM string.
    pub fn time_as_hhmm(&self) -> String {
        let total_minutes = (self.time_of_day * 24.0 * 60.0) as u32;
        let hours = (total_minutes / 60) % 24;
        let minutes = total_minutes % 60;
        format!("{:02}:{:02}", hours, minutes)
    }
}

/// Linearly interpolate between two RGB colors.
fn lerp_color(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

// ---------------------------------------------------------------------------
// LightingState — CPU-side lighting control
// ---------------------------------------------------------------------------

/// CPU-side lighting control state.
pub struct LightingState {
    /// Double-buffered SSBOs for LightingParams (one per in-flight frame).
    pub ssbo_buffers: [vk::Buffer; 2],
    pub ssbo_allocs: [Option<Allocation>; 2],
    /// Sun elevation angle in degrees (0-90) — manual override.
    pub sun_elevation: f32,
    /// Sun azimuth angle in degrees (0-360) — manual override.
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
    /// Day-night cycle driver (LGHT-05).
    pub day_night: DayNightCycle,
    /// Fog configuration (LGHT-05).
    pub fog_config: FogConfig,
    /// Whether day-night cycle drives lighting (vs manual controls).
    pub use_day_night_cycle: bool,
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
            time_of_day: 0.375, // ~9:00 AM
            day_night: DayNightCycle::default(),
            fog_config: FogConfig::default(),
            use_day_night_cycle: false, // default OFF so manual sun controls work
        };

        Ok(state)
    }

    /// Advance the day-night cycle and update derived lighting parameters.
    pub fn tick_day_night(&mut self, delta_time: f32) {
        self.day_night.tick(delta_time);

        if self.use_day_night_cycle {
            // Update sun direction from cycle.
            let sun_dir = self.day_night.sun_direction();
            // Convert direction to elevation/azimuth for compatibility with manual controls.
            self.sun_elevation = sun_dir[1].asin().to_degrees();
            self.sun_azimuth = sun_dir[0].atan2(sun_dir[2]).to_degrees();
            if self.sun_azimuth < 0.0 {
                self.sun_azimuth += 360.0;
            }

            // Update sun color and intensity from cycle.
            let (color, intensity) = self.day_night.sun_color_and_intensity();
            self.sun_color = color;
            self.sun_intensity = intensity;

            // Update ambient from cycle.
            let (amb_color, amb_intensity) = self.day_night.ambient_color_and_intensity();
            self.ambient_color = amb_color;
            self.ambient_intensity = amb_intensity;

            // Sync time_of_day.
            self.time_of_day = self.day_night.time_of_day;
        }
    }

    /// Compute sun direction from elevation and azimuth angles (public accessor).
    pub fn compute_sun_direction_pub(&self) -> [f32; 3] {
        self.compute_sun_direction()
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
    fn build_params(&self, renderer: &Renderer) -> LightingParams {
        let sun_direction = self.compute_sun_direction();
        let extent = renderer.swapchain_ctx.extent;
        let shadow_resolution = renderer
            .shadow_map
            .as_ref()
            .map(|shadow| shadow.resolution as f32)
            .unwrap_or(renderer.config.shadow.resolution as f32);

        // Fog color tracks sky horizon color when day-night cycle is active.
        let fog_color = if self.use_day_night_cycle {
            self.day_night.fog_color()
        } else {
            [0.7, 0.75, 0.85]
        };

        LightingParams {
            sun_direction,
            sun_intensity: self.sun_intensity,
            sun_color: self.sun_color,
            ambient_intensity: self.ambient_intensity,
            ambient_color: self.ambient_color,
            time_of_day: self.time_of_day,
            // CSM shadow matrices — zeroed, filled by submit.rs record_csm_shadow_passes.
            shadow_matrices: [[0.0; 16]; 4],
            cascade_splits: [0.0; 4],
            // Fog params (LGHT-05).
            fog_color: if self.fog_config.enabled {
                fog_color
            } else {
                [0.0; 3]
            },
            fog_density: if self.fog_config.enabled {
                self.fog_config.density
            } else {
                0.0
            },
            fog_start: self.fog_config.start,
            fog_end: self.fog_config.end,
            _pad_align: [0.0; 2],
            render_params: [
                extent.width as f32,
                extent.height as f32,
                shadow_resolution,
                0.0,
            ],
            fog_type: if self.fog_config.enabled {
                self.fog_config.fog_type as u32
            } else {
                u32::MAX
            },
            _pad: 0,
            _pad2: [0; 2],
        }
    }

    /// Upload current lighting parameters to the current frame's SSBO and
    /// register it at binding 18.
    pub fn update(&self, renderer: &Renderer, current_frame: usize) {
        let params = self.build_params(renderer);
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
                super::helpers::destroy_allocated_buffer(renderer, self.ssbo_buffers[i], alloc)?;
            }
        }
        Ok(())
    }
}
