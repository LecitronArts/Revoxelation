use anyhow::{Context, Result, anyhow};
use ash::{Instance, khr, vk};
use gpu_allocator::{
    MemoryLocation,
    vulkan::{Allocation, AllocationCreateDesc, AllocationScheme, Allocator},
};

use crate::renderer::device::DeviceContext;

pub const DEPTH_FORMAT: vk::Format = vk::Format::D32_SFLOAT;

pub struct SwapchainContext {
    pub swapchain_loader: khr::swapchain::Device,
    pub swapchain: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub image_views: Vec<vk::ImageView>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub render_pass: vk::RenderPass,
    pub framebuffers: Vec<vk::Framebuffer>,
    pub depth_image: vk::Image,
    pub depth_image_view: vk::ImageView,
    pub depth_allocation: Option<Allocation>,
}

pub fn create_swapchain_context(
    instance: &Instance,
    device_ctx: &DeviceContext,
    surface_loader: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
    window_extent: vk::Extent2D,
    allocator: &mut Allocator,
) -> Result<SwapchainContext> {
    let capabilities = unsafe {
        surface_loader
            .get_physical_device_surface_capabilities(device_ctx.physical_device, surface)
            .context("failed to query Vulkan surface capabilities")?
    };
    let surface_formats = unsafe {
        surface_loader
            .get_physical_device_surface_formats(device_ctx.physical_device, surface)
            .context("failed to query Vulkan surface formats")?
    };
    let present_modes = unsafe {
        surface_loader
            .get_physical_device_surface_present_modes(device_ctx.physical_device, surface)
            .context("failed to query Vulkan present modes")?
    };

    let surface_format = choose_surface_format(&surface_formats)
        .ok_or_else(|| anyhow!("surface reported no compatible Vulkan formats"))?;
    let present_mode = choose_present_mode(&present_modes);
    let extent = choose_extent(&capabilities, window_extent);
    let image_count = choose_image_count(&capabilities);
    let queue_family_indices = [device_ctx.graphics_family, device_ctx.present_family];

    let mut create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(surface_format.format)
        .image_color_space(surface_format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .pre_transform(capabilities.current_transform)
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(present_mode)
        .clipped(true);

    if device_ctx.graphics_family != device_ctx.present_family {
        create_info = create_info
            .image_sharing_mode(vk::SharingMode::CONCURRENT)
            .queue_family_indices(&queue_family_indices);
    } else {
        create_info = create_info.image_sharing_mode(vk::SharingMode::EXCLUSIVE);
    }

    let swapchain_loader = khr::swapchain::Device::new(instance, &device_ctx.device);
    let swapchain = unsafe {
        swapchain_loader
            .create_swapchain(&create_info, None)
            .context("failed to create Vulkan swapchain")?
    };
    let images = unsafe {
        swapchain_loader
            .get_swapchain_images(swapchain)
            .context("failed to enumerate Vulkan swapchain images")?
    };

    let image_views = images
        .iter()
        .map(|image| create_image_view(&device_ctx.device, *image, surface_format.format))
        .collect::<Result<Vec<_>>>()?;
    let render_pass = create_render_pass(&device_ctx.device, surface_format.format, DEPTH_FORMAT)?;

    // Create depth image and view (shared across all framebuffers).
    let depth_image = unsafe {
        device_ctx
            .device
            .create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(DEPTH_FORMAT)
                    .extent(vk::Extent3D {
                        width: extent.width,
                        height: extent.height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )
            .context("failed to create depth image")?
    };
    let depth_requirements = unsafe {
        device_ctx.device.get_image_memory_requirements(depth_image)
    };
    let depth_allocation = allocator
        .allocate(&AllocationCreateDesc {
            name: "swapchain-depth",
            requirements: depth_requirements,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })
        .map_err(|error| anyhow!("failed to allocate depth image memory: {error}"))?;
    unsafe {
        device_ctx
            .device
            .bind_image_memory(depth_image, depth_allocation.memory(), depth_allocation.offset())
            .context("failed to bind depth image memory")?;
    }
    let depth_subresource_range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::DEPTH)
        .level_count(1)
        .layer_count(1);
    let depth_image_view = unsafe {
        device_ctx
            .device
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(depth_image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(DEPTH_FORMAT)
                    .subresource_range(depth_subresource_range),
                None,
            )
            .context("failed to create depth image view")?
    };

    let framebuffers = image_views
        .iter()
        .map(|image_view| create_framebuffer(&device_ctx.device, render_pass, *image_view, depth_image_view, extent))
        .collect::<Result<Vec<_>>>()?;

    Ok(SwapchainContext {
        swapchain_loader,
        swapchain,
        images,
        image_views,
        format: surface_format.format,
        extent,
        render_pass,
        framebuffers,
        depth_image,
        depth_image_view,
        depth_allocation: Some(depth_allocation),
    })
}

fn choose_surface_format(formats: &[vk::SurfaceFormatKHR]) -> Option<vk::SurfaceFormatKHR> {
    formats
        .iter()
        .copied()
        .find(|format| {
            format.format == vk::Format::B8G8R8A8_SRGB
                && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .or_else(|| formats.first().copied())
}

fn choose_present_mode(present_modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    present_modes
        .iter()
        .copied()
        .find(|mode| *mode == vk::PresentModeKHR::MAILBOX)
        .unwrap_or(vk::PresentModeKHR::FIFO)
}

fn choose_extent(
    capabilities: &vk::SurfaceCapabilitiesKHR,
    window_extent: vk::Extent2D,
) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        return capabilities.current_extent;
    }

    vk::Extent2D {
        width: window_extent.width.clamp(
            capabilities.min_image_extent.width,
            capabilities.max_image_extent.width,
        ),
        height: window_extent.height.clamp(
            capabilities.min_image_extent.height,
            capabilities.max_image_extent.height,
        ),
    }
}

fn choose_image_count(capabilities: &vk::SurfaceCapabilitiesKHR) -> u32 {
    let desired = capabilities.min_image_count.saturating_add(1);
    if capabilities.max_image_count == 0 {
        desired
    } else {
        desired.min(capabilities.max_image_count)
    }
}

fn create_image_view(
    device: &ash::Device,
    image: vk::Image,
    format: vk::Format,
) -> Result<vk::ImageView> {
    let subresource_range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1);
    let create_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(subresource_range);

    unsafe {
        device
            .create_image_view(&create_info, None)
            .context("failed to create Vulkan image view")
    }
}

