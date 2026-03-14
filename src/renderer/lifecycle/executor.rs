use crate::renderer::RendererSettings;
use crate::renderer::resources::bind_groups::{RebuiltBindGroups, rebuild_bind_groups};
use crate::renderer::resources::context::RendererResourceContext;
use crate::renderer::resources::restir_storage::FrameBridge;
use crate::renderer::resources::surface::{
    build_surface_resource_state, rebuild_surface_resources,
};

use super::plan::RendererLifecyclePlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleExecutionSummary {
    pub configure_surface: bool,
    pub rebuild_surface_resources: bool,
    pub rebuild_bind_groups: bool,
    pub reset_accumulation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleExecutionAction {
    ConfigureSurface,
    RebuildSurfaceResources,
    RebuildBindGroups,
    ResetAccumulation,
}

pub const fn lifecycle_execution_summary(plan: RendererLifecyclePlan) -> LifecycleExecutionSummary {
    LifecycleExecutionSummary {
        configure_surface: plan.reconfigure_surface,
        rebuild_surface_resources: plan.rebuild_surface_resources,
        rebuild_bind_groups: plan.should_rebuild_bind_groups(),
        reset_accumulation: plan.reset_accumulation,
    }
}

pub trait RendererLifecycleHooks {
    fn configure_surface(&mut self);
    fn rebuild_surface_resources(&mut self);
    fn rebuild_bind_groups(&mut self);
    fn reset_accumulation(&mut self);
}

pub fn lifecycle_execution_trace(plan: RendererLifecyclePlan) -> Vec<LifecycleExecutionAction> {
    let summary = lifecycle_execution_summary(plan);
    let mut actions = Vec::new();
    if summary.configure_surface {
        actions.push(LifecycleExecutionAction::ConfigureSurface);
    }
    if summary.rebuild_surface_resources {
        actions.push(LifecycleExecutionAction::RebuildSurfaceResources);
    }
    if summary.rebuild_bind_groups {
        actions.push(LifecycleExecutionAction::RebuildBindGroups);
    }
    if summary.reset_accumulation {
        actions.push(LifecycleExecutionAction::ResetAccumulation);
    }
    actions
}

pub fn execute_renderer_lifecycle_with_hooks(
    plan: RendererLifecyclePlan,
    hooks: &mut impl RendererLifecycleHooks,
) -> LifecycleExecutionSummary {
    let summary = lifecycle_execution_summary(plan);
    for action in lifecycle_execution_trace(plan) {
        match action {
            LifecycleExecutionAction::ConfigureSurface => hooks.configure_surface(),
            LifecycleExecutionAction::RebuildSurfaceResources => hooks.rebuild_surface_resources(),
            LifecycleExecutionAction::RebuildBindGroups => hooks.rebuild_bind_groups(),
            LifecycleExecutionAction::ResetAccumulation => hooks.reset_accumulation(),
        }
    }
    summary
}

pub struct RendererBindGroupTargets<'a> {
    pub trace_bind_group: &'a mut wgpu::BindGroup,
    pub svgf_init_bind_group: &'a mut wgpu::BindGroup,
    pub svgf_atrous_bind_groups: &'a mut Vec<wgpu::BindGroup>,
    pub svgf_resolve_bind_group: &'a mut wgpu::BindGroup,
}

pub struct RendererLifecycleExecutorInput<'a> {
    pub surface: &'a wgpu::Surface<'static>,
    pub device: &'a wgpu::Device,
    pub config: &'a wgpu::SurfaceConfiguration,
    pub max_storage_binding_size: u64,
    pub output_format: wgpu::TextureFormat,
    pub settings: &'a RendererSettings,
    pub resources: &'a mut RendererResourceContext,
    pub bind_group_layout: &'a wgpu::BindGroupLayout,
    pub svgf_bind_group_layout: &'a wgpu::BindGroupLayout,
    pub camera_buffer: &'a wgpu::Buffer,
    pub previous_camera_buffer: &'a wgpu::Buffer,
    pub tracer_uniform_buffer: &'a wgpu::Buffer,
    pub bind_groups: RendererBindGroupTargets<'a>,
    pub frame_bridge: &'a mut FrameBridge,
}

impl RendererLifecycleHooks for RendererLifecycleExecutorInput<'_> {
    fn configure_surface(&mut self) {
        self.surface.configure(self.device, self.config);
    }

    fn rebuild_surface_resources(&mut self) {
        let surface_state = build_surface_resource_state(
            self.config.width,
            self.config.height,
            self.max_storage_binding_size,
            self.settings,
        );
        let rebuilt = rebuild_surface_resources(
            self.device,
            surface_state,
            self.output_format,
            self.settings,
        );
        self.resources.apply_surface_rebuild(rebuilt);
    }

    fn rebuild_bind_groups(&mut self) {
        let rebuilt: RebuiltBindGroups = rebuild_bind_groups(self.resources.bind_group_input(
            self.device,
            self.bind_group_layout,
            self.svgf_bind_group_layout,
            self.camera_buffer,
            self.previous_camera_buffer,
            self.tracer_uniform_buffer,
        ));
        *self.bind_groups.trace_bind_group = rebuilt.trace_bind_group;
        *self.bind_groups.svgf_init_bind_group = rebuilt.svgf_init_bind_group;
        *self.bind_groups.svgf_atrous_bind_groups = rebuilt.svgf_atrous_bind_groups;
        *self.bind_groups.svgf_resolve_bind_group = rebuilt.svgf_resolve_bind_group;
    }

    fn reset_accumulation(&mut self) {
        self.frame_bridge.reset();
    }
}

