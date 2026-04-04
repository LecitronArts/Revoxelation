//! SSAO pass — screen-space ambient occlusion compute + bilateral blur.
//!
//! Runs after Hi-Z generation. Reads the resolved depth via binding 7
//! (Hi-Z mip 0). Writes blurred AO to binding 17 for fragment shader
//! consumption on the next frame.

use anyhow::Result;

use crate::renderer::graph::{AccessType, PassRecordContext, PassSetupContext, PassNode};
use crate::renderer::graph::resource::*;

pub struct SsaoComputePass;

impl PassNode for SsaoComputePass {
    fn name(&self) -> &'static str {
        "ssao"
    }

    fn setup(&mut self, ctx: &mut PassSetupContext) {
        // Reads depth (via Hi-Z mip 0), writes SSAO texture
        ctx.read(RES_DEPTH_IMAGE, AccessType::ComputeRead);
        ctx.write(RES_SSAO_TEXTURE, AccessType::ComputeWrite);
    }

    fn record(&self, ctx: &mut PassRecordContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::record_ssao_pass(ctx.renderer, ctx.command_buffer);
        }
        Ok(())
    }
}
