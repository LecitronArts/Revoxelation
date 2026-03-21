use std::{fs, path::Path};

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

const GATE_CHECKLIST_FILE: &str =
    ".planning/phases/01-runtime-skeleton-and-quality-gates/01-GATE-CHECKLIST.md";
const GATE_EVIDENCE_TEMPLATE_FILE: &str =
    ".planning/phases/01-runtime-skeleton-and-quality-gates/01-GATE-EVIDENCE-TEMPLATE.md";
const ARCHITECTURE_BOUNDARIES_FILE: &str =
    ".planning/phases/01-runtime-skeleton-and-quality-gates/01-ARCHITECTURE-BOUNDARIES.md";

const REQUIRED_GATES: [&str; 7] = [
    "writing-plans",
    "test-driven-development",
    "systematic-debugging",
    "verification-before-completion",
    "requesting-code-review",
    "receiving-code-review",
    "finishing-a-development-branch",
];

const REQUIRED_EVIDENCE_FIELDS: [&str; 8] = [
    "Gate",
    "Command",
    "Key Output",
    "Pass/Fail",
    "Owner",
    "Explicit Reason",
    "Risk",
    "Remediation / Follow-Up Plan",
];
const REQUIRED_CHECKLIST_SECTIONS: [&str; 2] = ["## Required Gates", "## Gate Evidence Log"];

const REQUIRED_CHECKLIST_TABLE_HEADERS: [&str; 8] = [
    "Gate",
    "Command",
    "Key Output",
    "Pass/Fail",
    "Owner",
    "Explicit Reason",
    "Risk",
    "Remediation / Follow-Up Plan",
];

const REQUIRED_ARCHITECTURE_SECTIONS: [&str; 5] = [
    "## Stage Spine",
    "## Boundary Contracts",
    "## Cross-Domain Rules",
    "## Observability Handoff",
    "## Closure Guard Signals",
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

#[test]
fn quality_gate_artifacts_present() {
    for artifact in [GATE_CHECKLIST_FILE, GATE_EVIDENCE_TEMPLATE_FILE] {
        assert!(
            Path::new(artifact).exists(),
            "quality gate artifact missing: {artifact}",
        );
    }

    let checklist = fs::read_to_string(GATE_CHECKLIST_FILE)
        .expect("quality gate checklist must be readable for enforcement");

    for section in REQUIRED_CHECKLIST_SECTIONS {
        assert!(
            checklist.contains(section),
            "quality gate checklist missing required section: {section}",
        );
    }

    for header in REQUIRED_CHECKLIST_TABLE_HEADERS {
        assert!(
            checklist.contains(header),
            "quality gate checklist missing required evidence header: {header}",
        );
    }

    for gate in REQUIRED_GATES {
        assert!(
            checklist.contains(&format!("`{gate}`")),
            "quality gate checklist missing required gate: {gate}",
        );
    }
}

#[test]
fn quality_gate_evidence_fields_present() {
    let template = fs::read_to_string(GATE_EVIDENCE_TEMPLATE_FILE)
        .expect("quality gate evidence template must be readable for enforcement");

    for field in REQUIRED_EVIDENCE_FIELDS {
        assert!(
            template.contains(field),
            "quality gate evidence template missing required field: {field}",
        );
    }
}

#[test]
fn architecture_boundary_notes_present() {
    assert!(
        Path::new(ARCHITECTURE_BOUNDARIES_FILE).exists(),
        "architecture boundary notes artifact missing: {ARCHITECTURE_BOUNDARIES_FILE}",
    );

    let architecture_notes = fs::read_to_string(ARCHITECTURE_BOUNDARIES_FILE)
        .expect("architecture boundary notes must be readable for closure enforcement");

    for section in REQUIRED_ARCHITECTURE_SECTIONS {
        assert!(
            architecture_notes.contains(section),
            "architecture boundary notes missing required section: {section}",
        );
    }
}
