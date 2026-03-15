mod collision;
mod meshing;
mod persistence;
mod world;

pub use collision::CollisionBoundaryRegistry;
pub use meshing::MeshingBoundaryRegistry;
pub use persistence::PersistenceBoundaryRegistry;
pub use world::WorldBoundaryRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeDomain {
    World,
    Meshing,
    Collision,
    Persistence,
}

impl RuntimeDomain {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::World => "world",
            Self::Meshing => "meshing",
            Self::Collision => "collision",
            Self::Persistence => "persistence",
        }
    }
}

pub trait DomainSystem {
    const DOMAIN: RuntimeDomain;
    const NAME: &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationError {
    pub reason: String,
}

impl RegistrationError {
    fn with_reason(reason: String) -> Self {
        Self { reason }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredSystem {
    pub name: &'static str,
    pub declared_domain: RuntimeDomain,
}

#[derive(Debug, Clone)]
pub(crate) struct BoundaryRegistryCore {
    domain: RuntimeDomain,
    systems: Vec<RegisteredSystem>,
}

impl BoundaryRegistryCore {
    pub(crate) fn new(domain: RuntimeDomain) -> Self {
        Self {
            domain,
            systems: Vec::new(),
        }
    }

    pub(crate) fn register(
        &mut self,
        declared_domain: RuntimeDomain,
        name: &'static str,
    ) -> Result<(), RegistrationError> {
        if declared_domain != self.domain {
            return Err(RegistrationError::with_reason(format!(
                "cross-domain registration rejected: system `{name}` declared `{}` cannot register in `{}` boundary",
                declared_domain.as_str(),
                self.domain.as_str()
            )));
        }

        if self.systems.iter().any(|registered| registered.name == name) {
            return Err(RegistrationError::with_reason(format!(
                "duplicate registration rejected: system `{name}` already registered in `{}` boundary",
                self.domain.as_str()
            )));
        }

        self.systems.push(RegisteredSystem {
            name,
            declared_domain,
        });
        Ok(())
    }

    pub(crate) fn systems(&self) -> &[RegisteredSystem] {
        &self.systems
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeBoundaryRegistry {
    world: WorldBoundaryRegistry,
    meshing: MeshingBoundaryRegistry,
    collision: CollisionBoundaryRegistry,
    persistence: PersistenceBoundaryRegistry,
}

impl RuntimeBoundaryRegistry {
    pub fn world(&self) -> &WorldBoundaryRegistry {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut WorldBoundaryRegistry {
        &mut self.world
    }

    pub fn meshing(&self) -> &MeshingBoundaryRegistry {
        &self.meshing
    }

    pub fn meshing_mut(&mut self) -> &mut MeshingBoundaryRegistry {
        &mut self.meshing
    }

    pub fn collision(&self) -> &CollisionBoundaryRegistry {
        &self.collision
    }

    pub fn collision_mut(&mut self) -> &mut CollisionBoundaryRegistry {
        &mut self.collision
    }

    pub fn persistence(&self) -> &PersistenceBoundaryRegistry {
        &self.persistence
    }

    pub fn persistence_mut(&mut self) -> &mut PersistenceBoundaryRegistry {
        &mut self.persistence
    }
}

impl Default for RuntimeBoundaryRegistry {
    fn default() -> Self {
        Self {
            world: WorldBoundaryRegistry::new(),
            meshing: MeshingBoundaryRegistry::new(),
            collision: CollisionBoundaryRegistry::new(),
            persistence: PersistenceBoundaryRegistry::new(),
        }
    }
}