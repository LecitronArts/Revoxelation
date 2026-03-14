use crate::renderer::protocol::bindings::{svgf, trace};

use super::restir_storage::RestirBindings;

pub struct BindGroupExecutorInput<'a> {
    pub device: &'a wgpu::Device,
    pub trace_layout: &'a wgpu::BindGroupLayout,
    pub svgf_layout: &'a wgpu::BindGroupLayout,
    pub output_view: &'a wgpu::TextureView,
    pub accumulation_buffer: &'a wgpu::Buffer,
    pub camera_buffer: &'a wgpu::Buffer,
    pub previous_camera_buffer: &'a wgpu::Buffer,
    pub tracer_uniform_buffer: &'a wgpu::Buffer,
    pub voxel_buffer: &'a wgpu::Buffer,
    pub chunk_meta_buffer: &'a wgpu::Buffer,
    pub chunk_map_buffer: &'a wgpu::Buffer,
    pub emissive_voxel_buffer: &'a wgpu::Buffer,
    pub emissive_cdf_buffer: &'a wgpu::Buffer,
    pub emissive_remap_buffer: &'a wgpu::Buffer,
    pub importance_map_view: &'a wgpu::TextureView,
    pub svgf_init_uniform_buffer: &'a wgpu::Buffer,
    pub svgf_resolve_uniform_buffer: &'a wgpu::Buffer,
    pub svgf_atrous_uniform_buffers: &'a [wgpu::Buffer],
    pub svgf_ping_buffer: &'a wgpu::Buffer,
    pub svgf_pong_buffer: &'a wgpu::Buffer,
    pub svgf_debug_buffer: &'a wgpu::Buffer,
    pub restir_bindings: RestirBindings<'a>,
}

pub struct RebuiltBindGroups {
    pub trace_bind_group: wgpu::BindGroup,
    pub svgf_init_bind_group: wgpu::BindGroup,
    pub svgf_atrous_bind_groups: Vec<wgpu::BindGroup>,
    pub svgf_resolve_bind_group: wgpu::BindGroup,
}

#[cfg(test)]
pub(crate) const fn trace_bind_group_binding_order() -> [u32; trace::COUNT] {
    trace::ORDER
}

#[cfg(test)]
pub(crate) const fn svgf_bind_group_binding_order() -> [u32; svgf::COUNT] {
    svgf::ORDER
}

pub fn rebuild_bind_groups(input: BindGroupExecutorInput<'_>) -> RebuiltBindGroups {
    let trace_bind_group = create_trace_bind_group(
        input.device,
        input.trace_layout,
        input.output_view,
        input.accumulation_buffer,
        input.camera_buffer,
        input.previous_camera_buffer,
        input.tracer_uniform_buffer,
        input.voxel_buffer,
        input.chunk_meta_buffer,
        input.chunk_map_buffer,
        input.emissive_voxel_buffer,
        input.emissive_cdf_buffer,
        input.emissive_remap_buffer,
        input.restir_bindings.di_a,
        input.restir_bindings.di_b,
        input.restir_bindings.gi_a,
        input.restir_bindings.gi_b,
        input.restir_bindings.surface_history,
        input.importance_map_view,
    );
    let svgf_init_bind_group = create_svgf_bind_group(
        input.device,
        input.svgf_layout,
        input.output_view,
        input.accumulation_buffer,
        input.tracer_uniform_buffer,
        input.restir_bindings.surface_history,
        input.camera_buffer,
        input.previous_camera_buffer,
        input.svgf_init_uniform_buffer,
        input.svgf_ping_buffer,
        input.svgf_pong_buffer,
        input.svgf_debug_buffer,
    );
    let svgf_atrous_bind_groups = input
        .svgf_atrous_uniform_buffers
        .iter()
        .map(|uniform_buffer| {
            create_svgf_bind_group(
                input.device,
                input.svgf_layout,
                input.output_view,
                input.accumulation_buffer,
                input.tracer_uniform_buffer,
                input.restir_bindings.surface_history,
                input.camera_buffer,
                input.previous_camera_buffer,
                uniform_buffer,
                input.svgf_ping_buffer,
                input.svgf_pong_buffer,
                input.svgf_debug_buffer,
            )
        })
        .collect();
    let svgf_resolve_bind_group = create_svgf_bind_group(
        input.device,
        input.svgf_layout,
        input.output_view,
        input.accumulation_buffer,
        input.tracer_uniform_buffer,
        input.restir_bindings.surface_history,
        input.camera_buffer,
        input.previous_camera_buffer,
        input.svgf_resolve_uniform_buffer,
        input.svgf_ping_buffer,
        input.svgf_pong_buffer,
        input.svgf_debug_buffer,
    );

    RebuiltBindGroups {
        trace_bind_group,
        svgf_init_bind_group,
        svgf_atrous_bind_groups,
        svgf_resolve_bind_group,
    }
}

