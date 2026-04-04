//! Runtime render configuration (REFAC-01b).
//!
//! Groups egui-adjustable toggles and configuration structs into a single
//! sub-struct. This replaces 10+ scattered fields on the Renderer god-object
//! with one coherent `RenderConfig` that can be passed by reference to
//! sub-systems without borrowing the entire Renderer.

use super::shadow::ShadowConfig;
use super::ssao::SsaoConfig;

/// Runtime-adjustable rendering configuration.
///
/// All fields are egui-exposed or initialization-set. Grouped here so
/// submit.rs / pass code can take `&RenderConfig` instead of `&Renderer`.
#[derive(Clone, Debug)]
pub struct RenderConfig {
    // -- Meshlet culling toggles (egui checkboxes) -------------------------
    /// Enable backface culling in meshlet cull pass.
    pub meshlet_cull_backface: bool,
    /// Enable frustum culling in meshlet cull pass.
    pub meshlet_cull_frustum: bool,
    /// Enable Hi-Z occlusion culling in meshlet cull pass.
    pub meshlet_cull_hiz: bool,

    // -- Rendering path selection ------------------------------------------
    /// Whether the active meshlet_pipeline is a MeshShaderPath (skips meshlet_cull.comp).
    /// Set automatically based on GPU capability — not egui-controllable.
    pub use_mesh_shader_path: bool,
    /// Runtime toggle: use meshlet rendering (true) or legacy per-chunk path (false).
    pub use_meshlet_rendering: bool,

    // -- LOD / quality knobs -----------------------------------------------
    /// SSE threshold in pixels for LOD selection (MSHL-05). Default 2.0.
    pub sse_threshold: f32,

    // -- Shadow configuration (LGHT-02) ------------------------------------
    /// Cascaded shadow map configuration (egui-adjustable).
    pub shadow: ShadowConfig,

    // -- SSAO configuration (LGHT-03) --------------------------------------
    /// Screen-space ambient occlusion configuration (egui-adjustable).
    pub ssao: SsaoConfig,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            meshlet_cull_backface: true,
            meshlet_cull_frustum: true,
            meshlet_cull_hiz: true,
            use_mesh_shader_path: false,
            use_meshlet_rendering: false,
            sse_threshold: 2.0,
            shadow: ShadowConfig::default(),
            ssao: SsaoConfig::default(),
        }
    }
}
