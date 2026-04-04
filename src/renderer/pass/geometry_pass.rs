//! Geometry pass — begin render pass + sky + meshlet/chunk draw.
//!
//! This is the main rendering pass containing:
//! 1. Begin MSAA render pass with dynamic clear color
//! 2. Sky fullscreen triangle (behind all geometry)
//! 3. Meshlet or legacy per-chunk indirect draws
//!
//! Reads cull output (indirect buffers, visible meshlets), shadow maps, SSAO.
//! Writes color + depth attachments.

use anyhow::Result;

use crate::renderer::graph::{AccessType, PassRecordContext, PassSetupContext, PassNode};
use crate::renderer::graph::resource::*;

pub struct GeometryPass;

impl PassNode for GeometryPass {
    fn name(&self) -> &'static str {
        "geometry"
    }

    fn setup(&mut self, ctx: &mut PassSetupContext) {
        // Reads: cull output, shadow maps for lighting, SSAO for ambient.
        // Scene buffer (chunk instances) also needed for vertex shader lookups.
        ctx.read(RES_SCENE_BUFFER, AccessType::VertexInput);
        ctx.read(RES_VISIBLE_MESHLETS, AccessType::VertexInput);
        ctx.read(RES_MESHLET_INDIRECT, AccessType::IndirectRead);
        ctx.read(RES_MESHLET_COUNT, AccessType::IndirectRead);
        ctx.read(RES_INDIRECT_BUFFER, AccessType::IndirectRead);
        ctx.read(RES_DRAW_COUNT, AccessType::IndirectRead);
        ctx.read(RES_SHADOW_MAPS, AccessType::FragmentRead);
        ctx.read(RES_SSAO_TEXTURE, AccessType::FragmentRead);
        // Writes: color + depth attachments
        ctx.write(RES_SWAPCHAIN_COLOR, AccessType::ColorWrite);
        ctx.write(RES_DEPTH_IMAGE, AccessType::DepthWrite);
    }

    fn record(&self, ctx: &mut PassRecordContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::begin_render_pass(
                ctx.renderer,
                ctx.command_buffer,
                ctx.image_index,
            );
            crate::renderer::submit::draw_sky(ctx.renderer, ctx.command_buffer);
            crate::renderer::submit::draw_meshlets(
                ctx.renderer,
                ctx.command_buffer,
                ctx.camera_uniforms,
                ctx.current_time,
            );
        }
        Ok(())
    }
}
