#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldSyncPolicyEvent {
    Rejected,
    Succeeded,
    Resized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldSyncPolicyDecision {
    pub recreate_bind_groups: bool,
    pub reset_accumulation: bool,
}

pub const fn world_sync_policy(event: WorldSyncPolicyEvent) -> WorldSyncPolicyDecision {
    match event {
        WorldSyncPolicyEvent::Rejected => WorldSyncPolicyDecision {
            recreate_bind_groups: false,
            reset_accumulation: false,
        },
        WorldSyncPolicyEvent::Succeeded | WorldSyncPolicyEvent::Resized => {
            WorldSyncPolicyDecision {
                recreate_bind_groups: true,
                reset_accumulation: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejection_does_not_trigger_rebuild_or_reset() {
        let decision = world_sync_policy(WorldSyncPolicyEvent::Rejected);
        assert!(!decision.recreate_bind_groups);
        assert!(!decision.reset_accumulation);
    }

    #[test]
    fn success_triggers_rebuild_and_reset() {
        let decision = world_sync_policy(WorldSyncPolicyEvent::Succeeded);
        assert!(decision.recreate_bind_groups);
        assert!(decision.reset_accumulation);
    }

    #[test]
    fn resize_triggers_rebuild_and_reset() {
        let decision = world_sync_policy(WorldSyncPolicyEvent::Resized);
        assert!(decision.recreate_bind_groups);
        assert!(decision.reset_accumulation);
    }
}
