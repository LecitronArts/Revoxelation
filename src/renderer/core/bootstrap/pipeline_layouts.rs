use crate::renderer::protocol::bindings::{svgf, trace};

pub(super) struct PipelineLayoutSetup {
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub trace_pipeline_layout: wgpu::PipelineLayout,
    pub svgf_bind_group_layout: wgpu::BindGroupLayout,
    pub svgf_pipeline_layout: wgpu::PipelineLayout,
}

#[cfg(test)]
pub(crate) const fn trace_layout_binding_order() -> [u32; trace::COUNT] {
    trace::ORDER
}

#[cfg(test)]
pub(crate) const fn svgf_layout_binding_order() -> [u32; svgf::COUNT] {
    svgf::ORDER
}

pub(super) fn create_pipeline_layouts(
    device: &wgpu::Device,
    output_format: wgpu::TextureFormat,
) -> PipelineLayoutSetup {
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("trace-bind-group-layout"),
        entries: &trace_bind_group_layout_entries(output_format),
    });
    let trace_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("trace-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let svgf_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("svgf-bind-group-layout"),
            entries: &svgf_bind_group_layout_entries(output_format),
        });
    let svgf_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("svgf-pipeline-layout"),
        bind_group_layouts: &[&svgf_bind_group_layout],
        push_constant_ranges: &[],
    });

    PipelineLayoutSetup {
        bind_group_layout,
        trace_pipeline_layout,
        svgf_bind_group_layout,
        svgf_pipeline_layout,
    }
}

fn trace_bind_group_layout_entries(
    output_format: wgpu::TextureFormat,
) -> [wgpu::BindGroupLayoutEntry; trace::COUNT] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: trace::OUTPUT_VIEW,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: output_format,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::ACCUMULATION,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::CAMERA,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::TRACER_UNIFORM,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::VOXELS,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::CHUNK_META,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::CHUNK_MAP,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::EMISSIVE_VOXELS,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::DI_RESERVOIR_A,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::DI_RESERVOIR_B,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::PREVIOUS_CAMERA,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::SURFACE_HISTORY,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::EMISSIVE_CDF,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::EMISSIVE_REMAP,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::GI_RESERVOIR_A,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::GI_RESERVOIR_B,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: trace::IMPORTANCE_MAP,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D3,
                multisampled: false,
            },
            count: None,
        },
    ]
}

fn svgf_bind_group_layout_entries(
    output_format: wgpu::TextureFormat,
) -> [wgpu::BindGroupLayoutEntry; svgf::COUNT] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: svgf::OUTPUT_VIEW,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: output_format,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::ACCUMULATION,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::TRACER_UNIFORM,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::SURFACE_HISTORY,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::UNIFORM,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::PING,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::PONG,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::CAMERA,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::PREVIOUS_CAMERA,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: svgf::DEBUG_DATA,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use crate::renderer::protocol::bindings::{svgf, trace};
    use crate::renderer::resources::bind_groups::{
        svgf_bind_group_binding_order, trace_bind_group_binding_order,
    };

    use super::{
        svgf_bind_group_layout_entries, svgf_layout_binding_order, trace_bind_group_layout_entries,
        trace_layout_binding_order,
    };

    #[test]
    fn trace_layout_binding_order_matches_protocol_constants() {
        let actual = trace_bind_group_layout_entries(wgpu::TextureFormat::Rgba8Unorm)
            .iter()
            .map(|entry| entry.binding)
            .collect::<Vec<_>>();
        assert_eq!(actual, trace_layout_binding_order().to_vec());
    }

    #[test]
    fn svgf_layout_binding_order_matches_protocol_constants() {
        let actual = svgf_bind_group_layout_entries(wgpu::TextureFormat::Rgba8Unorm)
            .iter()
            .map(|entry| entry.binding)
            .collect::<Vec<_>>();
        assert_eq!(actual, svgf_layout_binding_order().to_vec());
    }

    #[test]
    fn trace_layout_binding_order_uses_protocol_count() {
        assert_eq!(trace_layout_binding_order().len(), trace::COUNT);
    }

    #[test]
    fn svgf_layout_binding_order_uses_protocol_count() {
        assert_eq!(svgf_layout_binding_order().len(), svgf::COUNT);
    }

    #[test]
    fn trace_layout_binding_order_matches_bind_group_builder_order() {
        assert_eq!(
            trace_layout_binding_order(),
            trace_bind_group_binding_order()
        );
    }

    #[test]
    fn svgf_layout_binding_order_matches_bind_group_builder_order() {
        assert_eq!(svgf_layout_binding_order(), svgf_bind_group_binding_order());
    }
}
