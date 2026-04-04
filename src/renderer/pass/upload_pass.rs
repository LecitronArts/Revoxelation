//! Upload pass — staging ring reset + chunk delta uploads + lighting/sky SSBO updates.
//!
//! This pass handles all pre-draw data uploads:
//! 1. Chunk delta uploads (staging → GPU)
//! 2. Shadow draw setup
//! 3. Lighting SSBO updates
//! 4. Point light uploads
//! 5. Sky params SSBO updates
//!
//! Writes to staging, scene buffer, indirect buffer, and shadow maps.
//! When mesh shaders are enabled, downstream passes read at TASK_SHADER_EXT
//! and MESH_SHADER_EXT stages — handled by RenderGraph barrier insertion.

use anyhow::Result;

use crate::renderer::graph::{AccessType, PassRecordContext, PassSetupContext, PassNode};
use crate::renderer::graph::resource::*;

pub struct UploadPass;

impl PassNode for UploadPass {
    fn name(&self) -> &'static str {
        "upload"
    }

    fn setup(&mut self, ctx: &mut PassSetupContext) {
        // Writes: staging→GPU copies target scene data, indirect buffers, shadow maps.
        // Also writes all meshlet buffers via record_chunk_delta_uploads + record_shadow_draw_setup.
        ctx.write(RES_STAGING, AccessType::TransferWrite);
        ctx.write(RES_SCENE_BUFFER, AccessType::TransferWrite);
        ctx.write(RES_INDIRECT_BUFFER, AccessType::TransferWrite);
        ctx.write(RES_SHADOW_MAPS, AccessType::TransferWrite);
        // Meshlet buffers written by record_chunk_delta_uploads (meshlet meta/vertex/tri)
        // and record_shadow_draw_setup (visible, indirect, count).
        ctx.write(RES_VISIBLE_MESHLETS, AccessType::TransferWrite);
        ctx.write(RES_MESHLET_INDIRECT, AccessType::TransferWrite);
        ctx.write(RES_MESHLET_COUNT, AccessType::TransferWrite);
    }

    fn record(&self, ctx: &mut PassRecordContext) -> Result<()> {
        let renderer = &mut *ctx.renderer;

        // Upload lighting and point light data (LGHT-01).
        {
            let current_frame = renderer.current_frame;
            if let Some(ls) = &renderer.lighting_state {
                ls.update(renderer, current_frame);
            }
            if let Some(plm) = &renderer.point_light_manager {
                plm.upload(renderer, current_frame);
            }
        }

        // Update sky params SSBO (LGHT-05).
        {
            let current_frame = renderer.current_frame;
            let sun_direction = renderer
                .lighting_state
                .as_ref()
                .map(|ls| ls.compute_sun_direction_pub())
                .unwrap_or([0.0, 1.0, 0.0]);
            let sun_color = renderer
                .lighting_state
                .as_ref()
                .map(|ls| ls.sun_color)
                .unwrap_or([1.0, 1.0, 1.0]);
            if let Some(sky) = &renderer.sky_renderer {
                sky.update(
                    renderer,
                    current_frame,
                    sun_direction,
                    sun_color,
                    ctx.camera_uniforms,
                );
            }
        }

        // Record staging→GPU copy commands for pending chunk deltas.
        renderer.record_chunk_delta_uploads(ctx.command_buffer)?;
        renderer.record_shadow_draw_setup(ctx.command_buffer)?;

        Ok(())
    }
}
