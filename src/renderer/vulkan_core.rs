//! VulkanCore sub-struct — core Vulkan infrastructure (REFAC-01).
//!
//! Groups the core Vulkan handles that form the foundation of the renderer:
//! entry, instance, surface, debug utilities. These are created first and
//! destroyed last.

use ash::{khr, vk};

/// Core Vulkan infrastructure: entry, instance, debug, surface (REFAC-01).
///
/// Logical view into the renderer's core handles. Used as a borrow-friendly
/// reference bundle when functions need access to core Vulkan state without
/// borrowing the entire Renderer.
#[allow(dead_code)]
pub struct VulkanCore<'a> {
    pub entry: &'a ash::Entry,
    pub instance: &'a ash::Instance,
    pub surface_loader: &'a khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
}
