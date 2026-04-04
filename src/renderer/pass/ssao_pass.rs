//! SSAO pass — screen-space ambient occlusion compute + bilateral blur.

use anyhow::Result;
use super::{FrameContext, RenderPass};

pub struct SsaoComputePass;

impl RenderPass for SsaoComputePass {
    fn name(&self) -> &'static str {
        "ssao"
    }

    fn record(&self, ctx: &mut FrameContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::record_ssao_pass_pub(ctx.renderer, ctx.command_buffer);
        }
        Ok(())
    }
}
