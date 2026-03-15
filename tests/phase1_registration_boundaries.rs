use revoxelation::runtime::{
    boundaries::{RuntimeBoundaryRegistry, RuntimeDomain},
    systems::placeholders,
};

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