pub const fn should_rebuild_bind_groups(
    world_resources_changed: bool,
    surface_resources_changed: bool,
) -> bool {
    world_resources_changed || surface_resources_changed
}

#[allow(clippy::too_many_arguments)]
fn create_trace_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    output_view: &wgpu::TextureView,
    accumulation_buffer: &wgpu::Buffer,
    camera_buffer: &wgpu::Buffer,
    previous_camera_buffer: &wgpu::Buffer,
    tracer_uniform_buffer: &wgpu::Buffer,
    voxel_buffer: &wgpu::Buffer,
    chunk_meta_buffer: &wgpu::Buffer,
    chunk_map_buffer: &wgpu::Buffer,
    emissive_voxel_buffer: &wgpu::Buffer,
    emissive_cdf_buffer: &wgpu::Buffer,
    emissive_remap_buffer: &wgpu::Buffer,
    reservoir_a_buffer: &wgpu::Buffer,
    reservoir_b_buffer: &wgpu::Buffer,
    gi_reservoir_a_buffer: &wgpu::Buffer,
    gi_reservoir_b_buffer: &wgpu::Buffer,
    surface_history_buffer: &wgpu::Buffer,
    importance_map_view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("trace-bind-group"),
        layout,
        entries: &trace_bind_group_entries(
            output_view,
            accumulation_buffer,
            camera_buffer,
            previous_camera_buffer,
            tracer_uniform_buffer,
            voxel_buffer,
            chunk_meta_buffer,
            chunk_map_buffer,
            emissive_voxel_buffer,
            emissive_cdf_buffer,
            emissive_remap_buffer,
            reservoir_a_buffer,
            reservoir_b_buffer,
            gi_reservoir_a_buffer,
            gi_reservoir_b_buffer,
            surface_history_buffer,
            importance_map_view,
        ),
    })
}

#[allow(clippy::too_many_arguments)]
fn trace_bind_group_entries<'a>(
    output_view: &'a wgpu::TextureView,
    accumulation_buffer: &'a wgpu::Buffer,
    camera_buffer: &'a wgpu::Buffer,
    previous_camera_buffer: &'a wgpu::Buffer,
    tracer_uniform_buffer: &'a wgpu::Buffer,
    voxel_buffer: &'a wgpu::Buffer,
    chunk_meta_buffer: &'a wgpu::Buffer,
    chunk_map_buffer: &'a wgpu::Buffer,
    emissive_voxel_buffer: &'a wgpu::Buffer,
    emissive_cdf_buffer: &'a wgpu::Buffer,
    emissive_remap_buffer: &'a wgpu::Buffer,
    reservoir_a_buffer: &'a wgpu::Buffer,
    reservoir_b_buffer: &'a wgpu::Buffer,
    gi_reservoir_a_buffer: &'a wgpu::Buffer,
    gi_reservoir_b_buffer: &'a wgpu::Buffer,
    surface_history_buffer: &'a wgpu::Buffer,
    importance_map_view: &'a wgpu::TextureView,
) -> [wgpu::BindGroupEntry<'a>; trace::COUNT] {
    [
        wgpu::BindGroupEntry {
            binding: trace::OUTPUT_VIEW,
            resource: wgpu::BindingResource::TextureView(output_view),
        },
        wgpu::BindGroupEntry {
            binding: trace::ACCUMULATION,
            resource: accumulation_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::CAMERA,
            resource: camera_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::TRACER_UNIFORM,
            resource: tracer_uniform_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::VOXELS,
            resource: voxel_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::CHUNK_META,
            resource: chunk_meta_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::CHUNK_MAP,
            resource: chunk_map_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::EMISSIVE_VOXELS,
            resource: emissive_voxel_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::DI_RESERVOIR_A,
            resource: reservoir_a_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::DI_RESERVOIR_B,
            resource: reservoir_b_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::PREVIOUS_CAMERA,
            resource: previous_camera_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::SURFACE_HISTORY,
            resource: surface_history_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::EMISSIVE_CDF,
            resource: emissive_cdf_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::EMISSIVE_REMAP,
            resource: emissive_remap_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::GI_RESERVOIR_A,
            resource: gi_reservoir_a_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::GI_RESERVOIR_B,
            resource: gi_reservoir_b_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: trace::IMPORTANCE_MAP,
            resource: wgpu::BindingResource::TextureView(importance_map_view),
        },
    ]
}

