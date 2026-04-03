use std::ffi::{CStr, CString};
#[cfg(debug_assertions)]
use std::os::raw::c_void;

use anyhow::{Context, Result};
use ash::{Entry, Instance, vk};

pub const VALIDATION_LAYER_NAME: &str = "VK_LAYER_KHRONOS_validation";
pub const DEBUG_UTILS_EXTENSION_NAME: &str = "VK_EXT_debug_utils";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceDebugConfig {
    pub validation_layer_enabled: bool,
    pub debug_utils_enabled: bool,
}

pub struct InstanceBootstrap {
    pub instance: Instance,
    pub debug: InstanceDebugConfig,
}

pub fn resolve_debug_instance_config(
    available_layers: &[String],
    available_extensions: &[String],
) -> InstanceDebugConfig {
    if !cfg!(debug_assertions) {
        return InstanceDebugConfig {
            validation_layer_enabled: false,
            debug_utils_enabled: false,
        };
    }

    let validation_layer_enabled = available_layers
        .iter()
        .any(|name| name == VALIDATION_LAYER_NAME);
    if !validation_layer_enabled {
        eprintln!(
            "VK_LAYER_KHRONOS_validation not available; continuing without validation layer."
        );
    }

    let debug_utils_enabled = validation_layer_enabled
        && available_extensions
            .iter()
            .any(|name| name == DEBUG_UTILS_EXTENSION_NAME);
    if validation_layer_enabled && !debug_utils_enabled {
        eprintln!("VK_EXT_debug_utils not available; continuing without Vulkan debug messenger.");
    }

    InstanceDebugConfig {
        validation_layer_enabled,
        debug_utils_enabled,
    }
}

pub fn create_instance(
    entry: &Entry,
    display_handle: raw_window_handle::RawDisplayHandle,
) -> Result<InstanceBootstrap> {
    let app_name = CString::new("Revoxelation").expect("static app name is valid");
    let mut extension_names = ash_window::enumerate_required_extensions(display_handle)
        .context("failed to enumerate required Vulkan surface extensions")?
        .to_vec();
    let available_layers = available_instance_layer_names(entry)?;
    let available_extensions = available_instance_extension_names(entry)?;
    let debug = resolve_debug_instance_config(&available_layers, &available_extensions);
    if debug.debug_utils_enabled {
        extension_names.push(ash::ext::debug_utils::NAME.as_ptr());
    }

    let validation_layer = CString::new(VALIDATION_LAYER_NAME).expect("static layer name");
    let layer_names = if debug.validation_layer_enabled {
        vec![validation_layer.as_ptr()]
    } else {
        Vec::new()
    };

    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name.as_c_str())
        .engine_name(app_name.as_c_str())
        .api_version(vk::make_api_version(0, 1, 3, 0));

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_names)
        .enabled_layer_names(&layer_names);

    unsafe {
        let instance = entry
            .create_instance(&create_info, None)
            .context("failed to create Vulkan instance")?;
        Ok(InstanceBootstrap { instance, debug })
    }
}

fn available_instance_layer_names(entry: &Entry) -> Result<Vec<String>> {
    let properties = unsafe {
        entry
            .enumerate_instance_layer_properties()
            .context("failed to enumerate Vulkan instance layers")?
    };

    Ok(properties
        .iter()
        .map(|property| vk_name_to_string(&property.layer_name))
        .collect())
}

fn available_instance_extension_names(entry: &Entry) -> Result<Vec<String>> {
    let properties = unsafe {
        entry
            .enumerate_instance_extension_properties(None)
            .context("failed to enumerate Vulkan instance extensions")?
    };

    Ok(properties
        .iter()
        .map(|property| vk_name_to_string(&property.extension_name))
        .collect())
}

fn vk_name_to_string(raw_name: &[i8]) -> String {
    unsafe { CStr::from_ptr(raw_name.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(debug_assertions)]
pub fn setup_debug_messenger(
    entry: &Entry,
    instance: &Instance,
) -> Result<vk::DebugUtilsMessengerEXT> {
    let debug_utils_loader = ash::ext::debug_utils::Instance::new(entry, instance);
    let create_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(vulkan_debug_callback));

    unsafe {
        debug_utils_loader
            .create_debug_utils_messenger(&create_info, None)
            .context("failed to create Vulkan debug messenger")
    }
}

#[cfg(debug_assertions)]
unsafe extern "system" fn vulkan_debug_callback(
    _severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _types: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    if callback_data.is_null() {
        return vk::FALSE;
    }

    let message_ptr = unsafe { (*callback_data).p_message };
    if !message_ptr.is_null() {
        let message = unsafe { CStr::from_ptr(message_ptr) };
        eprintln!("[vulkan] {}", message.to_string_lossy());
    }

    vk::FALSE
}
