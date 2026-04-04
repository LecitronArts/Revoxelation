//! Well-known resource IDs for the render graph.
//!
//! These constants identify GPU resources that passes declare as dependencies.
//! The graph uses these to compute barrier insertion points.

use super::ResourceId;

/// Scene buffer (binding 0) — chunk metadata SSBO.
pub const RES_SCENE_BUFFER: ResourceId = ResourceId(0);

/// Dense indirect draw buffer (binding 3).
pub const RES_INDIRECT_BUFFER: ResourceId = ResourceId(1);

/// Draw count buffer (binding 5).
pub const RES_DRAW_COUNT: ResourceId = ResourceId(2);

/// Visible meshlet buffer (binding 13).
pub const RES_VISIBLE_MESHLETS: ResourceId = ResourceId(3);

/// Meshlet indirect buffer (binding 14).
pub const RES_MESHLET_INDIRECT: ResourceId = ResourceId(4);

/// Meshlet count buffer (binding 15).
pub const RES_MESHLET_COUNT: ResourceId = ResourceId(5);

/// Hi-Z pyramid image (binding 7).
pub const RES_HIZ_PYRAMID: ResourceId = ResourceId(6);

/// Resolved depth image (swapchain depth).
pub const RES_DEPTH_IMAGE: ResourceId = ResourceId(7);

/// SSAO output texture (binding 17).
pub const RES_SSAO_TEXTURE: ResourceId = ResourceId(8);

/// CSM shadow map array (binding 16).
pub const RES_SHADOW_MAPS: ResourceId = ResourceId(9);

/// Swapchain color image.
pub const RES_SWAPCHAIN_COLOR: ResourceId = ResourceId(10);

/// Staging ring (transfer source).
pub const RES_STAGING: ResourceId = ResourceId(11);
