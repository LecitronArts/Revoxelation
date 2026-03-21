use super::{
    BoundaryRegistryCore, DomainSystem, RegisteredSystem, RegistrationError, RuntimeDomain,
};

#[derive(Debug, Clone)]
pub struct MeshingBoundaryRegistry {
    core: BoundaryRegistryCore,
}

impl MeshingBoundaryRegistry {
    pub(crate) fn new() -> Self {
        Self {
            core: BoundaryRegistryCore::new(RuntimeDomain::Meshing),
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
// MeshSyncSystem
// ---------------------------------------------------------------------------

/// Boundary-registered stub for the mesh synchronisation system.
pub struct MeshSyncSystem;

impl DomainSystem for MeshSyncSystem {
    const DOMAIN: RuntimeDomain = RuntimeDomain::Meshing;
    const NAME: &'static str = "mesh_sync";
}

/// Register all mesh sync systems into `reg`.
pub fn register_mesh_sync_systems(
    reg: &mut MeshingBoundaryRegistry,
) -> Result<(), RegistrationError> {
    reg.register::<MeshSyncSystem>()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_mesh_sync_registered() {
        let mut reg = MeshingBoundaryRegistry::new();
        register_mesh_sync_systems(&mut reg).expect("registration must succeed");
        let names: Vec<&str> = reg.systems().iter().map(|s| s.name).collect();
        assert!(
            names.contains(&"mesh_sync"),
            "expected 'mesh_sync' in registry, got: {:?}",
            names
        );
    }
}
