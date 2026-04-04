//! Upload pass — staging ring reset + chunk delta uploads.

use anyhow::Result;
use super::{FrameContext, RenderPass};

pub struct UploadPass;

impl RenderPass for UploadPass {
    fn name(&self) -> &'static str {
        "upload"
    }

    fn record(&self, ctx: &mut FrameContext) -> Result<()> {
        ctx.renderer.record_chunk_delta_uploads(ctx.command_buffer)?;
        ctx.renderer.record_shadow_draw_setup(ctx.command_buffer)?;
        Ok(())
    }
}
