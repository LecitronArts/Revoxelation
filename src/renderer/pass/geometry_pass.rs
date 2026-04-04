//! Geometry pass — begin render pass + sky + meshlet/chunk draw.

use anyhow::Result;
use super::{FrameContext, RenderPass};

pub struct GeometryPass;

impl RenderPass for GeometryPass {
    fn name(&self) -> &'static str {
        "geometry"
    }

    fn record(&self, ctx: &mut FrameContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::begin_render_pass_pub(
                ctx.renderer,
                ctx.command_buffer,
                ctx.image_index,
            );
            crate::renderer::submit::draw_sky_pub(ctx.renderer, ctx.command_buffer);
            crate::renderer::submit::draw_meshlets_pub(
                ctx.renderer,
                ctx.command_buffer,
                ctx.camera_uniforms,
                ctx.current_time,
            );
        }
        Ok(())
    }
}
