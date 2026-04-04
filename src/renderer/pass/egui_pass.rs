//! Egui pass — egui overlay rendering.

use anyhow::Result;
use super::{FrameContext, RenderPass};

pub struct EguiPass;

impl RenderPass for EguiPass {
    fn name(&self) -> &'static str {
        "egui"
    }

    fn record(&self, ctx: &mut FrameContext) -> Result<()> {
        crate::renderer::submit::draw_egui_pub(ctx.renderer, ctx.command_buffer)
    }
}
