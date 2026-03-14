use super::pipeline_layouts::PipelineLayoutSetup;
use super::shader_modules::ShaderModuleSetup;

pub(super) struct ComputePipelineSetup {
    pub trace_pipeline: wgpu::ComputePipeline,
    pub reistir_pipeline: wgpu::ComputePipeline,
    pub svgf_init_pipeline: wgpu::ComputePipeline,
    pub svgf_atrous_pipeline: wgpu::ComputePipeline,
    pub svgf_resolve_pipeline: wgpu::ComputePipeline,
}

pub(super) fn create_compute_pipelines(
    device: &wgpu::Device,
    layouts: &PipelineLayoutSetup,
    shaders: &ShaderModuleSetup,
) -> ComputePipelineSetup {
    let trace_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("trace-compute-pipeline"),
        layout: Some(&layouts.trace_pipeline_layout),
        module: &shaders.trace_shader,
        entry_point: "main_cs",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });
    let reistir_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("reistir-compute-pipeline"),
        layout: Some(&layouts.trace_pipeline_layout),
        module: &shaders.reistir_shader,
        entry_point: "restir_spatial_cs",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });
    let svgf_init_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("svgf-init-pipeline"),
        layout: Some(&layouts.svgf_pipeline_layout),
        module: &shaders.svgf_shader,
        entry_point: "svgf_init_cs",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });
    let svgf_atrous_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("svgf-atrous-pipeline"),
        layout: Some(&layouts.svgf_pipeline_layout),
        module: &shaders.svgf_shader,
        entry_point: "svgf_atrous_cs",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });
    let svgf_resolve_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("svgf-resolve-pipeline"),
        layout: Some(&layouts.svgf_pipeline_layout),
        module: &shaders.svgf_shader,
        entry_point: "svgf_resolve_cs",
        compilation_options: wgpu::PipelineCompilationOptions::default(),
    });

    ComputePipelineSetup {
        trace_pipeline,
        reistir_pipeline,
        svgf_init_pipeline,
        svgf_atrous_pipeline,
        svgf_resolve_pipeline,
    }
}
