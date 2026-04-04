//! Hi-Z pass — depth pyramid generation for occlusion culling.

use anyhow::Result;
use super::{FrameContext, RenderPass};

pub struct HiZPass;

impl RenderPass for HiZPass {
    fn name(&self) -> &'static str {
        "hiz"
    }

    fn record(&self, ctx: &mut FrameContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::generate_hiz_pub(ctx.renderer, ctx.command_buffer);
        }
        Ok(())
    }
}
