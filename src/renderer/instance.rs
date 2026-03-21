use std::ffi::{CStr, CString};
use std::os::raw::c_void;

use anyhow::{Context, Result};
use ash::{Entry, Instance, ext, vk};

pub fn create_instance(
    entry: &Entry,
    display_handle: raw_window_handle::RawDisplayHandle,
) -> Result<Instance> {
    let app_name = CString::new("Revoxelation").expect("static app name is valid");
    let mut extension_names = ash_window::enumerate_required_extensions(display_handle)
        .context("failed to enumerate required Vulkan surface extensions")?
        .to_vec();

    #[cfg(debug_assertions)]
    extension_names.push(ext::debug_utils::NAME.as_ptr());

    #[cfg(debug_assertions)]
    let validation_layer = CString::new("VK_LAYER_KHRONOS_validation").expect("static layer name");
    #[cfg(debug_assertions)]
    let layer_names = [validation_layer.as_ptr()];
    #[cfg(not(debug_assertions))]
    let layer_names: [*const i8; 0] = [];

    let app_info = vk::ApplicationInfo::default()
        .application_name(app_name.as_c_str())
        .engine_name(app_name.as_c_str())
        .api_version(vk::make_api_version(0, 1, 3, 0));

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_names)
        .enabled_layer_names(&layer_names);

    unsafe {
        entry
            .create_instance(&create_info, None)
            .context("failed to create Vulkan instance")
    }
}

#[cfg(debug_assertions)]
pub fn setup_debug_messenger(
    entry: &Entry,
    instance: &Instance,
) -> Result<vk::DebugUtilsMessengerEXT> {
    let debug_utils_loader = ext::debug_utils::Instance::new(entry, instance);
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
