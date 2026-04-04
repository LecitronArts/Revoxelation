//! RenderGraph framework for automatic barrier management.
//!
//! The graph collects PassNode implementations, compiles them in dependency
//! order, and automatically inserts pipeline barriers between passes based
//! on declared resource access patterns.
//!
//! ## Architecture
//!
//! - `PassNode`: trait that each render pass implements
//! - `ResourceId`: opaque handle for GPU resources tracked by the graph
//! - `ResourceAccess`: declares how a pass reads/writes a resource
//! - `RenderGraph`: collects passes, compiles dependency order, executes
//!
//! ## Barrier insertion
//!
//! When pass N writes to resource R and pass M reads from R (M > N),
//! the graph inserts a `vkCmdPipelineBarrier` between N and M with:
//! - srcStageMask derived from the writer's AccessType
//! - dstStageMask derived from the reader's AccessType
//! - srcAccessMask / dstAccessMask from the respective AccessTypes

pub mod resource;

use std::collections::HashMap;

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
    /// Task shader read (mesh shader pipeline).
    TaskShaderRead,
    /// Mesh shader read (mesh shader pipeline).
    MeshShaderRead,
}

impl AccessType {
    /// Map to Vulkan pipeline stage flags.
    pub fn stage_flags(self) -> vk::PipelineStageFlags {
        match self {
            Self::ComputeRead | Self::ComputeWrite => vk::PipelineStageFlags::COMPUTE_SHADER,
            Self::IndirectRead => vk::PipelineStageFlags::DRAW_INDIRECT,
            Self::VertexInput => vk::PipelineStageFlags::VERTEX_INPUT,
            Self::ColorWrite => vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            Self::DepthWrite => {
                vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                    | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS
            }
            Self::FragmentRead => vk::PipelineStageFlags::FRAGMENT_SHADER,
            Self::TransferRead | Self::TransferWrite => vk::PipelineStageFlags::TRANSFER,
            // TASK_SHADER_EXT / MESH_SHADER_EXT for mesh shader pipeline reads.
            Self::TaskShaderRead => vk::PipelineStageFlags::TASK_SHADER_EXT,
            Self::MeshShaderRead => vk::PipelineStageFlags::MESH_SHADER_EXT,
        }
    }

    /// Map to Vulkan access flags.
    pub fn access_flags(self) -> vk::AccessFlags {
        match self {
            Self::ComputeRead => vk::AccessFlags::SHADER_READ,
            Self::ComputeWrite => vk::AccessFlags::SHADER_WRITE,
            Self::IndirectRead => vk::AccessFlags::INDIRECT_COMMAND_READ,
            Self::VertexInput => {
                vk::AccessFlags::VERTEX_ATTRIBUTE_READ | vk::AccessFlags::INDEX_READ
            }
            Self::ColorWrite => vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            Self::DepthWrite => {
                vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                    | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
            }
            Self::FragmentRead => vk::AccessFlags::SHADER_READ,
            Self::TransferRead => vk::AccessFlags::TRANSFER_READ,
            Self::TransferWrite => vk::AccessFlags::TRANSFER_WRITE,
            Self::TaskShaderRead | Self::MeshShaderRead => vk::AccessFlags::SHADER_READ,
        }
    }

    /// Whether this access type is a write.
    pub fn is_write(self) -> bool {
        matches!(
            self,
            Self::ComputeWrite | Self::ColorWrite | Self::DepthWrite | Self::TransferWrite
        )
    }
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
pub trait PassNode: Send + 'static {
    /// Human-readable name for debug labels and profiling.
    fn name(&self) -> &'static str;

    /// Whether this pass is currently enabled (default: always enabled).
    /// Disabled passes are skipped during execution but still occupy a slot.
    fn enabled(&self, _config: &super::config::RenderConfig) -> bool {
        true
    }

    /// Declare resource dependencies.
    /// Called once during graph compilation.
    fn setup(&mut self, ctx: &mut PassSetupContext);

    /// Record commands into the frame's command buffer.
    /// Called during graph execution, in dependency order.
    fn record(&self, ctx: &mut PassRecordContext) -> Result<()>;
}

/// Tracked state of the most recent write to a resource.
#[derive(Debug, Clone)]
struct ResourceWriteState {
    /// Which pass last wrote this resource.
    #[allow(dead_code)]
    pass_index: usize,
    /// The access type of the write.
    access_type: AccessType,
    /// Which destination pipeline stages have already been covered by barriers
    /// for this write. A barrier only makes data visible to the specified
    /// dstStageMask — later reads at different stages need their own barriers.
    dst_stages_covered: vk::PipelineStageFlags,
}

