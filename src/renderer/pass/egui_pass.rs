//! Egui pass — egui overlay rendering.
//!
//! Records egui draw commands within the active render pass.
//! Handles texture delta uploads and mesh scratch buffer management.
//! Writes to swapchain color (overlay on top of geometry).

use anyhow::Result;

use crate::renderer::graph::{AccessType, PassRecordContext, PassSetupContext, PassNode};
use crate::renderer::graph::resource::*;

pub struct EguiPass;

impl PassNode for EguiPass {
    fn name(&self) -> &'static str {
        "egui"
    }

    fn setup(&mut self, ctx: &mut PassSetupContext) {
        // Egui draws on top of the geometry — reads and writes swapchain color.
        // Uses ColorWrite for both because egui blends into the existing color
        // attachment within the same render pass (no layout transition needed).
        ctx.write(RES_SWAPCHAIN_COLOR, AccessType::ColorWrite);
    }

    fn record(&self, ctx: &mut PassRecordContext) -> Result<()> {
        crate::renderer::submit::draw_egui(ctx.renderer, ctx.command_buffer)
    }
}
