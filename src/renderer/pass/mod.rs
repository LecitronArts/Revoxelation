//! Render pass abstraction for submit decomposition (Phase 2 prep).
//!
//! Each pass module encapsulates one logical render stage. submit.rs calls
//! them sequentially. In Phase 3+ they will implement the PassNode trait
//! for automatic barrier management via RenderGraph.

pub mod upload_pass;
pub mod shadow_pass;
pub mod cull_pass;
pub mod geometry_pass;
pub mod egui_pass;
pub mod hiz_pass;
pub mod ssao_pass;

use anyhow::Result;
use ash::vk;

use super::Renderer;
use super::camera::CameraUniforms;

/// Context passed to each render pass during recording.
///
/// Holds the command buffer, camera uniforms, and other per-frame data
/// needed by all passes. Avoids passing 5+ parameters to each function.
pub struct FrameContext<'a> {
    pub renderer: &'a mut Renderer,
    pub command_buffer: vk::CommandBuffer,
    pub camera_uniforms: &'a CameraUniforms,
    pub current_time: f32,
    pub image_index: u32,
}

/// Trait for render passes — will be extended with resource declarations
/// in Phase 3 (RenderGraph).
pub trait RenderPass {
    /// Human-readable name for debug/profiling labels.
    fn name(&self) -> &'static str;

    /// Record commands into the frame's command buffer.
    ///
    /// Passes may skip recording if their preconditions aren't met
    /// (e.g., SSAO disabled, no shadow map). This is not an error.
    fn record(&self, ctx: &mut FrameContext) -> Result<()>;
}
