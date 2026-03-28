//! PipelineSet sub-struct — pipeline objects and caches (REFAC-01).
//!
//! Groups all graphics and compute pipeline handles together. These are
//! created after core infrastructure and destroyed before it.

use super::{cull_pipeline, hiz, mesh_pipeline, pipeline_cache};

/// Pipeline objects and caches (REFAC-01).
///
/// Logical view into the renderer's pipeline handles. Used as a borrow-friendly
/// reference bundle when functions need pipeline access without borrowing
/// the entire Renderer.
#[allow(dead_code)]
pub struct PipelineSet<'a> {
    pub mesh_pipeline: Option<&'a mesh_pipeline::ChunkMeshPipeline>,
    pub cull_pipeline: Option<&'a cull_pipeline::ChunkCullPipeline>,
    pub meshlet_cull_pipeline: Option<&'a cull_pipeline::MeshletCullPipeline>,
    pub meshlet_pipeline: Option<&'a dyn mesh_pipeline::MeshletPipeline>,
    pub pipeline_cache: Option<&'a pipeline_cache::PipelineCache>,
    pub hiz_pyramid: Option<&'a hiz::HiZPyramid>,
}
