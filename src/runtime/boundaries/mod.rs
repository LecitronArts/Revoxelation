mod meshing;
mod world;

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
pub struct BoundaryRegistry {
    domain: RuntimeDomain,
    systems: Vec<RegisteredSystem>,
}

impl BoundaryRegistry {
    pub fn new(domain: RuntimeDomain) -> Self {
        Self {
            domain,
            systems: Vec::new(),
        }
    }

    pub fn register<S: DomainSystem>(&mut self) -> Result<(), RegistrationError> {
        let declared_domain = S::DOMAIN;
        let name = S::NAME;

        if declared_domain != self.domain {
            return Err(RegistrationError::with_reason(format!(
                "cross-domain registration rejected: system `{name}` declared `{}` cannot register in `{}` boundary",
                declared_domain.as_str(),
                self.domain.as_str()
            )));
        }

        if self
            .systems
            .iter()
            .any(|registered| registered.name == name)
        {
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

    pub fn systems(&self) -> &[RegisteredSystem] {
        &self.systems
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeBoundaryRegistry {
    world: BoundaryRegistry,
    meshing: BoundaryRegistry,
    collision: BoundaryRegistry,
    persistence: BoundaryRegistry,
}

impl RuntimeBoundaryRegistry {
    pub fn world(&self) -> &BoundaryRegistry {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut BoundaryRegistry {
        &mut self.world
    }

    pub fn meshing(&self) -> &BoundaryRegistry {
        &self.meshing
    }

    pub fn meshing_mut(&mut self) -> &mut BoundaryRegistry {
        &mut self.meshing
    }

    pub fn collision(&self) -> &BoundaryRegistry {
        &self.collision
    }

    pub fn collision_mut(&mut self) -> &mut BoundaryRegistry {
        &mut self.collision
    }

    pub fn persistence(&self) -> &BoundaryRegistry {
        &self.persistence
    }

    pub fn persistence_mut(&mut self) -> &mut BoundaryRegistry {
        &mut self.persistence
    }
}

impl Default for RuntimeBoundaryRegistry {
    fn default() -> Self {
        Self {
            world: BoundaryRegistry::new(RuntimeDomain::World),
            meshing: BoundaryRegistry::new(RuntimeDomain::Meshing),
            collision: BoundaryRegistry::new(RuntimeDomain::Collision),
            persistence: BoundaryRegistry::new(RuntimeDomain::Persistence),
        }
    }
}
