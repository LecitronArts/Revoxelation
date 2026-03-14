use log::error;
use winit::dpi::PhysicalSize;

use crate::renderer::lifecycle::executor::{
    LifecycleExecutionAction, RendererBindGroupTargets, RendererLifecycleExecutorInput,
    execute_renderer_lifecycle, lifecycle_execution_trace,
};
use crate::renderer::lifecycle::plan::{
    RendererLifecycleEvent, RendererLifecyclePlan, plan_renderer_lifecycle,
};
use crate::renderer::resources::surface::clamp_render_extent;
use crate::renderer::world::sync::{
    prepare_world_sync, record_world_sync_rejection_state, record_world_sync_success_state,
};
use crate::renderer::world::upload::{
    ExecutedWorldUpload, apply_world_upload_metadata, execute_world_upload, prepare_world_upload,
};
use crate::world::VoxelWorld;

use super::state::{Renderer, RendererDiagEvent, RendererDiagEventSnapshot};

pub(super) fn sync_world(renderer: &mut Renderer, world: &VoxelWorld) {
    let prepared = match prepare_world_sync(
        world,
        renderer.runtime.frame_bridge.frame_index(),
        &renderer.runtime.emissive_signatures,
        renderer.gpu.max_storage_binding_size,
    ) {
        Ok(prepared) => prepared,
        Err(rejected) => {
            for issue in &rejected.issues {
                error!("[sync_world] {issue}");
            }
            record_world_sync_rejection_state(
                &mut renderer.runtime.world_sync_reject_count,
                &mut renderer.runtime.last_world_sync_reject_reason,
                &rejected.reason,
            );
            error!(
                "[sync_world] rejected world sync due to storage buffer limits (max={} bytes, reason={})",
                renderer.gpu.max_storage_binding_size, rejected.reason
            );
            apply_renderer_lifecycle(renderer, RendererLifecycleEvent::SyncRejected);
            return;
        }
    };
    let upload_plan = prepare_world_upload(prepared.payload, prepared.remap);
    let uploaded = execute_world_upload(&renderer.gpu.device, &renderer.gpu.queue, upload_plan);
    apply_world_upload(renderer, uploaded);
    apply_renderer_lifecycle(renderer, RendererLifecycleEvent::SyncSucceeded);
}

pub(super) fn resize(renderer: &mut Renderer, new_size: PhysicalSize<u32>) {
    let clamped = clamp_render_extent(
        new_size.width,
        new_size.height,
        renderer.gpu.max_storage_binding_size,
    );
    renderer.gpu.size = PhysicalSize::new(clamped.0, clamped.1);
    if new_size.width == 0 || new_size.height == 0 {
        return;
    }

    renderer.gpu.config.width = renderer.gpu.size.width;
    renderer.gpu.config.height = renderer.gpu.size.height;
    apply_renderer_lifecycle(renderer, RendererLifecycleEvent::Resize);
}

pub(super) fn reconfigure(renderer: &mut Renderer) {
    if renderer.gpu.size.width == 0 || renderer.gpu.size.height == 0 {
        return;
    }
    apply_renderer_lifecycle(renderer, RendererLifecycleEvent::Reconfigure);
}

pub(super) fn reset_accumulation(renderer: &mut Renderer) {
    renderer.runtime.frame_bridge.reset();
}

