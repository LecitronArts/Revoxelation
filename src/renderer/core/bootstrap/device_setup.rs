use std::sync::Arc;

use anyhow::{Context, Result, bail};
use winit::dpi::PhysicalSize;
use winit::window::Window;

use crate::renderer::resources::surface::{
    SurfaceResourcesEvent, clamp_render_extent, surface_resources_policy,
};

pub(super) struct DeviceSetupOutput {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub max_storage_binding_size: u64,
    pub config: wgpu::SurfaceConfiguration,
    pub size: PhysicalSize<u32>,
    pub output_format: wgpu::TextureFormat,
}

pub(super) async fn setup_device(window: Arc<Window>) -> Result<DeviceSetupOutput> {
    let instance = wgpu::Instance::default();
    let surface = instance
        .create_surface(window.clone())
        .context("failed to create wgpu surface")?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .context("no suitable GPU adapter found")?;

    let capabilities = surface.get_capabilities(&adapter);
    let mut format = capabilities
        .formats
        .iter()
        .copied()
        .find(|candidate| *candidate == wgpu::TextureFormat::Rgba8UnormSrgb)
        .or_else(|| {
            capabilities
                .formats
                .iter()
                .copied()
                .find(|candidate| *candidate == wgpu::TextureFormat::Rgba8Unorm)
        })
        .or_else(|| {
            capabilities
                .formats
                .iter()
                .copied()
                .find(|candidate| candidate.is_srgb())
        })
        .unwrap_or(capabilities.formats[0]);

    let mut output_format = format.remove_srgb_suffix();
    let mut required_features = wgpu::Features::empty();
    if output_format == wgpu::TextureFormat::Bgra8Unorm {
        if adapter
            .features()
            .contains(wgpu::Features::BGRA8UNORM_STORAGE)
        {
            required_features |= wgpu::Features::BGRA8UNORM_STORAGE;
        } else if capabilities
            .formats
            .contains(&wgpu::TextureFormat::Rgba8UnormSrgb)
        {
            format = wgpu::TextureFormat::Rgba8UnormSrgb;
            output_format = wgpu::TextureFormat::Rgba8Unorm;
        } else if capabilities
            .formats
            .contains(&wgpu::TextureFormat::Rgba8Unorm)
        {
            format = wgpu::TextureFormat::Rgba8Unorm;
            output_format = wgpu::TextureFormat::Rgba8Unorm;
        } else {
            bail!(
                "surface requires BGRA output but adapter does not support BGRA8 storage textures"
            );
        }
    }

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("revoxelation-device"),
                required_features,
                required_limits: adapter.limits(),
            },
            None,
        )
        .await
        .context("failed to request logical device")?;
    let max_storage_binding_size = device.limits().max_storage_buffer_binding_size as u64;

    let mut size = window.inner_size();
    if size.width == 0 || size.height == 0 {
        size = PhysicalSize::new(1, 1);
    }
    let clamped = clamp_render_extent(size.width, size.height, max_storage_binding_size);
    size = PhysicalSize::new(clamped.0, clamped.1);

    let present_mode = if capabilities
        .present_modes
        .contains(&wgpu::PresentMode::Mailbox)
    {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    };

    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
        format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode,
        alpha_mode: capabilities.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    let new_surface_policy = surface_resources_policy(SurfaceResourcesEvent::New);
    debug_assert!(new_surface_policy.rebuild_resources);
    debug_assert!(new_surface_policy.reconfigure_surface);
    surface.configure(&device, &config);

    Ok(DeviceSetupOutput {
        surface,
        device,
        queue,
        max_storage_binding_size,
        config,
        size,
        output_format,
    })
}
