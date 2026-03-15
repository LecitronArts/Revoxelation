use super::{BoundaryRegistryCore, DomainSystem, RegisteredSystem, RegistrationError, RuntimeDomain};

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