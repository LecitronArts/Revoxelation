use std::collections::BTreeSet;
use std::ffi::CStr;

use anyhow::{Context, Result, anyhow};
use ash::{Instance, ext, khr, vk};

/// Information about the device's subgroup support, queried at device creation.
#[derive(Debug, Clone, Copy)]
pub struct SubgroupInfo {
    /// Subgroup size (typically 32 or 64).
    pub subgroup_size: u32,
    /// Raw bitmask of supported subgroup operations.
    pub supported_operations: vk::SubgroupFeatureFlags,
    /// Whether VK_SUBGROUP_FEATURE_BALLOT_BIT is supported.
    pub has_ballot: bool,
    /// Whether VK_SUBGROUP_FEATURE_BASIC_BIT is supported.
    pub has_basic: bool,
}

pub struct DeviceContext {
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub graphics_queue: vk::Queue,
    pub present_queue: vk::Queue,
    pub graphics_family: u32,
    pub present_family: u32,
    /// Subgroup feature info, queried at device creation (D-11).
    pub subgroup_info: SubgroupInfo,
    /// Whether VK_EXT_mesh_shader (task + mesh) is supported and enabled.
    pub mesh_shader_supported: bool,
    /// ash mesh shader extension loader (present only when mesh_shader_supported == true).
    pub mesh_shader_fn: Option<ext::mesh_shader::Device>,
}

/// Names of the 7 required Vulkan 1.2 features for bindless rendering.
/// No fallback path exists — unsupported GPUs fail fast.
const REQUIRED_VULKAN12_FEATURE_NAMES: [&str; 7] = [
    "descriptor_indexing",
    "shader_sampled_image_array_non_uniform_indexing",
    "runtime_descriptor_array",
    "descriptor_binding_partially_bound",
    "descriptor_binding_sampled_image_update_after_bind",
    "descriptor_binding_storage_buffer_update_after_bind",
    "draw_indirect_count",
];

/// Check which required Vulkan 1.2 features are missing on the given physical device.
/// Returns a list of missing feature names (empty = all supported).
///
/// Uses `get_physical_device_features2` with `PhysicalDeviceVulkan12Features` in
/// the pNext chain (core Vulkan 1.2 struct, not extension-era DescriptorIndexingFeatures).
fn missing_vulkan12_features(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Vec<&'static str> {
    let mut vulkan12_features = vk::PhysicalDeviceVulkan12Features::default();
    let mut features2 =
        vk::PhysicalDeviceFeatures2::default().push_next(&mut vulkan12_features);

    unsafe {
        instance.get_physical_device_features2(physical_device, &mut features2);
    }

    let mut missing = Vec::new();

    if vulkan12_features.descriptor_indexing == vk::FALSE {
        missing.push(REQUIRED_VULKAN12_FEATURE_NAMES[0]);
    }
    if vulkan12_features.shader_sampled_image_array_non_uniform_indexing == vk::FALSE {
        missing.push(REQUIRED_VULKAN12_FEATURE_NAMES[1]);
    }
    if vulkan12_features.runtime_descriptor_array == vk::FALSE {
        missing.push(REQUIRED_VULKAN12_FEATURE_NAMES[2]);
    }
    if vulkan12_features.descriptor_binding_partially_bound == vk::FALSE {
        missing.push(REQUIRED_VULKAN12_FEATURE_NAMES[3]);
    }
    if vulkan12_features.descriptor_binding_sampled_image_update_after_bind == vk::FALSE {
        missing.push(REQUIRED_VULKAN12_FEATURE_NAMES[4]);
    }
    if vulkan12_features.descriptor_binding_storage_buffer_update_after_bind == vk::FALSE {
        missing.push(REQUIRED_VULKAN12_FEATURE_NAMES[5]);
    }
    if vulkan12_features.draw_indirect_count == vk::FALSE {
        missing.push(REQUIRED_VULKAN12_FEATURE_NAMES[6]);
    }

    missing
}

fn supports_required_features(instance: &Instance, physical_device: vk::PhysicalDevice) -> bool {
    let features = unsafe { instance.get_physical_device_features(physical_device) };
    features.sampler_anisotropy == vk::TRUE
        && features.multi_draw_indirect == vk::TRUE
        && features.draw_indirect_first_instance == vk::TRUE
}

