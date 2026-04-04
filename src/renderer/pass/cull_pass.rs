//! Cull pass — chunk-level + meshlet-level culling compute dispatches.
//!
//! Runs frustum culling (chunk level) and meshlet-level backface/frustum/Hi-Z
//! occlusion culling. Reads scene data + Hi-Z pyramid, writes visible meshlet
//! buffer + indirect draw commands.

use anyhow::Result;

use crate::renderer::graph::{AccessType, PassRecordContext, PassSetupContext, PassNode};
use crate::renderer::graph::resource::*;

pub struct CullPass;

impl PassNode for CullPass {
    fn name(&self) -> &'static str {
        "cull"
    }

    fn setup(&mut self, ctx: &mut PassSetupContext) {
        // Reads: scene buffer (chunk metadata), Hi-Z pyramid (occlusion cull),
        // and meshlet metadata (for meshlet_cull.comp).
        ctx.read(RES_SCENE_BUFFER, AccessType::ComputeRead);
        ctx.read(RES_HIZ_PYRAMID, AccessType::ComputeRead);
        // Meshlet cull reads meshlet meta via binding 10 (meshlet_meta SSBO).
        // Visible/indirect/count are overwritten by cull, but we need to read
        // the upload pass's initial data (shadow draw setup writes these).
        ctx.read(RES_VISIBLE_MESHLETS, AccessType::ComputeRead);
        ctx.read(RES_MESHLET_INDIRECT, AccessType::ComputeRead);
        ctx.read(RES_MESHLET_COUNT, AccessType::ComputeRead);
        // Writes: visible meshlet list, meshlet indirect, meshlet count,
        //         main-view indirect buffer, draw count
        ctx.write(RES_VISIBLE_MESHLETS, AccessType::ComputeWrite);
        ctx.write(RES_MESHLET_INDIRECT, AccessType::ComputeWrite);
        ctx.write(RES_MESHLET_COUNT, AccessType::ComputeWrite);
        ctx.write(RES_INDIRECT_BUFFER, AccessType::ComputeWrite);
        ctx.write(RES_DRAW_COUNT, AccessType::ComputeWrite);
    }

    fn record(&self, ctx: &mut PassRecordContext) -> Result<()> {
        unsafe {
            crate::renderer::submit::dispatch_chunk_cull(
                ctx.renderer,
                ctx.command_buffer,
                ctx.camera_uniforms,
            );
        }
        Ok(())
    }
}
