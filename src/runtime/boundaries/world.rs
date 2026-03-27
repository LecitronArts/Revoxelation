use super::{
    BoundaryRegistryCore, DomainSystem, RegisteredSystem, RegistrationError, RuntimeDomain,
};

#[derive(Debug, Clone)]
pub struct WorldBoundaryRegistry {
    core: BoundaryRegistryCore,
}

impl WorldBoundaryRegistry {
    pub(crate) fn new() -> Self {
        Self {
            core: BoundaryRegistryCore::new(RuntimeDomain::World),
        }
    }

    pub fn register<S: DomainSystem>(&mut self) -> Result<(), RegistrationError> {
        self.core.register(S::DOMAIN, S::NAME)
    }

    pub fn systems(&self) -> &[RegisteredSystem] {
        self.core.systems()
    }
}

// ---------------------------------------------------------------------------
// WorldStreamingSystem
// ---------------------------------------------------------------------------

/// Boundary-registered stub for the streaming world update system.
#[allow(dead_code)]
pub struct WorldStreamingSystem;

impl DomainSystem for WorldStreamingSystem {
    const DOMAIN: RuntimeDomain = RuntimeDomain::World;
    const NAME: &'static str = "world_streaming";
}

/// Register all world streaming systems into `reg`.
#[allow(dead_code)]
pub fn register_streaming_systems(
    reg: &mut WorldBoundaryRegistry,
) -> Result<(), RegistrationError> {
    reg.register::<WorldStreamingSystem>()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_world_streaming_registered() {
        let mut reg = WorldBoundaryRegistry::new();
        register_streaming_systems(&mut reg).expect("registration must succeed");
        let names: Vec<&str> = reg.systems().iter().map(|s| s.name).collect();
        assert!(
            names.contains(&"world_streaming"),
            "expected 'world_streaming' in registry, got: {:?}",
            names
        );
    }
}
