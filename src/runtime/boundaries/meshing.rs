#[cfg(test)]
mod tests {
    use crate::runtime::boundaries::{BoundaryRegistry, DomainSystem, RuntimeDomain};

    struct MeshSyncSystem;

    impl DomainSystem for MeshSyncSystem {
        const DOMAIN: RuntimeDomain = RuntimeDomain::Meshing;
        const NAME: &'static str = "mesh_sync";
    }

    #[test]
    fn boundary_mesh_sync_registered() {
        let mut reg = BoundaryRegistry::new(RuntimeDomain::Meshing);
        reg.register::<MeshSyncSystem>()
            .expect("registration must succeed");
        let names: Vec<&str> = reg.systems().iter().map(|s| s.name).collect();
        assert!(
            names.contains(&"mesh_sync"),
            "expected 'mesh_sync' in registry, got: {:?}",
            names
        );
    }
}
