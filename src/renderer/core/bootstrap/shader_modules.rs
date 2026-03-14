fn storage_format_token(format: wgpu::TextureFormat) -> &'static str {
    match format {
        wgpu::TextureFormat::Rgba8Unorm => "rgba8unorm",
        wgpu::TextureFormat::Bgra8Unorm => "bgra8unorm",
        _ => "rgba8unorm",
    }
}

pub(super) struct ShaderModuleSetup {
    pub trace_shader: wgpu::ShaderModule,
    pub reistir_shader: wgpu::ShaderModule,
    pub svgf_shader: wgpu::ShaderModule,
}

pub(super) fn create_shader_modules(
    device: &wgpu::Device,
    output_format: wgpu::TextureFormat,
) -> ShaderModuleSetup {
    let trace_shader_source = include_str!("../../../shaders/trace.wgsl").replace(
        "__TRACE_STORAGE_FORMAT__",
        storage_format_token(output_format),
    );
    let trace_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("trace-wgsl"),
        source: wgpu::ShaderSource::Wgsl(trace_shader_source.into()),
    });

    let reistir_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("reistir-wgsl"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../../shaders/reistir.wgsl").into()),
    });

    let svgf_shader_source = include_str!("../../../shaders/svgf.wgsl").replace(
        "__SVGF_STORAGE_FORMAT__",
        storage_format_token(output_format),
    );
    let svgf_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("svgf-wgsl"),
        source: wgpu::ShaderSource::Wgsl(svgf_shader_source.into()),
    });

    ShaderModuleSetup {
        trace_shader,
        reistir_shader,
        svgf_shader,
    }
}
