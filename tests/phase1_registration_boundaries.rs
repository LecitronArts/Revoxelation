use revoxelation::runtime::{
    boundaries::{DomainSystem, RuntimeBoundaryRegistry, RuntimeDomain},
    systems::placeholders,
};

struct MeshingCrossDomainProbe;

impl DomainSystem for MeshingCrossDomainProbe {
    const DOMAIN: RuntimeDomain = RuntimeDomain::Meshing;
    const NAME: &'static str = "meshing_cross_domain_probe";
}

struct WorldDuplicateProbe;

impl DomainSystem for WorldDuplicateProbe {
    const DOMAIN: RuntimeDomain = RuntimeDomain::World;
    const NAME: &'static str = "world_duplicate_probe";
}

#[test]
fn boundary_registers_in_domain_systems() {
    let mut registry = RuntimeBoundaryRegistry::default();
    placeholders::register(&mut registry).expect("placeholder registration should succeed");

    let world = registry.world().systems();
    assert_eq!(world.len(), 1, "world boundary should register one system");
    assert_eq!(world[0].name, "world_placeholder");
    assert_eq!(world[0].declared_domain, RuntimeDomain::World);

    let meshing = registry.meshing().systems();
    assert_eq!(meshing.len(), 1, "meshing boundary should register one system");
    assert_eq!(meshing[0].name, "meshing_placeholder");
    assert_eq!(meshing[0].declared_domain, RuntimeDomain::Meshing);

    let collision = registry.collision().systems();
    assert_eq!(collision.len(), 1, "collision boundary should register one system");
    assert_eq!(collision[0].name, "collision_placeholder");
    assert_eq!(collision[0].declared_domain, RuntimeDomain::Collision);

    let persistence = registry.persistence().systems();
    assert_eq!(
        persistence.len(),
        1,
        "persistence boundary should register one system",
    );
    assert_eq!(persistence[0].name, "persistence_placeholder");
    assert_eq!(persistence[0].declared_domain, RuntimeDomain::Persistence);
}

#[test]
fn boundary_rejects_cross_domain_registration() {
    let mut registry = RuntimeBoundaryRegistry::default();

    let error = registry
        .world_mut()
        .register::<MeshingCrossDomainProbe>()
        .expect_err("cross-domain registration should be rejected");

    assert_eq!(
        error.reason,
        "cross-domain registration rejected: system `meshing_cross_domain_probe` declared `meshing` cannot register in `world` boundary"
    );
}

#[test]
fn boundary_rejects_duplicate_registration() {
    let mut registry = RuntimeBoundaryRegistry::default();

    registry
        .world_mut()
        .register::<WorldDuplicateProbe>()
        .expect("first registration should succeed");

    let error = registry
        .world_mut()
        .register::<WorldDuplicateProbe>()
        .expect_err("duplicate registration should be rejected");

    assert_eq!(
        error.reason,
        "duplicate registration rejected: system `world_duplicate_probe` already registered in `world` boundary"
    );
}