//! Hi-Z pass — depth pyramid generation for next-frame occlusion culling.
//!
//! Generates a hierarchical depth buffer (Hi-Z pyramid) from the resolved
//! depth image. The pyramid is sampled by the cull pass on the next frame.
//! Must run after the render pass ends (depth is finalized).

use anyhow::Result;

use crate::renderer::graph::{AccessType, PassRecordContext, PassSetupContext, PassNode};
use crate::renderer::graph::resource::*;

pub struct HiZPass;

impl PassNode for HiZPass {
    fn name(&self) -> &'static str {
        "hiz"
    }

    fn setup(&mut self, ctx: &mut PassSetupContext) {
        // Reads finalized depth, writes Hi-Z pyramid
        ctx.read(RES_DEPTH_IMAGE, AccessType::ComputeRead);
        ctx.write(RES_HIZ_PYRAMID, AccessType::ComputeWrite);
    }

    fn record(&self, ctx: &mut PassRecordContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::generate_hiz(ctx.renderer, ctx.command_buffer);
        }
        Ok(())
    }
}
