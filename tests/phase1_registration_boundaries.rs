use std::collections::BTreeSet;
use std::path::Path;

const RUNTIME_BOUNDARIES: [&str; 4] = ["world", "meshing", "collision", "persistence"];
const BOUNDARY_REGISTRY_FILE: &str = "src/runtime/boundaries/mod.rs";

#[test]
fn wave0_boundary_selector_bootstrap() {
    assert_eq!(
        RUNTIME_BOUNDARIES,
        ["world", "meshing", "collision", "persistence"],
        "Wave 0 boundary fixtures changed unexpectedly",
    );

    let unique: BTreeSet<&str> = RUNTIME_BOUNDARIES.into_iter().collect();
    assert_eq!(
        unique.len(),
        RUNTIME_BOUNDARIES.len(),
        "Boundary fixtures must remain unique",
    );

    if Path::new(BOUNDARY_REGISTRY_FILE).exists() {
        let boundary_source = std::fs::read_to_string(BOUNDARY_REGISTRY_FILE)
            .expect("boundary registry source should be readable when present");
        for boundary in RUNTIME_BOUNDARIES {
            assert!(
                boundary_source.contains(boundary),
                "boundary source drifted: missing `{boundary}` anchor",
            );
        }
    }
}
