use std::collections::BTreeSet;
use std::path::Path;

const LOCKED_STAGE_NAMES: [&str; 5] = [
    "Input",
    "Simulation",
    "WorldUpdate",
    "MeshSync",
    "RenderSubmit",
];
const STAGE_FILE: &str = "src/runtime/stages.rs";

#[test]
fn wave0_stage_selector_bootstrap() {
    assert_eq!(
        LOCKED_STAGE_NAMES,
        [
            "Input",
            "Simulation",
            "WorldUpdate",
            "MeshSync",
            "RenderSubmit",
        ],
        "Wave 0 stage lock fixtures changed unexpectedly"
    );

    let unique: BTreeSet<&str> = LOCKED_STAGE_NAMES.into_iter().collect();
    assert_eq!(
        unique.len(),
        LOCKED_STAGE_NAMES.len(),
        "Stage lock fixtures must remain unique"
    );

    if Path::new(STAGE_FILE).exists() {
        let stage_source =
            std::fs::read_to_string(STAGE_FILE).expect("stage source should be readable when present");
        for stage_name in LOCKED_STAGE_NAMES {
            assert!(
                stage_source.contains(stage_name),
                "stage source drifted: missing `{stage_name}` lock",
            );
        }
    }
}
