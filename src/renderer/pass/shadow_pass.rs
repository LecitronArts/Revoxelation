//! Shadow pass — CSM 4-cascade depth rendering (LGHT-02).
//!
//! Records shadow depth-only render passes before the main geometry pass.
//! Reads scene buffer + indirect buffer, writes shadow maps.

use anyhow::Result;

use crate::renderer::graph::{AccessType, PassRecordContext, PassSetupContext, PassNode};
use crate::renderer::graph::resource::*;

pub struct ShadowPass;

impl PassNode for ShadowPass {
    fn name(&self) -> &'static str {
        "shadow"
    }

    fn setup(&mut self, ctx: &mut PassSetupContext) {
        // Shadow pass reads scene buffer (chunk instances) and meshlet buffers
        // produced by upload pass, then draws into shadow depth maps.
        ctx.read(RES_SCENE_BUFFER, AccessType::VertexInput);
        ctx.read(RES_VISIBLE_MESHLETS, AccessType::VertexInput);
        ctx.read(RES_MESHLET_INDIRECT, AccessType::IndirectRead);
        ctx.read(RES_MESHLET_COUNT, AccessType::IndirectRead);
        ctx.write(RES_SHADOW_MAPS, AccessType::DepthWrite);
    }

    fn record(&self, ctx: &mut PassRecordContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::record_csm_shadow_passes(
                ctx.renderer,
                ctx.command_buffer,
                ctx.camera_uniforms,
            );
        }
        Ok(())
    }
}