fn device_name(instance: &Instance, physical_device: vk::PhysicalDevice) -> String {
    let props = unsafe { instance.get_physical_device_properties(physical_device) };
    let name = unsafe { CStr::from_ptr(props.device_name.as_ptr()) };
    name.to_string_lossy().into_owned()
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
    let mut last_missing_1_0 = false;
    let mut last_missing_1_2: Option<(String, Vec<&'static str>)> = None;

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

        // Check Vulkan 1.0 required features.
        if !supports_required_features(instance, physical_device) {
            last_missing_1_0 = true;
            let name = device_name(instance, physical_device);
            log::warn!(
                "Skipping GPU {name}: missing required Vulkan 1.0 features \
                 (samplerAnisotropy, multiDrawIndirect, drawIndirectFirstInstance)"
            );
            continue;
        }

        // Check Vulkan 1.2 required features.
        let missing = missing_vulkan12_features(instance, physical_device);
        if !missing.is_empty() {
            let name = device_name(instance, physical_device);
            log::warn!(
                "Skipping GPU {name}: Vulkan 1.2 feature(s) missing: {}",
                missing.join(", ")
            );
            last_missing_1_2 = Some((name, missing));
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
        if let Some((gpu_name, missing)) = last_missing_1_2 {
            anyhow!(
                "Vulkan 1.2 feature(s) missing: {}. GPU: {}.",
                missing.join(", "),
                gpu_name
            )
        } else if last_missing_1_0 {
            anyhow!(
                "Vulkan device missing required features: \
                 samplerAnisotropy, multiDrawIndirect, drawIndirectFirstInstance"
            )
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

    // Check for optional VK_EXT_mesh_shader extension support.
    let mesh_shader_ext_available = {
        let available_extensions = unsafe {
            instance
                .enumerate_device_extension_properties(physical_device)
                .unwrap_or_default()
        };
        available_extensions.iter().any(|ext_props| {
            let name = unsafe { CStr::from_ptr(ext_props.extension_name.as_ptr()) };
            name == ext::mesh_shader::NAME
        })
    };

    // Query VkPhysicalDeviceMeshShaderFeaturesEXT if extension is available.
    let mesh_shader_supported = if mesh_shader_ext_available {
        let mut mesh_shader_features = vk::PhysicalDeviceMeshShaderFeaturesEXT::default();
        let mut features2_query =
            vk::PhysicalDeviceFeatures2::default().push_next(&mut mesh_shader_features);
        unsafe {
            instance.get_physical_device_features2(physical_device, &mut features2_query);
        }
        let supported =
            mesh_shader_features.task_shader == vk::TRUE && mesh_shader_features.mesh_shader == vk::TRUE;
        if supported {
            log::info!("VK_EXT_mesh_shader: taskShader + meshShader supported — enabling mesh shader path");
        } else {
            log::info!(
                "VK_EXT_mesh_shader extension present but features missing (taskShader={}, meshShader={}) — using compute fallback",
                mesh_shader_features.task_shader == vk::TRUE,
                mesh_shader_features.mesh_shader == vk::TRUE,
            );
        }
        supported
    } else {
        log::info!("VK_EXT_mesh_shader not available — using compute+indirect fallback path");
        false
    };

    // Build device extension list: always swapchain, optionally mesh_shader.
    let device_extensions: Vec<*const std::ffi::c_char> = if mesh_shader_supported {
        vec![khr::swapchain::NAME.as_ptr(), ext::mesh_shader::NAME.as_ptr()]
    } else {
        vec![khr::swapchain::NAME.as_ptr()]
    };

    // Vulkan 1.0 features — set via PhysicalDeviceFeatures2.features field.
    let device_features_1_0 = vk::PhysicalDeviceFeatures::default()
        .sampler_anisotropy(true)
        .multi_draw_indirect(true)
        .draw_indirect_first_instance(true);

    // Vulkan 1.2 features — chained via pNext on PhysicalDeviceFeatures2.
    let mut vulkan12_features = vk::PhysicalDeviceVulkan12Features::default()
        .descriptor_indexing(true)
        .shader_sampled_image_array_non_uniform_indexing(true)
        .runtime_descriptor_array(true)
        .descriptor_binding_partially_bound(true)
        .descriptor_binding_sampled_image_update_after_bind(true)
        .descriptor_binding_storage_buffer_update_after_bind(true)
        .draw_indirect_count(true);

    let mut features2 = vk::PhysicalDeviceFeatures2::default()
        .features(device_features_1_0)
        .push_next(&mut vulkan12_features);

    // Optionally enable mesh shader features in pNext chain.
    let mut mesh_shader_enable_features = vk::PhysicalDeviceMeshShaderFeaturesEXT::default()
        .task_shader(true)
        .mesh_shader(true);
    if mesh_shader_supported {
        features2 = features2.push_next(&mut mesh_shader_enable_features);
    }

    let create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_extension_names(&device_extensions)
        .push_next(&mut features2);

    let device = unsafe {
        instance
            .create_device(physical_device, &create_info, None)
            .context("failed to create Vulkan logical device")?
    };

    let graphics_queue = unsafe { device.get_device_queue(graphics_family, 0) };
    let present_queue = unsafe { device.get_device_queue(present_family, 0) };

    // Query VkPhysicalDeviceSubgroupProperties for subgroup size and features (D-11).
    let subgroup_info = {
        let mut subgroup_props = vk::PhysicalDeviceSubgroupProperties::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut subgroup_props);
        unsafe {
            instance.get_physical_device_properties2(physical_device, &mut props2);
        }
        let has_basic = subgroup_props
            .supported_operations
            .contains(vk::SubgroupFeatureFlags::BASIC);
        let has_ballot = subgroup_props
            .supported_operations
            .contains(vk::SubgroupFeatureFlags::BALLOT);
        log::info!(
            "Subgroup: size={}, operations={:?}, basic={}, ballot={}",
            subgroup_props.subgroup_size,
            subgroup_props.supported_operations,
            has_basic,
            has_ballot,
        );
        if !has_ballot {
            log::warn!(
                "VK_SUBGROUP_FEATURE_BALLOT_BIT not supported — meshlet subgroup compaction \
                 will fall back to per-thread atomics"
            );
        }
        if !has_basic {
            log::warn!(
                "VK_SUBGROUP_FEATURE_BASIC_BIT not supported — subgroup operations unavailable"
            );
        }
        SubgroupInfo {
            subgroup_size: subgroup_props.subgroup_size,
            supported_operations: subgroup_props.supported_operations,
            has_ballot,
            has_basic,
        }
    };

    // Create ash mesh shader extension loader if supported.
    let mesh_shader_fn = if mesh_shader_supported {
        Some(ext::mesh_shader::Device::new(instance, &device))
    } else {
        None
    };

    Ok(DeviceContext {
        physical_device,
        device,
        graphics_queue,
        present_queue,
        graphics_family,
        present_family,
        subgroup_info,
        mesh_shader_supported,
        mesh_shader_fn,
    })
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
