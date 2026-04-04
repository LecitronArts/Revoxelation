//! Cull pass — chunk-level + meshlet-level culling compute dispatches.

use anyhow::Result;
use super::{FrameContext, RenderPass};

pub struct CullPass;

impl RenderPass for CullPass {
    fn name(&self) -> &'static str {
        "cull"
    }

    fn record(&self, ctx: &mut FrameContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::dispatch_chunk_cull_pub(
                ctx.renderer,
                ctx.command_buffer,
                ctx.camera_uniforms,
            );
        }
        Ok(())
    }
}
