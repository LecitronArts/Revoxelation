//! Shadow pass — CSM 4-cascade depth rendering (LGHT-02).
//!
//! Delegates to the existing `record_csm_shadow_passes` in submit.rs.

use anyhow::Result;
use super::{FrameContext, RenderPass};

pub struct ShadowPass;

impl RenderPass for ShadowPass {
    fn name(&self) -> &'static str {
        "shadow"
    }

    fn record(&self, ctx: &mut FrameContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::record_csm_shadow_passes_pub(
                ctx.renderer,
                ctx.command_buffer,
                ctx.camera_uniforms,
            );
        }
        Ok(())
    }
}