pub(super) fn apply_renderer_lifecycle(renderer: &mut Renderer, event: RendererLifecycleEvent) {
    let plan: RendererLifecyclePlan = plan_renderer_lifecycle(event);
    let trace = lifecycle_execution_trace(plan);
    renderer.runtime.diagnostics.last_lifecycle_event = Some(event);
    renderer.runtime.diagnostics.last_lifecycle_trace = trace.clone();

    let mut input = RendererLifecycleExecutorInput {
        surface: &renderer.gpu.surface,
        device: &renderer.gpu.device,
        config: &renderer.gpu.config,
        max_storage_binding_size: renderer.gpu.max_storage_binding_size,
        output_format: renderer.gpu.output_format,
        settings: &renderer.runtime.settings,
        resources: &mut renderer.resources,
        bind_group_layout: &renderer.pipelines.bind_group_layout,
        svgf_bind_group_layout: &renderer.pipelines.svgf_bind_group_layout,
        camera_buffer: &renderer.uniforms.camera_buffer,
        previous_camera_buffer: &renderer.uniforms.previous_camera_buffer,
        tracer_uniform_buffer: &renderer.uniforms.tracer_uniform_buffer,
        bind_groups: RendererBindGroupTargets {
            trace_bind_group: &mut renderer.pipelines.bind_group,
            svgf_init_bind_group: &mut renderer.pipelines.svgf_init_bind_group,
            svgf_atrous_bind_groups: &mut renderer.pipelines.svgf_atrous_bind_groups,
            svgf_resolve_bind_group: &mut renderer.pipelines.svgf_resolve_bind_group,
        },
        frame_bridge: &mut renderer.runtime.frame_bridge,
    };
    let _ = execute_renderer_lifecycle(plan, &mut input);
    push_lifecycle_diag_event(renderer, event, trace);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::lifecycle::executor::LifecycleExecutionAction;

    fn lifecycle_trace(event: RendererLifecycleEvent) -> Vec<LifecycleExecutionAction> {
        lifecycle_execution_trace(plan_renderer_lifecycle(event))
    }

    #[test]
    fn lifecycle_diag_trace_sync_success() {
        assert_eq!(
            lifecycle_trace(RendererLifecycleEvent::SyncSucceeded),
            vec![
                LifecycleExecutionAction::RebuildBindGroups,
                LifecycleExecutionAction::ResetAccumulation
            ]
        );
    }

    #[test]
    fn lifecycle_diag_trace_sync_reject() {
        assert_eq!(
            lifecycle_trace(RendererLifecycleEvent::SyncRejected),
            Vec::new()
        );
    }

    #[test]
    fn lifecycle_diag_trace_resize() {
        assert_eq!(
            lifecycle_trace(RendererLifecycleEvent::Resize),
            vec![
                LifecycleExecutionAction::ConfigureSurface,
                LifecycleExecutionAction::RebuildSurfaceResources,
                LifecycleExecutionAction::RebuildBindGroups,
                LifecycleExecutionAction::ResetAccumulation
            ]
        );
    }

    #[test]
    fn lifecycle_diag_trace_reconfigure() {
        assert_eq!(
            lifecycle_trace(RendererLifecycleEvent::Reconfigure),
            vec![
                LifecycleExecutionAction::ConfigureSurface,
                LifecycleExecutionAction::ResetAccumulation
            ]
        );
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct WorldSyncDiagState {
        reject_count: u32,
        last_reason: String,
        dropped_entries: u32,
    }

    fn on_reject(state: &mut WorldSyncDiagState, reason: &str) {
        record_world_sync_rejection_state(&mut state.reject_count, &mut state.last_reason, reason);
    }

    fn on_success(state: &mut WorldSyncDiagState, dropped_entries: u32) {
        record_world_sync_success_state(
            &mut state.dropped_entries,
            &mut state.last_reason,
            dropped_entries,
        );
    }

    #[test]
    fn world_sync_diag_reject_updates_reason_and_count() {
        let mut state = WorldSyncDiagState::default();
        on_reject(&mut state, "rejected");
        assert_eq!(state.reject_count, 1);
        assert_eq!(state.last_reason, "rejected");
        assert_eq!(state.dropped_entries, 0);
    }

    #[test]
    fn world_sync_diag_success_clears_reason_and_sets_dropped_entries() {
        let mut state = WorldSyncDiagState::default();
        on_reject(&mut state, "rejected");
        on_success(&mut state, 3);
        assert_eq!(state.reject_count, 1);
        assert_eq!(state.last_reason, "");
        assert_eq!(state.dropped_entries, 3);
    }

    #[test]
    fn world_sync_diag_reject_then_reject_keeps_latest_reason() {
        let mut state = WorldSyncDiagState::default();
        on_reject(&mut state, "a");
        on_reject(&mut state, "b");
        assert_eq!(state.reject_count, 2);
        assert_eq!(state.last_reason, "b");
    }

    #[test]
    fn world_sync_diag_success_does_not_change_reject_count() {
        let mut state = WorldSyncDiagState::default();
        on_reject(&mut state, "a");
        on_success(&mut state, 1);
        on_success(&mut state, 2);
        assert_eq!(state.reject_count, 1);
        assert_eq!(state.dropped_entries, 2);
    }
}

fn apply_world_upload(renderer: &mut Renderer, uploaded: ExecutedWorldUpload) {
    let ExecutedWorldUpload {
        metadata,
        resources,
    } = uploaded;
    let dropped_entries = apply_world_upload_metadata(
        metadata,
        &mut renderer.runtime.chunk_count,
        &mut renderer.runtime.chunk_map_size,
        &mut renderer.runtime.chunk_map_mask,
        &mut renderer.runtime.chunk_map_max_probe,
        &mut renderer.runtime.chunk_map_avg_probe,
        &mut renderer.runtime.chunk_map_max_probe_observed,
        &mut renderer.runtime.chunk_map_load_factor,
        &mut renderer.runtime.emissive_count,
        &mut renderer.runtime.emissive_cdf_count,
        &mut renderer.runtime.emissive_remap_count,
        &mut renderer.runtime.importance_map_dims,
        &mut renderer.runtime.emissive_signatures,
        &mut renderer.runtime.world_min,
        &mut renderer.runtime.world_max,
    );
    record_world_sync_success_state(
        &mut renderer.runtime.chunk_map_dropped_entries,
        &mut renderer.runtime.last_world_sync_reject_reason,
        dropped_entries,
    );

    renderer.resources.apply_world_upload(resources);
}

fn push_lifecycle_diag_event(
    renderer: &mut Renderer,
    event: RendererLifecycleEvent,
    lifecycle_trace: Vec<LifecycleExecutionAction>,
) {
    let snapshot = RendererDiagEventSnapshot {
        frame_index: renderer.runtime.frame_bridge.frame_index(),
        lifecycle_trace: Some(lifecycle_trace),
        render_summary: None,
        render_trace_len: renderer.runtime.diagnostics.last_render_trace_len,
        resource_version_signature: renderer.resources.versions().dependency_signature(),
        world_sync_reject_count: renderer.runtime.world_sync_reject_count,
        last_world_sync_reject_reason: renderer.runtime.last_world_sync_reject_reason.clone(),
        chunk_map_dropped_entries: renderer.runtime.chunk_map_dropped_entries,
    };
    let event = match event {
        RendererLifecycleEvent::SyncRejected => RendererDiagEvent::SyncRejected(snapshot),
        RendererLifecycleEvent::SyncSucceeded => RendererDiagEvent::SyncSucceeded(snapshot),
        RendererLifecycleEvent::Resize => RendererDiagEvent::Resize(snapshot),
        RendererLifecycleEvent::Reconfigure => RendererDiagEvent::Reconfigure(snapshot),
    };
    renderer.runtime.diag_events.push(event);
}