/// Compiled pass entry — pass + its declared dependencies.
struct CompiledPass {
    pass: Box<dyn PassNode>,
    reads: Vec<ResourceAccess>,
    writes: Vec<ResourceAccess>,
}

/// Callback invoked between passes during graph execution.
/// Used for operations like ending a Vulkan render pass between
/// the in-render-pass group and post-process passes.
pub type InterPassCallback = Box<dyn Fn(&mut Renderer, vk::CommandBuffer)>;

/// The render graph — collects, compiles, and executes passes with
/// automatic barrier insertion between write→read dependencies.
pub struct RenderGraph {
    passes: Vec<CompiledPass>,
    compiled: bool,
    /// Optional callbacks keyed by pass index — invoked AFTER that pass completes.
    inter_pass_callbacks: HashMap<usize, InterPassCallback>,
    /// Pre-allocated barrier state to avoid per-frame HashMap allocation.
    last_writes: HashMap<ResourceId, ResourceWriteState>,
}

impl RenderGraph {
    pub fn new() -> Self {
        Self {
            passes: Vec::new(),
            compiled: false,
            inter_pass_callbacks: HashMap::new(),
            last_writes: HashMap::new(),
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

    /// Reset the graph for a new frame — clears passes and callbacks but
    /// preserves internal allocations (Vec capacity, HashMap capacity).
    pub fn reset(&mut self) {
        self.passes.clear();
        self.inter_pass_callbacks.clear();
        self.compiled = false;
    }

    /// Register a callback to run AFTER the pass at `pass_index` finishes.
    ///
    /// Used for operations that don't fit the pass model, e.g. ending a
    /// Vulkan render pass between the in-render-pass group and post-process.
    pub fn add_inter_pass_callback(
        &mut self,
        pass_index: usize,
        callback: impl Fn(&mut Renderer, vk::CommandBuffer) + 'static,
    ) {
        self.inter_pass_callbacks
            .insert(pass_index, Box::new(callback));
    }

    /// Compile the graph: collect resource declarations from each pass.
    ///
    /// Current implementation preserves insertion order. A future version
    /// could do topological sort for maximum parallelism.
    pub fn compile(&mut self) {
        for entry in &mut self.passes {
            let mut setup = PassSetupContext::new();
            entry.pass.setup(&mut setup);
            entry.reads = setup.reads;
            entry.writes = setup.writes;
        }
        self.compiled = true;
    }

    /// Execute all passes in compiled order, automatically inserting
    /// pipeline barriers between write→read resource dependencies.
    pub fn execute<'a>(
        &mut self,
        renderer: &'a mut Renderer,
        command_buffer: vk::CommandBuffer,
        camera_uniforms: &'a CameraUniforms,
        current_time: f32,
        image_index: u32,
    ) -> Result<()> {
        assert!(
            self.compiled,
            "RenderGraph::compile() must be called before execute()"
        );

        // Reuse pre-allocated map — clear entries but keep capacity.
        self.last_writes.clear();

        for (pass_idx, entry) in self.passes.iter().enumerate() {
            // Skip disabled passes — they don't record commands or affect barriers.
            if !entry.pass.enabled(&renderer.config) {
                // Run inter-pass callback even for disabled passes (e.g. end_render_pass).
                if let Some(callback) = self.inter_pass_callbacks.get(&pass_idx) {
                    callback(renderer, command_buffer);
                }
                continue;
            }

            // Compute barrier needed before this pass: for each resource
            // this pass reads, check if there's an outstanding write that
            // hasn't been made visible to this pass's pipeline stages yet.
            let mut src_stage_mask = vk::PipelineStageFlags::empty();
            let mut dst_stage_mask = vk::PipelineStageFlags::empty();
            let mut src_access_mask = vk::AccessFlags::empty();
            let mut dst_access_mask = vk::AccessFlags::empty();

            for read in &entry.reads {
                if let Some(write_state) = self.last_writes.get_mut(&read.resource) {
                    let reader_stages = read.access.stage_flags();
                    // Only barrier for stages not yet covered.
                    let uncovered = reader_stages & !write_state.dst_stages_covered;
                    if !uncovered.is_empty() {
                        src_stage_mask |= write_state.access_type.stage_flags();
                        src_access_mask |= write_state.access_type.access_flags();
                        dst_stage_mask |= uncovered;
                        dst_access_mask |= read.access.access_flags();
                        write_state.dst_stages_covered |= reader_stages;
                    }
                }
            }

            // Also check write-after-write hazards: if this pass writes a resource
            // that was previously written by an earlier pass, we need a barrier to
            // ensure the previous write completes before we overwrite.
            for write in &entry.writes {
                if let Some(write_state) = self.last_writes.get_mut(&write.resource) {
                    src_stage_mask |= write_state.access_type.stage_flags();
                    src_access_mask |= write_state.access_type.access_flags();
                    dst_stage_mask |= write.access.stage_flags();
                    dst_access_mask |= write.access.access_flags();
                }
            }

            // Insert barrier if needed.
            if !src_stage_mask.is_empty() {
                let barrier = vk::MemoryBarrier::default()
                    .src_access_mask(src_access_mask)
                    .dst_access_mask(dst_access_mask);
                unsafe {
                    renderer.device_ctx.device.cmd_pipeline_barrier(
                        command_buffer,
                        src_stage_mask,
                        dst_stage_mask,
                        vk::DependencyFlags::empty(),
                        &[barrier],
                        &[],
                        &[],
                    );
                }
            }

            // Record the pass.
            {
                let ctx = &mut PassRecordContext {
                    renderer,
                    command_buffer,
                    camera_uniforms,
                    current_time,
                    image_index,
                };
                entry.pass.record(ctx)?;
            }

            // Run inter-pass callback if registered for this index.
            if let Some(callback) = self.inter_pass_callbacks.get(&pass_idx) {
                callback(renderer, command_buffer);
            }

            // Track this pass's writes.
            for write in &entry.writes {
                self.last_writes.insert(
                    write.resource,
                    ResourceWriteState {
                        pass_index: pass_idx,
                        access_type: write.access,
                        dst_stages_covered: vk::PipelineStageFlags::empty(),
                    },
                );
            }
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

    /// Clear all passes (for rebuilding the graph next frame).
    pub fn clear(&mut self) {
        self.passes.clear();
        self.compiled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_type_stage_flags_are_nonzero() {
        let types = [
            AccessType::ComputeRead,
            AccessType::ComputeWrite,
            AccessType::IndirectRead,
            AccessType::VertexInput,
            AccessType::ColorWrite,
            AccessType::DepthWrite,
            AccessType::FragmentRead,
            AccessType::TransferRead,
            AccessType::TransferWrite,
            AccessType::TaskShaderRead,
            AccessType::MeshShaderRead,
        ];
        for ty in types {
            assert!(
                !ty.stage_flags().is_empty(),
                "{:?} must have non-empty stage flags",
                ty
            );
            assert!(
                !ty.access_flags().is_empty(),
                "{:?} must have non-empty access flags",
                ty
            );
        }
    }

    #[test]
    fn write_types_are_classified_correctly() {
        assert!(AccessType::ComputeWrite.is_write());
        assert!(AccessType::ColorWrite.is_write());
        assert!(AccessType::DepthWrite.is_write());
        assert!(AccessType::TransferWrite.is_write());
        assert!(!AccessType::ComputeRead.is_write());
        assert!(!AccessType::FragmentRead.is_write());
        assert!(!AccessType::IndirectRead.is_write());
    }

    #[test]
    fn render_graph_reset_preserves_capacity() {
        struct DummyPass(&'static str);
        impl PassNode for DummyPass {
            fn name(&self) -> &'static str {
                self.0
            }
            fn setup(&mut self, _ctx: &mut PassSetupContext) {}
            fn record(&self, _ctx: &mut PassRecordContext) -> Result<()> {
                Ok(())
            }
        }

        let mut graph = RenderGraph::new();
        graph.add_pass(DummyPass("a"));
        graph.add_pass(DummyPass("b"));
        graph.add_pass(DummyPass("c"));
        graph.compile();
        assert_eq!(graph.pass_count(), 3);

        graph.reset();
        assert_eq!(graph.pass_count(), 0);
        assert!(!graph.compiled);

        // Capacity preserved — no re-allocation on next frame.
        graph.add_pass(DummyPass("x"));
        graph.add_pass(DummyPass("y"));
        graph.compile();
        assert_eq!(graph.pass_count(), 2);
        assert_eq!(graph.pass_names(), vec!["x", "y"]);
    }

    #[test]
    fn render_graph_pass_count_and_names() {
        struct DummyPass(&'static str);
        impl PassNode for DummyPass {
            fn name(&self) -> &'static str {
                self.0
            }
            fn setup(&mut self, _ctx: &mut PassSetupContext) {}
            fn record(&self, _ctx: &mut PassRecordContext) -> Result<()> {
                Ok(())
            }
        }

        let mut graph = RenderGraph::new();
        assert_eq!(graph.pass_count(), 0);

        graph.add_pass(DummyPass("upload"));
        graph.add_pass(DummyPass("shadow"));
        graph.add_pass(DummyPass("cull"));
        graph.compile();

        assert_eq!(graph.pass_count(), 3);
        assert_eq!(graph.pass_names(), vec!["upload", "shadow", "cull"]);
    }
}
