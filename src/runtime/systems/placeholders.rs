use crate::runtime::boundaries::{
    DomainSystem, RegistrationError, RuntimeBoundaryRegistry, RuntimeDomain,
};

pub struct WorldPlaceholderSystem;

impl DomainSystem for WorldPlaceholderSystem {
    const DOMAIN: RuntimeDomain = RuntimeDomain::World;
    const NAME: &'static str = "world_placeholder";
}

pub struct MeshingPlaceholderSystem;

impl DomainSystem for MeshingPlaceholderSystem {
    const DOMAIN: RuntimeDomain = RuntimeDomain::Meshing;
    const NAME: &'static str = "meshing_placeholder";
}

pub struct CollisionPlaceholderSystem;

impl DomainSystem for CollisionPlaceholderSystem {
    const DOMAIN: RuntimeDomain = RuntimeDomain::Collision;
    const NAME: &'static str = "collision_placeholder";
}

pub struct PersistencePlaceholderSystem;

impl DomainSystem for PersistencePlaceholderSystem {
    const DOMAIN: RuntimeDomain = RuntimeDomain::Persistence;
    const NAME: &'static str = "persistence_placeholder";
}

pub fn register(registry: &mut RuntimeBoundaryRegistry) -> Result<(), RegistrationError> {
    registry.world_mut().register::<WorldPlaceholderSystem>()?;
    registry
        .meshing_mut()
        .register::<MeshingPlaceholderSystem>()?;
    registry
        .collision_mut()
        .register::<CollisionPlaceholderSystem>()?;
    registry
        .persistence_mut()
        .register::<PersistencePlaceholderSystem>()?;
    Ok(())
}