fn create_render_pass(device: &ash::Device, color_format: vk::Format, depth_format: vk::Format) -> Result<vk::RenderPass> {
    let attachments = [
        vk::AttachmentDescription::default()
            .format(color_format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),
        vk::AttachmentDescription::default()
            .format(depth_format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
    ];
    let color_attachment_refs = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let depth_attachment_ref = vk::AttachmentReference::default()
        .attachment(1)
        .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachment_refs)
        .depth_stencil_attachment(&depth_attachment_ref)];
    let dependencies = [vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
        )
        .dst_stage_mask(
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
        )
        .dst_access_mask(
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        )];
    let create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    unsafe {
        device
            .create_render_pass(&create_info, None)
            .context("failed to create Vulkan render pass")
    }
}

fn create_framebuffer(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    color_view: vk::ImageView,
    depth_view: vk::ImageView,
    extent: vk::Extent2D,
) -> Result<vk::Framebuffer> {
    let attachments = [color_view, depth_view];
    let create_info = vk::FramebufferCreateInfo::default()
        .render_pass(render_pass)
        .attachments(&attachments)
        .width(extent.width)
        .height(extent.height)
        .layers(1);

    unsafe {
        device
            .create_framebuffer(&create_info, None)
            .context("failed to create Vulkan framebuffer")
    }
}
