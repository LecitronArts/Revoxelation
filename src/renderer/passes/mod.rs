pub mod reistir;
pub mod svgf;
pub mod trace;

pub const DEFAULT_WORKGROUP_SIZE: [u32; 3] = [8, 8, 1];

#[derive(Debug, Clone, Copy)]
pub struct DispatchGrid {
    pub extent: [u32; 2],
    pub workgroup: [u32; 2],
}

impl DispatchGrid {
    pub fn new(width: u32, height: u32, workgroup_x: u32, workgroup_y: u32) -> Self {
        Self {
            extent: [width.max(1), height.max(1)],
            workgroup: [workgroup_x.max(1), workgroup_y.max(1)],
        }
    }

    pub fn groups(self) -> [u32; 2] {
        [
            self.extent[0].div_ceil(self.workgroup[0]),
            self.extent[1].div_ceil(self.workgroup[1]),
        ]
    }
}

pub struct PrepareContext<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub frame: &'a super::FrameContext,
    pub dispatch: DispatchGrid,
    pub trace_bind_group_ready: bool,
    pub reistir_bind_group_ready: bool,
    pub svgf_init_bind_group_ready: bool,
    pub svgf_resolve_bind_group_ready: bool,
    pub svgf_atrous_bind_group_count: usize,
    pub svgf_passes: usize,
}

impl PrepareContext<'_> {
    pub fn trace_ready(&self) -> bool {
        self.trace_bind_group_ready
    }

    pub fn reistir_ready(&self) -> bool {
        self.reistir_bind_group_ready
    }

    pub fn svgf_ready(&self) -> bool {
        self.svgf_init_bind_group_ready
            && self.svgf_resolve_bind_group_ready
            && self.svgf_atrous_bind_group_count >= self.svgf_passes
    }
}

pub struct RecordContext<'a> {
    pub encoder: &'a mut wgpu::CommandEncoder,
    pub frame: &'a super::FrameContext,
    pub dispatch: DispatchGrid,
    pub trace_pipeline: &'a wgpu::ComputePipeline,
    pub trace_bind_group: &'a wgpu::BindGroup,
    pub reistir_pipeline: &'a wgpu::ComputePipeline,
    pub reistir_bind_group: &'a wgpu::BindGroup,
    pub svgf_init_pipeline: &'a wgpu::ComputePipeline,
    pub svgf_init_bind_group: &'a wgpu::BindGroup,
    pub svgf_atrous_pipeline: &'a wgpu::ComputePipeline,
    pub svgf_atrous_bind_groups: &'a [wgpu::BindGroup],
    pub svgf_resolve_pipeline: &'a wgpu::ComputePipeline,
    pub svgf_resolve_bind_group: &'a wgpu::BindGroup,
    pub svgf_passes: usize,
}

impl RecordContext<'_> {
    pub fn groups(&self) -> [u32; 2] {
        self.dispatch.groups()
    }
}

pub trait RenderPass {
    fn label(&self) -> &'static str;
    fn prepare(&mut self, context: &PrepareContext<'_>);
    fn record(&self, context: &mut RecordContext<'_>);
}
