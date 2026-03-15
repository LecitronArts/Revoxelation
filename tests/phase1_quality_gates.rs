use std::path::Path;

const WAVE0_SELECTOR_NAMES: [&str; 5] = [
    "wave0_stage_selector_bootstrap",
    "wave0_observability_selector_bootstrap",
    "wave0_boundary_selector_bootstrap",
    "wave0_events_selector_bootstrap",
    "wave0_quality_gate_selector_bootstrap",
];

const WAVE0_SMOKE_COMMAND: &str = "cargo test --quiet wave0_ -- --nocapture";

const WAVE0_ARTIFACT_FILES: [&str; 5] = [
    "tests/phase1_stage_order.rs",
    "tests/phase1_observability.rs",
    "tests/phase1_registration_boundaries.rs",
    "tests/phase1_events.rs",
    "tests/phase1_quality_gates.rs",
];

#[test]
fn wave0_quality_gate_selector_bootstrap() {
    for selector in WAVE0_SELECTOR_NAMES {
        assert!(
            selector.starts_with("wave0_"),
            "selector must be wave0-prefixed: {selector}",
        );
    }

    assert!(
        WAVE0_SMOKE_COMMAND.contains("wave0_"),
        "Wave 0 smoke command must target wave0 selectors",
    );

    for artifact in WAVE0_ARTIFACT_FILES {
        assert!(
            Path::new(artifact).exists(),
            "Wave 0 artifact missing: {artifact}",
        );
    }
}
