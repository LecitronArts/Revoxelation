use std::collections::BTreeSet;
use std::ffi::CStr;

use anyhow::{Context, Result, anyhow};
use ash::{Instance, khr, vk};

pub struct DeviceContext {
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub graphics_queue: vk::Queue,
    pub present_queue: vk::Queue,
    pub graphics_family: u32,
    pub present_family: u32,
}

pub fn required_device_features_error() -> &'static str {
    "Vulkan device missing required features: samplerAnisotropy, multiDrawIndirect, drawIndirectFirstInstance"
}

pub fn pick_physical_device(
    instance: &Instance,
    surface_loader: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<DeviceContext> {
    let physical_devices = unsafe {
        instance
            .enumerate_physical_devices()
            .context("failed to enumerate Vulkan physical devices")?
    };

    let mut selected: Option<(vk::PhysicalDevice, u32, u32, bool)> = None;
    let mut saw_missing_required_features = false;
    for physical_device in physical_devices {
        let Some((graphics_family, present_family)) =
            find_queue_families(instance, surface_loader, surface, physical_device)?
        else {
            continue;
        };

        if !supports_required_extensions(instance, physical_device)? {
            continue;
        }

        if !supports_swapchain(surface_loader, physical_device, surface)? {
            continue;
        }

        if !supports_required_features(instance, physical_device) {
            saw_missing_required_features = true;
            continue;
        }

        let is_discrete = unsafe {
            instance
                .get_physical_device_properties(physical_device)
                .device_type
                == vk::PhysicalDeviceType::DISCRETE_GPU
        };

        match selected {
            None => {
                selected = Some((
                    physical_device,
                    graphics_family,
                    present_family,
                    is_discrete,
                ))
            }
            Some((_, _, _, false)) if is_discrete => {
                selected = Some((physical_device, graphics_family, present_family, true));
            }
            _ => {}
        }
    }

    let (physical_device, graphics_family, present_family, _) = selected.ok_or_else(|| {
        if saw_missing_required_features {
            anyhow!(required_device_features_error())
        } else {
            anyhow!("no Vulkan device supports graphics, present, and swapchain")
        }
    })?;

    let queue_priorities = [1.0_f32];
    let queue_create_infos: Vec<_> = BTreeSet::from([graphics_family, present_family])
        .into_iter()
        .map(|family| {
            vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family)
                .queue_priorities(&queue_priorities)
        })
        .collect();

    let required_extensions = [khr::swapchain::NAME.as_ptr()];
    let device_features = vk::PhysicalDeviceFeatures::default()
        .sampler_anisotropy(true)
        .multi_draw_indirect(true)
        .draw_indirect_first_instance(true);
    let create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_extension_names(&required_extensions)
        .enabled_features(&device_features);

    let device = unsafe {
        instance
            .create_device(physical_device, &create_info, None)
            .context("failed to create Vulkan logical device")?
    };

    let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
    let present_queue = unsafe { device.get_device_queue(present_family, 0) };

    Ok(DeviceContext {
        physical_device,
        device,
        graphics_queue,
        present_queue,
        graphics_family,
        present_family,
    })
}

fn supports_required_features(instance: &Instance, physical_device: vk::PhysicalDevice) -> bool {
    let features = unsafe { instance.get_physical_device_features(physical_device) };
    features.sampler_anisotropy == vk::TRUE
        && features.multi_draw_indirect == vk::TRUE
        && features.draw_indirect_first_instance == vk::TRUE
}

fn find_queue_families(
    instance: &Instance,
    surface_loader: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
) -> Result<Option<(u32, u32)>> {
    let queue_families =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
    let mut graphics_family = None;
    let mut present_family = None;

    for (index, family) in queue_families.iter().enumerate() {
        let index = index as u32;
        if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            graphics_family.get_or_insert(index);
        }

        let present_supported = unsafe {
            surface_loader
                .get_physical_device_surface_support(physical_device, index, surface)
                .context("failed to query Vulkan present support")?
        };
        if present_supported {
            present_family.get_or_insert(index);
        }

        if let (Some(graphics_family), Some(present_family)) = (graphics_family, present_family) {
            return Ok(Some((graphics_family, present_family)));
        }
    }

    Ok(None)
}

fn supports_required_extensions(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Result<bool> {
    let available_extensions = unsafe {
        instance
            .enumerate_device_extension_properties(physical_device)
            .context("failed to enumerate Vulkan device extensions")?
    };

    Ok(available_extensions.iter().any(|extension| unsafe {
        CStr::from_ptr(extension.extension_name.as_ptr()) == khr::swapchain::NAME
    }))
}

fn supports_swapchain(
    surface_loader: &khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
) -> Result<bool> {
    let formats = unsafe {
        surface_loader
            .get_physical_device_surface_formats(physical_device, surface)
            .context("failed to query Vulkan surface formats")?
    };
    let present_modes = unsafe {
        surface_loader
            .get_physical_device_surface_present_modes(physical_device, surface)
            .context("failed to query Vulkan present modes")?
    };

    Ok(!formats.is_empty() && !present_modes.is_empty())
}