pub fn execute_renderer_lifecycle(
    plan: RendererLifecyclePlan,
    input: &mut RendererLifecycleExecutorInput<'_>,
) -> LifecycleExecutionSummary {
    execute_renderer_lifecycle_with_hooks(plan, input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::lifecycle::plan::{RendererLifecycleEvent, plan_renderer_lifecycle};

    #[derive(Default)]
    struct HookTrace {
        calls: Vec<&'static str>,
    }

    impl RendererLifecycleHooks for HookTrace {
        fn configure_surface(&mut self) {
            self.calls.push("configure_surface");
        }

        fn rebuild_surface_resources(&mut self) {
            self.calls.push("rebuild_surface_resources");
        }

        fn rebuild_bind_groups(&mut self) {
            self.calls.push("rebuild_bind_groups");
        }

        fn reset_accumulation(&mut self) {
            self.calls.push("reset_accumulation");
        }
    }

    #[test]
    fn sync_success_executes_bind_group_rebuild_and_reset() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::SyncSucceeded);
        let mut hooks = HookTrace::default();
        let summary = execute_renderer_lifecycle_with_hooks(plan, &mut hooks);
        assert_eq!(
            hooks.calls,
            vec!["rebuild_bind_groups", "reset_accumulation"]
        );
        assert!(!summary.configure_surface);
        assert!(summary.rebuild_bind_groups);
        assert!(summary.reset_accumulation);
    }

    #[test]
    fn sync_reject_executes_no_operations() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::SyncRejected);
        let mut hooks = HookTrace::default();
        let summary = execute_renderer_lifecycle_with_hooks(plan, &mut hooks);
        assert!(hooks.calls.is_empty());
        assert!(!summary.configure_surface);
        assert!(!summary.rebuild_surface_resources);
        assert!(!summary.rebuild_bind_groups);
        assert!(!summary.reset_accumulation);
    }

    #[test]
    fn resize_executes_full_rebuild_sequence() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Resize);
        let mut hooks = HookTrace::default();
        let summary = execute_renderer_lifecycle_with_hooks(plan, &mut hooks);
        assert_eq!(
            hooks.calls,
            vec![
                "configure_surface",
                "rebuild_surface_resources",
                "rebuild_bind_groups",
                "reset_accumulation",
            ]
        );
        assert!(summary.configure_surface);
        assert!(summary.rebuild_surface_resources);
        assert!(summary.rebuild_bind_groups);
        assert!(summary.reset_accumulation);
    }

    #[test]
    fn reconfigure_executes_configure_and_reset_only() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Reconfigure);
        let mut hooks = HookTrace::default();
        let summary = execute_renderer_lifecycle_with_hooks(plan, &mut hooks);
        assert_eq!(hooks.calls, vec!["configure_surface", "reset_accumulation"]);
        assert!(summary.configure_surface);
        assert!(!summary.rebuild_surface_resources);
        assert!(!summary.rebuild_bind_groups);
        assert!(summary.reset_accumulation);
    }

    #[test]
    fn summary_matches_noop_plan() {
        let plan = super::super::plan::RendererLifecyclePlan {
            reconfigure_surface: false,
            rebuild_surface_resources: false,
            request_bind_group_rebuild: false,
            world_resources_changed: false,
            surface_resources_changed: false,
            reset_accumulation: false,
        };
        let summary = lifecycle_execution_summary(plan);
        assert!(!summary.configure_surface);
        assert!(!summary.rebuild_surface_resources);
        assert!(!summary.rebuild_bind_groups);
        assert!(!summary.reset_accumulation);
    }

    #[test]
    fn summary_requires_resource_change_for_bind_group_rebuild() {
        let plan = super::super::plan::RendererLifecyclePlan {
            reconfigure_surface: false,
            rebuild_surface_resources: false,
            request_bind_group_rebuild: true,
            world_resources_changed: false,
            surface_resources_changed: false,
            reset_accumulation: false,
        };
        let summary = lifecycle_execution_summary(plan);
        assert!(!summary.rebuild_bind_groups);
    }

    #[test]
    fn trace_matches_sync_success_order() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::SyncSucceeded);
        assert_eq!(
            lifecycle_execution_trace(plan),
            vec![
                LifecycleExecutionAction::RebuildBindGroups,
                LifecycleExecutionAction::ResetAccumulation,
            ]
        );
    }

    #[test]
    fn trace_matches_sync_reject_order() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::SyncRejected);
        assert_eq!(lifecycle_execution_trace(plan), Vec::new());
    }

    #[test]
    fn trace_matches_resize_order() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Resize);
        assert_eq!(
            lifecycle_execution_trace(plan),
            vec![
                LifecycleExecutionAction::ConfigureSurface,
                LifecycleExecutionAction::RebuildSurfaceResources,
                LifecycleExecutionAction::RebuildBindGroups,
                LifecycleExecutionAction::ResetAccumulation,
            ]
        );
    }

    #[test]
    fn trace_matches_reconfigure_order() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Reconfigure);
        assert_eq!(
            lifecycle_execution_trace(plan),
            vec![
                LifecycleExecutionAction::ConfigureSurface,
                LifecycleExecutionAction::ResetAccumulation,
            ]
        );
    }
}
