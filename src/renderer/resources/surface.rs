use bytemuck::bytes_of;
use wgpu::util::DeviceExt;

use crate::renderer::RendererSettings;
use crate::renderer::protocol::{GiReservoirGpu, ReservoirGpu, SurfaceSampleGpu, SvgfUniform};

use super::restir_storage::RestirStorage;

pub const SVGF_MAX_ATROUS_PASSES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceResourcesEvent {
    New,
    Resize,
    Reconfigure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceResourcesPolicy {
    pub reconfigure_surface: bool,
    pub rebuild_resources: bool,
}

pub const fn surface_resources_policy(event: SurfaceResourcesEvent) -> SurfaceResourcesPolicy {
    match event {
        SurfaceResourcesEvent::New | SurfaceResourcesEvent::Resize => SurfaceResourcesPolicy {
            reconfigure_surface: true,
            rebuild_resources: true,
        },
        SurfaceResourcesEvent::Reconfigure => SurfaceResourcesPolicy {
            reconfigure_surface: true,
            rebuild_resources: false,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceResourceState {
    pub width: u32,
    pub height: u32,
    pub active_svgf_passes: usize,
    pub resolve_source_selector: u32,
    pub atrous_uniform_count: usize,
}

pub struct RebuiltSurfaceResources {
    pub output_texture: wgpu::Texture,
    pub output_view: wgpu::TextureView,
    pub accumulation_buffer: wgpu::Buffer,
    pub restir_storage: RestirStorage,
    pub svgf_ping_buffer: wgpu::Buffer,
    pub svgf_pong_buffer: wgpu::Buffer,
    pub svgf_debug_buffer: wgpu::Buffer,
    pub svgf_init_uniform_buffer: wgpu::Buffer,
    pub svgf_resolve_uniform_buffer: wgpu::Buffer,
    pub svgf_atrous_uniform_buffers: Vec<wgpu::Buffer>,
}

pub fn build_surface_resource_state(
    width: u32,
    height: u32,
    max_storage_binding_size: u64,
    settings: &RendererSettings,
) -> SurfaceResourceState {
    let (width, height) = clamp_render_extent(width, height, max_storage_binding_size);
    let active_svgf_passes = active_svgf_passes(settings);
    SurfaceResourceState {
        width,
        height,
        active_svgf_passes,
        resolve_source_selector: resolve_source_selector(active_svgf_passes),
        atrous_uniform_count: SVGF_MAX_ATROUS_PASSES,
    }
}

pub fn rebuild_surface_resources(
    device: &wgpu::Device,
    state: SurfaceResourceState,
    output_format: wgpu::TextureFormat,
    settings: &RendererSettings,
) -> RebuiltSurfaceResources {
    let (output_texture, output_view) =
        create_output_texture(device, state.width, state.height, output_format);
    let accumulation_buffer = create_accumulation_buffer(device, state.width, state.height);
    let restir_storage = RestirStorage::new(device, state.width, state.height);
    let svgf_ping_buffer = create_svgf_buffer(device, state.width, state.height);
    let svgf_pong_buffer = create_svgf_buffer(device, state.width, state.height);
    let svgf_debug_buffer = create_svgf_buffer(device, state.width, state.height);
    let svgf_init_uniform_buffer =
        create_svgf_uniform_buffer(device, state.width, state.height, 1, 0, settings);
    let svgf_resolve_uniform_buffer = create_svgf_uniform_buffer(
        device,
        state.width,
        state.height,
        1,
        state.resolve_source_selector,
        settings,
    );
    let svgf_atrous_uniform_buffers =
        create_svgf_atrous_uniform_buffers(device, state.width, state.height, settings);

    RebuiltSurfaceResources {
        output_texture,
        output_view,
        accumulation_buffer,
        restir_storage,
        svgf_ping_buffer,
        svgf_pong_buffer,
        svgf_debug_buffer,
        svgf_init_uniform_buffer,
        svgf_resolve_uniform_buffer,
        svgf_atrous_uniform_buffers,
    }
}

pub fn clamp_render_extent(width: u32, height: u32, max_storage_binding_size: u64) -> (u32, u32) {
    let width = width.max(1);
    let height = height.max(1);
    let pixel_count = (width as u64) * (height as u64);
    let reservoir_stride = std::mem::size_of::<ReservoirGpu>() as u64;
    let gi_reservoir_stride = std::mem::size_of::<GiReservoirGpu>() as u64;
    let surface_stride = (2 * std::mem::size_of::<SurfaceSampleGpu>()) as u64;
    let max_pixels = (max_storage_binding_size / reservoir_stride)
        .min(max_storage_binding_size / gi_reservoir_stride)
        .min(max_storage_binding_size / surface_stride)
        .max(1);
    if pixel_count <= max_pixels {
        return (width, height);
    }

    let aspect = width as f64 / height as f64;
    let max_pixels_f = max_pixels as f64;
    let mut target_h = (max_pixels_f / aspect).sqrt().floor().max(1.0) as u32;
    let mut target_w = (target_h as f64 * aspect).floor().max(1.0) as u32;
    while (target_w as u64) * (target_h as u64) > max_pixels {
        if target_w >= target_h && target_w > 1 {
            target_w -= 1;
        } else if target_h > 1 {
            target_h -= 1;
        } else {
            break;
        }
    }
    (target_w.max(1), target_h.max(1))
}

pub fn active_svgf_passes(settings: &RendererSettings) -> usize {
    if settings.svgf_enabled {
        settings.svgf_passes.min(SVGF_MAX_ATROUS_PASSES as u32) as usize
    } else {
        0
    }
}

pub const fn resolve_source_selector(active_svgf_passes: usize) -> u32 {
    (active_svgf_passes as u32) & 1u32
}

pub const fn svgf_atrous_step(pass_index: u32, step_scale: u32) -> u32 {
    let step_scale = if step_scale == 0 { 1 } else { step_scale };
    let base = 1u32 << pass_index;
    if pass_index == 0 {
        1
    } else {
        base.saturating_mul(step_scale)
    }
}

pub fn svgf_anti_ghosting_extras(settings: &RendererSettings) -> [f32; 4] {
    [
        settings.svgf_invalid_variance_boost,
        settings.svgf_center_weight,
        settings.svgf_history_normal_reject_cos,
        settings.svgf_history_depth_reject_scale,
    ]
}

#[cfg(test)]
pub fn responsive_history_weight(
    base_weight: f32,
    normal_cos: f32,
    depth_relative_delta: f32,
    motion_pixels: f32,
    settings: &RendererSettings,
) -> f32 {
    let normal_threshold = settings.svgf_history_normal_reject_cos.clamp(0.5, 0.999);
    let depth_threshold = settings.svgf_history_depth_reject_scale.clamp(0.01, 0.5);
    if normal_cos < normal_threshold || depth_relative_delta > depth_threshold {
        return 0.0;
    }

    let normal_margin =
        ((normal_cos - normal_threshold) / (1.0 - normal_threshold)).clamp(0.0, 1.0);
    let depth_margin = (1.0 - depth_relative_delta / depth_threshold).clamp(0.0, 1.0);
    let motion_term = (-motion_pixels * 0.35).exp().clamp(0.0, 1.0);
    let reactive_term = (normal_margin * depth_margin * motion_term).clamp(0.0, 1.0);
    (base_weight.clamp(0.0, 1.0) * reactive_term).clamp(0.0, 1.0)
}

pub fn svgf_uniform(
    width: u32,
    height: u32,
    step: u32,
    source_selector: u32,
    settings: &RendererSettings,
) -> SvgfUniform {
    SvgfUniform {
        resolution_step: [width.max(1), height.max(1), step.max(1), source_selector],
        params: [
            settings.svgf_normal_phi,
            settings.svgf_depth_phi,
            settings.svgf_luma_phi,
            settings.svgf_clamp_sigma,
        ],
        extras: svgf_anti_ghosting_extras(settings),
    }
}

pub fn create_svgf_uniform_buffer(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    step: u32,
    source_selector: u32,
    settings: &RendererSettings,
) -> wgpu::Buffer {
    let uniform = svgf_uniform(width, height, step, source_selector, settings);
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("svgf-uniform-buffer"),
        contents: bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

pub fn create_svgf_atrous_uniform_buffers(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    settings: &RendererSettings,
) -> Vec<wgpu::Buffer> {
    let mut uniform_buffers = Vec::with_capacity(SVGF_MAX_ATROUS_PASSES);
    for index in 0..SVGF_MAX_ATROUS_PASSES {
        let step = svgf_atrous_step(index as u32, settings.svgf_step_scale);
        let source_selector = (index as u32) & 1u32;
        uniform_buffers.push(create_svgf_uniform_buffer(
            device,
            width,
            height,
            step,
            source_selector,
            settings,
        ));
    }
    uniform_buffers
}

pub fn create_output_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("trace-output-texture"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

pub fn create_accumulation_buffer(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    let pixel_count = (width.max(1) as u64) * (height.max(1) as u64);
    let byte_size = (pixel_count * std::mem::size_of::<[f32; 4]>() as u64).max(16);
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("trace-accumulation-buffer"),
        size: byte_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub fn create_svgf_buffer(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Buffer {
    let pixel_count = (width.max(1) as u64) * (height.max(1) as u64);
    let byte_size = (pixel_count * std::mem::size_of::<[f32; 4]>() as u64).max(16);
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("svgf-buffer"),
        size: byte_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_render_extent_keeps_size_when_under_limit() {
        let (w, h) = clamp_render_extent(640, 360, u64::MAX);
        assert_eq!(w, 640);
        assert_eq!(h, 360);
    }

    #[test]
    fn clamp_render_extent_reduces_size_when_over_limit() {
        let (w, h) = clamp_render_extent(4096, 4096, 16 * 1024);
        assert!(w < 4096 || h < 4096);
        assert!(w >= 1 && h >= 1);
    }

    #[test]
    fn resolve_source_selector_matches_pass_parity() {
        assert_eq!(resolve_source_selector(0), 0);
        assert_eq!(resolve_source_selector(1), 1);
        assert_eq!(resolve_source_selector(2), 0);
        assert_eq!(resolve_source_selector(5), 1);
    }

    #[test]
    fn svgf_atrous_step_keeps_first_pass_at_one_even_with_scale() {
        assert_eq!(svgf_atrous_step(0, 1), 1);
        assert_eq!(svgf_atrous_step(0, 4), 1);
    }

    #[test]
    fn svgf_atrous_step_applies_scale_after_first_pass() {
        assert_eq!(svgf_atrous_step(1, 1), 2);
        assert_eq!(svgf_atrous_step(1, 2), 4);
        assert_eq!(svgf_atrous_step(2, 3), 12);
    }

    #[test]
    fn surface_resource_state_reflects_svgf_settings() {
        let mut settings = RendererSettings::default();
        settings.svgf_enabled = true;
        settings.svgf_passes = 3;

        let state = build_surface_resource_state(1280, 720, u64::MAX, &settings);
        assert_eq!(state.width, 1280);
        assert_eq!(state.height, 720);
        assert_eq!(state.active_svgf_passes, 3);
        assert_eq!(state.resolve_source_selector, 1);
        assert_eq!(state.atrous_uniform_count, SVGF_MAX_ATROUS_PASSES);
    }

    #[test]
    fn surface_resource_state_is_stable_for_same_inputs() {
        let settings = RendererSettings::default();
        let first = build_surface_resource_state(800, 600, 1024 * 1024, &settings);
        let second = build_surface_resource_state(800, 600, 1024 * 1024, &settings);
        assert_eq!(first, second);
    }

    #[test]
    fn surface_resource_policy_matches_events() {
        let new_policy = surface_resources_policy(SurfaceResourcesEvent::New);
        assert!(new_policy.reconfigure_surface);
        assert!(new_policy.rebuild_resources);

        let resize_policy = surface_resources_policy(SurfaceResourcesEvent::Resize);
        assert!(resize_policy.reconfigure_surface);
        assert!(resize_policy.rebuild_resources);

        let reconfigure_policy = surface_resources_policy(SurfaceResourcesEvent::Reconfigure);
        assert!(reconfigure_policy.reconfigure_surface);
        assert!(!reconfigure_policy.rebuild_resources);
    }

    #[test]
    fn svgf_uniform_encodes_history_reject_thresholds_into_extras() {
        let mut settings = RendererSettings::default();
        settings.svgf_invalid_variance_boost = 5.5;
        settings.svgf_center_weight = 3.0;
        settings.svgf_history_normal_reject_cos = 0.9;
        settings.svgf_history_depth_reject_scale = 0.2;

        let uniform = svgf_uniform(640, 360, 1, 0, &settings);
        assert_eq!(uniform.extras, [5.5, 3.0, 0.9, 0.2]);
    }

    #[test]
    fn responsive_history_weight_rejects_when_normal_below_threshold() {
        let mut settings = RendererSettings::default();
        settings.svgf_history_normal_reject_cos = 0.88;

        let weight = responsive_history_weight(0.8, 0.87, 0.02, 0.0, &settings);
        assert_eq!(weight, 0.0);
    }

    #[test]
    fn responsive_history_weight_rejects_when_depth_above_threshold() {
        let mut settings = RendererSettings::default();
        settings.svgf_history_depth_reject_scale = 0.09;

        let weight = responsive_history_weight(0.8, 0.95, 0.11, 0.0, &settings);
        assert_eq!(weight, 0.0);
    }

    #[test]
    fn responsive_history_weight_drops_as_inconsistency_grows() {
        let settings = RendererSettings::default();
        let strong = responsive_history_weight(0.9, 0.98, 0.01, 0.0, &settings);
        let weak = responsive_history_weight(0.9, 0.90, 0.08, 2.5, &settings);
        assert!(strong > weak);
    }

    #[test]
    fn responsive_history_weight_stays_within_unit_interval() {
        let settings = RendererSettings::default();
        let weight = responsive_history_weight(2.5, 0.99, 0.001, -3.0, &settings);
        assert!((0.0..=1.0).contains(&weight));
    }
}
