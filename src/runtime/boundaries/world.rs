#[cfg(test)]
mod tests {
    use crate::runtime::boundaries::{BoundaryRegistry, DomainSystem, RuntimeDomain};

    struct WorldStreamingSystem;

    impl DomainSystem for WorldStreamingSystem {
        const DOMAIN: RuntimeDomain = RuntimeDomain::World;
        const NAME: &'static str = "world_streaming";
    }

    #[test]
    fn boundary_world_streaming_registered() {
        let mut reg = BoundaryRegistry::new(RuntimeDomain::World);
        reg.register::<WorldStreamingSystem>()
            .expect("registration must succeed");
        let names: Vec<&str> = reg.systems().iter().map(|s| s.name).collect();
        assert!(
            names.contains(&"world_streaming"),
            "expected 'world_streaming' in registry, got: {:?}",
            names
        );
    }
}
