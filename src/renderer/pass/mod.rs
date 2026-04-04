//! Render pass modules for graph-integrated frame execution.
//!
//! Each pass implements the `PassNode` trait from the render graph,
//! declaring resource dependencies in `setup()` and recording commands
//! in `record()`. The RenderGraph compiles pass order and automatically
//! inserts pipeline barriers between write->read resource dependencies.

pub mod upload_pass;
pub mod shadow_pass;
pub mod cull_pass;
pub mod geometry_pass;
pub mod egui_pass;
pub mod hiz_pass;
pub mod ssao_pass;

// Re-export graph types used by pass implementations.
pub use super::graph::{PassNode, PassSetupContext, PassRecordContext};
