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
                    .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::SAMPLED)
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

/// Recreate swapchain and all dependent resources (depth image, image views, framebuffers)
/// after a window resize or Vulkan OUT_OF_DATE/SUBOPTIMAL error.
///
/// Per D-01: device_wait_idle → destroy framebuffers → destroy depth → destroy old image views
/// → create new swapchain (with old_swapchain param) → destroy old swapchain → create new resources.
///
/// Per D-02/D-03: render pass and pipelines are NOT recreated.
pub fn recreate_swapchain_context(
    renderer: &mut super::Renderer,
    new_extent: vk::Extent2D,
) -> Result<()> {
    let device = &renderer.device_ctx.device;

    // Wait for all GPU work to complete before destroying resources.
    unsafe {
        device
            .device_wait_idle()
            .context("failed to wait for device idle during swapchain recreation")?;
    }

    // 1. Destroy old framebuffers.
    for framebuffer in renderer.swapchain_ctx.framebuffers.drain(..).rev() {
        unsafe {
            device.destroy_framebuffer(framebuffer, None);
        }
    }

    // 2. Destroy old depth image/view.
    unsafe {
        device.destroy_image_view(renderer.swapchain_ctx.depth_image_view, None);
    }
    if let Some(alloc) = renderer.swapchain_ctx.depth_allocation.take() {
        let _ = renderer.allocator.free(alloc);
    }
    unsafe {
        device.destroy_image(renderer.swapchain_ctx.depth_image, None);
    }

    // 3. Destroy old color image views.
    for image_view in renderer.swapchain_ctx.image_views.drain(..).rev() {
        unsafe {
            device.destroy_image_view(image_view, None);
        }
    }

    // 4. Query new surface capabilities to pick correct extent.
    let capabilities = unsafe {
        renderer
            .surface_loader
            .get_physical_device_surface_capabilities(
                renderer.device_ctx.physical_device,
                renderer.surface,
            )
            .context("failed to query surface capabilities during swapchain recreation")?
    };
    let surface_formats = unsafe {
        renderer
            .surface_loader
            .get_physical_device_surface_formats(
                renderer.device_ctx.physical_device,
                renderer.surface,
            )
            .context("failed to query surface formats during swapchain recreation")?
    };
    let present_modes = unsafe {
        renderer
            .surface_loader
            .get_physical_device_surface_present_modes(
                renderer.device_ctx.physical_device,
                renderer.surface,
            )
            .context("failed to query present modes during swapchain recreation")?
    };

    let surface_format = choose_surface_format(&surface_formats)
        .ok_or_else(|| anyhow!("no compatible surface format during swapchain recreation"))?;
    let present_mode = choose_present_mode(&present_modes);
    let extent = choose_extent(&capabilities, new_extent);
    let image_count = choose_image_count(&capabilities);
    let queue_family_indices = [
        renderer.device_ctx.graphics_family,
        renderer.device_ctx.present_family,
    ];

    // 5. Create new swapchain with old_swapchain for driver optimization.
    let old_swapchain = renderer.swapchain_ctx.swapchain;
    let mut create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(renderer.surface)
        .min_image_count(image_count)
        .image_format(surface_format.format)
        .image_color_space(surface_format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .pre_transform(capabilities.current_transform)
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(present_mode)
        .clipped(true)
        .old_swapchain(old_swapchain);

    if renderer.device_ctx.graphics_family != renderer.device_ctx.present_family {
        create_info = create_info
            .image_sharing_mode(vk::SharingMode::CONCURRENT)
            .queue_family_indices(&queue_family_indices);
    } else {
        create_info = create_info.image_sharing_mode(vk::SharingMode::EXCLUSIVE);
    }

    let new_swapchain = unsafe {
        renderer
            .swapchain_ctx
            .swapchain_loader
            .create_swapchain(&create_info, None)
            .context("failed to create new swapchain during recreation")?
    };

    // 6. Destroy old swapchain after new one is created.
    unsafe {
        renderer
            .swapchain_ctx
            .swapchain_loader
            .destroy_swapchain(old_swapchain, None);
    }

    renderer.swapchain_ctx.swapchain = new_swapchain;

    // 7. Get new swapchain images.
    let images = unsafe {
        renderer
            .swapchain_ctx
            .swapchain_loader
            .get_swapchain_images(new_swapchain)
            .context("failed to enumerate swapchain images during recreation")?
    };

    // 8. Create new image views.
    let image_views = images
        .iter()
        .map(|image| create_image_view(device, *image, surface_format.format))
        .collect::<Result<Vec<_>>>()?;

    // 9. Create new depth image + view (reuse render pass — D-02).
    let depth_image = unsafe {
        device
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
                    .usage(
                        vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT
                            | vk::ImageUsageFlags::SAMPLED,
                    )
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )
            .context("failed to create depth image during swapchain recreation")?
    };
    let depth_requirements = unsafe { device.get_image_memory_requirements(depth_image) };
    let depth_allocation = renderer
        .allocator
        .allocate(&gpu_allocator::vulkan::AllocationCreateDesc {
            name: "swapchain-depth",
            requirements: depth_requirements,
            location: MemoryLocation::GpuOnly,
            linear: false,
            allocation_scheme: AllocationScheme::GpuAllocatorManaged,
        })
        .map_err(|error| anyhow!("failed to allocate depth image memory during recreation: {error}"))?;
    unsafe {
        device
            .bind_image_memory(
                depth_image,
                depth_allocation.memory(),
                depth_allocation.offset(),
            )
            .context("failed to bind depth image memory during recreation")?;
    }
    let depth_subresource_range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::DEPTH)
        .level_count(1)
        .layer_count(1);
    let depth_image_view = unsafe {
        device
            .create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(depth_image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(DEPTH_FORMAT)
                    .subresource_range(depth_subresource_range),
                None,
            )
            .context("failed to create depth image view during recreation")?
    };

    // 10. Create new framebuffers.
    let framebuffers = image_views
        .iter()
        .map(|image_view| {
            create_framebuffer(
                device,
                renderer.swapchain_ctx.render_pass,
                *image_view,
                depth_image_view,
                extent,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    // 11. Update swapchain context fields in place.
    renderer.swapchain_ctx.images = images;
    renderer.swapchain_ctx.image_views = image_views;
    renderer.swapchain_ctx.format = surface_format.format;
    renderer.swapchain_ctx.extent = extent;
    renderer.swapchain_ctx.framebuffers = framebuffers;
    renderer.swapchain_ctx.depth_image = depth_image;
    renderer.swapchain_ctx.depth_image_view = depth_image_view;
    renderer.swapchain_ctx.depth_allocation = Some(depth_allocation);

    // 12. Recreate Hi-Z pyramid for the new swapchain dimensions (FIX-01).
    // Sequence per D-04: take → destroy old → create new → re-register bindless → store.
    if let Some(old_hiz) = renderer.hiz_pyramid.take() {
        old_hiz.destroy(renderer);
        let new_hiz = super::hiz::HiZPyramid::new(renderer, extent.width, extent.height)?;
        // Re-register the new Hi-Z full_view + sampler at bindless binding 7.
        if let Some(bindless) = &renderer.bindless {
            bindless.register_image(
                &renderer.device_ctx.device,
                7,
                new_hiz.full_view,
                new_hiz.sampler,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        }
        log::info!(
            "Hi-Z pyramid recreated: {}x{} ({} mips)",
            extent.width,
            extent.height,
            super::hiz::hiz_mip_count(extent.width, extent.height),
        );
        renderer.hiz_pyramid = Some(new_hiz);
    }

    log::info!(
        "Swapchain recreated: {}x{}",
        extent.width,
        extent.height
    );

    Ok(())
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
            .store_op(vk::AttachmentStoreOp::STORE)
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
