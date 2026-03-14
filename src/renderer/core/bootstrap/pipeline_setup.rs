use super::compute_pipelines::create_compute_pipelines;
use super::pipeline_layouts::create_pipeline_layouts;
use super::shader_modules::create_shader_modules;

pub(super) struct PipelineSetupOutput {
    pub trace_pipeline: wgpu::ComputePipeline,
    pub reistir_pipeline: wgpu::ComputePipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub svgf_init_pipeline: wgpu::ComputePipeline,
    pub svgf_atrous_pipeline: wgpu::ComputePipeline,
    pub svgf_resolve_pipeline: wgpu::ComputePipeline,
    pub svgf_bind_group_layout: wgpu::BindGroupLayout,
}

pub(super) fn setup_pipelines(
    device: &wgpu::Device,
    output_format: wgpu::TextureFormat,
) -> PipelineSetupOutput {
    let layouts = create_pipeline_layouts(device, output_format);
    let shaders = create_shader_modules(device, output_format);
    let pipelines = create_compute_pipelines(device, &layouts, &shaders);

    PipelineSetupOutput {
        trace_pipeline: pipelines.trace_pipeline,
        reistir_pipeline: pipelines.reistir_pipeline,
        bind_group_layout: layouts.bind_group_layout,
        svgf_init_pipeline: pipelines.svgf_init_pipeline,
        svgf_atrous_pipeline: pipelines.svgf_atrous_pipeline,
        svgf_resolve_pipeline: pipelines.svgf_resolve_pipeline,
        svgf_bind_group_layout: layouts.svgf_bind_group_layout,
    }
}