#[allow(clippy::too_many_arguments)]
fn create_svgf_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    output_view: &wgpu::TextureView,
    accumulation_buffer: &wgpu::Buffer,
    tracer_uniform_buffer: &wgpu::Buffer,
    surface_history_buffer: &wgpu::Buffer,
    camera_buffer: &wgpu::Buffer,
    previous_camera_buffer: &wgpu::Buffer,
    svgf_uniform_buffer: &wgpu::Buffer,
    svgf_ping_buffer: &wgpu::Buffer,
    svgf_pong_buffer: &wgpu::Buffer,
    svgf_debug_buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("svgf-bind-group"),
        layout,
        entries: &svgf_bind_group_entries(
            output_view,
            accumulation_buffer,
            tracer_uniform_buffer,
            surface_history_buffer,
            camera_buffer,
            previous_camera_buffer,
            svgf_uniform_buffer,
            svgf_ping_buffer,
            svgf_pong_buffer,
            svgf_debug_buffer,
        ),
    })
}

#[allow(clippy::too_many_arguments)]
fn svgf_bind_group_entries<'a>(
    output_view: &'a wgpu::TextureView,
    accumulation_buffer: &'a wgpu::Buffer,
    tracer_uniform_buffer: &'a wgpu::Buffer,
    surface_history_buffer: &'a wgpu::Buffer,
    camera_buffer: &'a wgpu::Buffer,
    previous_camera_buffer: &'a wgpu::Buffer,
    svgf_uniform_buffer: &'a wgpu::Buffer,
    svgf_ping_buffer: &'a wgpu::Buffer,
    svgf_pong_buffer: &'a wgpu::Buffer,
    svgf_debug_buffer: &'a wgpu::Buffer,
) -> [wgpu::BindGroupEntry<'a>; svgf::COUNT] {
    [
        wgpu::BindGroupEntry {
            binding: svgf::OUTPUT_VIEW,
            resource: wgpu::BindingResource::TextureView(output_view),
        },
        wgpu::BindGroupEntry {
            binding: svgf::ACCUMULATION,
            resource: accumulation_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::TRACER_UNIFORM,
            resource: tracer_uniform_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::SURFACE_HISTORY,
            resource: surface_history_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::UNIFORM,
            resource: svgf_uniform_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::PING,
            resource: svgf_ping_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::PONG,
            resource: svgf_pong_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::CAMERA,
            resource: camera_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::PREVIOUS_CAMERA,
            resource: previous_camera_buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: svgf::DEBUG_DATA,
            resource: svgf_debug_buffer.as_entire_binding(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use crate::renderer::protocol::bindings::{svgf, trace};

    use super::{
        should_rebuild_bind_groups, svgf_bind_group_binding_order, trace_bind_group_binding_order,
    };

    #[test]
    fn rebuild_triggered_by_world_resource_change() {
        assert!(should_rebuild_bind_groups(true, false));
    }

    #[test]
    fn rebuild_triggered_by_surface_resource_change() {
        assert!(should_rebuild_bind_groups(false, true));
    }

    #[test]
    fn rebuild_not_triggered_without_any_change() {
        assert!(!should_rebuild_bind_groups(false, false));
    }

    #[test]
    fn trace_bind_group_order_matches_protocol_constants() {
        assert_eq!(trace_bind_group_binding_order(), trace::ORDER);
    }

    #[test]
    fn svgf_bind_group_order_matches_protocol_constants() {
        assert_eq!(svgf_bind_group_binding_order(), svgf::ORDER);
    }

    #[test]
    fn trace_bind_group_order_uses_protocol_count() {
        assert_eq!(trace_bind_group_binding_order().len(), trace::COUNT);
    }

    #[test]
    fn svgf_bind_group_order_uses_protocol_count() {
        assert_eq!(svgf_bind_group_binding_order().len(), svgf::COUNT);
    }
}
