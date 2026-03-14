use crate::renderer::resources::bind_groups::should_rebuild_bind_groups;
use crate::renderer::resources::surface::{SurfaceResourcesEvent, surface_resources_policy};

use super::policy::{WorldSyncPolicyEvent, world_sync_policy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererLifecycleEvent {
    SyncRejected,
    SyncSucceeded,
    Resize,
    Reconfigure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererLifecyclePlan {
    pub reconfigure_surface: bool,
    pub rebuild_surface_resources: bool,
    pub request_bind_group_rebuild: bool,
    pub world_resources_changed: bool,
    pub surface_resources_changed: bool,
    pub reset_accumulation: bool,
}

impl RendererLifecyclePlan {
    pub const fn should_rebuild_bind_groups(self) -> bool {
        self.request_bind_group_rebuild
            && should_rebuild_bind_groups(
                self.world_resources_changed,
                self.surface_resources_changed,
            )
    }
}

pub const fn plan_renderer_lifecycle(event: RendererLifecycleEvent) -> RendererLifecyclePlan {
    match event {
        RendererLifecycleEvent::SyncRejected => {
            let sync = world_sync_policy(WorldSyncPolicyEvent::Rejected);
            RendererLifecyclePlan {
                reconfigure_surface: false,
                rebuild_surface_resources: false,
                request_bind_group_rebuild: sync.recreate_bind_groups,
                world_resources_changed: false,
                surface_resources_changed: false,
                reset_accumulation: sync.reset_accumulation,
            }
        }
        RendererLifecycleEvent::SyncSucceeded => {
            let sync = world_sync_policy(WorldSyncPolicyEvent::Succeeded);
            RendererLifecyclePlan {
                reconfigure_surface: false,
                rebuild_surface_resources: false,
                request_bind_group_rebuild: sync.recreate_bind_groups,
                world_resources_changed: true,
                surface_resources_changed: false,
                reset_accumulation: sync.reset_accumulation,
            }
        }
        RendererLifecycleEvent::Resize => {
            let sync = world_sync_policy(WorldSyncPolicyEvent::Resized);
            let surface = surface_resources_policy(SurfaceResourcesEvent::Resize);
            RendererLifecyclePlan {
                reconfigure_surface: surface.reconfigure_surface,
                rebuild_surface_resources: surface.rebuild_resources,
                request_bind_group_rebuild: sync.recreate_bind_groups,
                world_resources_changed: false,
                surface_resources_changed: true,
                reset_accumulation: sync.reset_accumulation,
            }
        }
        RendererLifecycleEvent::Reconfigure => {
            let surface = surface_resources_policy(SurfaceResourcesEvent::Reconfigure);
            RendererLifecyclePlan {
                reconfigure_surface: surface.reconfigure_surface,
                rebuild_surface_resources: surface.rebuild_resources,
                request_bind_group_rebuild: false,
                world_resources_changed: false,
                surface_resources_changed: false,
                reset_accumulation: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_success_plan_rebuilds_bind_groups_and_resets() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::SyncSucceeded);
        assert!(!plan.reconfigure_surface);
        assert!(!plan.rebuild_surface_resources);
        assert!(plan.should_rebuild_bind_groups());
        assert!(plan.reset_accumulation);
    }

    #[test]
    fn sync_rejected_plan_is_noop() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::SyncRejected);
        assert!(!plan.reconfigure_surface);
        assert!(!plan.rebuild_surface_resources);
        assert!(!plan.should_rebuild_bind_groups());
        assert!(!plan.reset_accumulation);
    }

    #[test]
    fn resize_plan_reconfigures_rebuilds_and_resets() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Resize);
        assert!(plan.reconfigure_surface);
        assert!(plan.rebuild_surface_resources);
        assert!(plan.should_rebuild_bind_groups());
        assert!(plan.reset_accumulation);
    }

    #[test]
    fn reconfigure_plan_only_reconfigures_and_resets() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Reconfigure);
        assert!(plan.reconfigure_surface);
        assert!(!plan.rebuild_surface_resources);
        assert!(!plan.should_rebuild_bind_groups());
        assert!(plan.reset_accumulation);
    }

    #[test]
    fn sync_success_marks_world_change_only() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::SyncSucceeded);
        assert!(plan.world_resources_changed);
        assert!(!plan.surface_resources_changed);
        assert!(plan.request_bind_group_rebuild);
    }

    #[test]
    fn resize_marks_surface_change_only() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Resize);
        assert!(!plan.world_resources_changed);
        assert!(plan.surface_resources_changed);
        assert!(plan.request_bind_group_rebuild);
    }

    #[test]
    fn reconfigure_disables_bind_group_rebuild_request() {
        let plan = plan_renderer_lifecycle(RendererLifecycleEvent::Reconfigure);
        assert!(!plan.world_resources_changed);
        assert!(!plan.surface_resources_changed);
        assert!(!plan.request_bind_group_rebuild);
    }
}
