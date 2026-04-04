//! RenderGraph framework for automatic barrier management (Phase 3).
//!
//! The graph collects PassNode implementations, topologically sorts them
//! based on declared resource dependencies, and automatically inserts
//! pipeline barriers between passes.
//!
//! ## Architecture
//!
//! - `PassNode`: trait that each render pass implements
//! - `ResourceId`: opaque handle for GPU resources tracked by the graph
//! - `ResourceAccess`: declares how a pass reads/writes a resource
//! - `RenderGraph`: collects passes, compiles dependency order, executes
//!
//! ## Current status
//!
//! Phase 3 skeleton — trait definitions and compile/execute stubs.
//! Actual automatic barrier insertion will be implemented in Phase 4.

pub mod resource;

use anyhow::Result;
use ash::vk;

use super::Renderer;
use super::camera::CameraUniforms;

/// Opaque resource identifier for graph-tracked GPU resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(pub u32);

/// How a pass accesses a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    /// Read from compute shader.
    ComputeRead,
    /// Write from compute shader.
    ComputeWrite,
    /// Read as indirect command buffer.
    IndirectRead,
    /// Vertex/index input.
    VertexInput,
    /// Color attachment write.
    ColorWrite,
    /// Depth attachment write.
    DepthWrite,
    /// Fragment shader read (sampled image).
    FragmentRead,
    /// Transfer source.
    TransferRead,
    /// Transfer destination.
    TransferWrite,
}

/// A declared resource dependency for a pass.
#[derive(Debug, Clone)]
pub struct ResourceAccess {
    pub resource: ResourceId,
    pub access: AccessType,
}

/// Context passed during pass setup — passes declare their resource dependencies here.
pub struct PassSetupContext {
    pub reads: Vec<ResourceAccess>,
    pub writes: Vec<ResourceAccess>,
}

impl PassSetupContext {
    pub fn new() -> Self {
        Self {
            reads: Vec::new(),
            writes: Vec::new(),
        }
    }

    pub fn read(&mut self, resource: ResourceId, access: AccessType) {
        self.reads.push(ResourceAccess { resource, access });
    }

    pub fn write(&mut self, resource: ResourceId, access: AccessType) {
        self.writes.push(ResourceAccess { resource, access });
    }
}

/// Context passed during pass recording — holds command buffer and renderer.
pub struct PassRecordContext<'a> {
    pub renderer: &'a mut Renderer,
    pub command_buffer: vk::CommandBuffer,
    pub camera_uniforms: &'a CameraUniforms,
    pub current_time: f32,
    pub image_index: u32,
}

/// Trait that each render pass implements for graph integration.
///
/// In Phase 4, submit_frame will be replaced by:
/// ```ignore
/// graph.add_pass(UploadPass);
/// graph.add_pass(ShadowPass);
/// graph.add_pass(CullPass);
/// // ...
/// graph.compile();
/// graph.execute(renderer, cmd, camera, time)?;
/// ```
pub trait PassNode: Send + 'static {
    /// Human-readable name for debug labels and profiling.
    fn name(&self) -> &'static str;

    /// Declare resource dependencies.
    /// Called once during graph compilation.
    fn setup(&mut self, ctx: &mut PassSetupContext);

    /// Record commands into the frame's command buffer.
    /// Called during graph execution, in dependency order.
    fn record(&self, ctx: &mut PassRecordContext) -> Result<()>;
}

/// Compiled pass entry — pass + its declared dependencies.
struct CompiledPass {
    pass: Box<dyn PassNode>,
    #[allow(dead_code)]
    reads: Vec<ResourceAccess>,
    #[allow(dead_code)]
    writes: Vec<ResourceAccess>,
}

/// The render graph — collects, compiles, and executes passes.
pub struct RenderGraph {
    passes: Vec<CompiledPass>,
    compiled: bool,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            compiled: false,
        }
    }

    /// Add a pass to the graph. Must call `compile()` before `execute()`.
    pub fn add_pass(&mut self, pass: impl PassNode) {
        self.passes.push(CompiledPass {
            pass: Box::new(pass),
            reads: Vec::new(),
            writes: Vec::new(),
        });
        self.compiled = false;
    }

    /// Compile the graph: collect resource declarations, topologically sort,
    /// and compute barrier insertion points.
    ///
    /// Current implementation: linear order (preserving add order).
    /// Phase 4 will add proper topological sort + automatic barriers.
    pub fn compile(&mut self) {
        for entry in &mut self.passes {
            let mut setup = PassSetupContext::new();
            entry.pass.setup(&mut setup);
            entry.reads = setup.reads;
            entry.writes = setup.writes;
        }
        // Phase 4: topological sort based on read/write dependencies.
        // For now, respect insertion order.
        self.compiled = true;
    }

    /// Execute all passes in compiled order.
    ///
    /// Phase 4 will automatically insert barriers between passes based
    /// on their declared resource dependencies.
    pub fn execute<'a>(
        &self,
        renderer: &'a mut Renderer,
        command_buffer: vk::CommandBuffer,
        camera_uniforms: &'a CameraUniforms,
        current_time: f32,
        image_index: u32,
    ) -> Result<()> {
        assert!(self.compiled, "RenderGraph::compile() must be called before execute()");

        for entry in &self.passes {
            // Phase 4: insert barrier here based on previous pass's writes
            // vs this pass's reads.
            let ctx = &mut PassRecordContext {
                renderer,
                command_buffer,
                camera_uniforms,
                current_time,
                image_index,
            };
            entry.pass.record(ctx)?;
        }

        Ok(())
    }

    /// Number of passes in the graph.
    pub fn pass_count(&self) -> usize {
        self.passes.len()
    }

    /// List pass names (for debug/profiling).
    pub fn pass_names(&self) -> Vec<&'static str> {
        self.passes.iter().map(|p| p.pass.name()).collect()
    }
}
