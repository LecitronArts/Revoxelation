use std::path::Path;

const STRUCTURED_LOG_FIELDS: [&str; 4] = ["frame_index", "stage", "transition", "sequence"];
const TRACE_FILE: &str = "src/runtime/trace.rs";
const HUD_OVERLAY_FILE: &str = "src/runtime/observability/hud.rs";
const HUD_OVERLAY_ANCHOR: &str = "overlay";

#[test]
fn wave0_observability_selector_bootstrap() {
    assert_eq!(STRUCTURED_LOG_FIELDS[0], "frame_index");
    assert_eq!(STRUCTURED_LOG_FIELDS[1], "stage");
    assert_eq!(STRUCTURED_LOG_FIELDS[2], "transition");
    assert_eq!(STRUCTURED_LOG_FIELDS[3], "sequence");

    if Path::new(TRACE_FILE).exists() {
        let trace_source =
            std::fs::read_to_string(TRACE_FILE).expect("trace source should be readable when present");

        for field in ["frame_index=", "stage=", "transition=", "sequence="] {
            assert!(
                trace_source.contains(field),
                "structured log anchor drifted: missing `{field}`",
            );
        }
    }

    if Path::new(HUD_OVERLAY_FILE).exists() {
        let hud_source = std::fs::read_to_string(HUD_OVERLAY_FILE)
            .expect("HUD/overlay source should be readable when present");
        assert!(
            hud_source.contains(HUD_OVERLAY_ANCHOR),
            "HUD/overlay source drifted: missing `overlay` anchor",
        );
    } else {
        assert_eq!(
            HUD_OVERLAY_ANCHOR, "overlay",
            "overlay anchor fixture must remain stable before HUD implementation lands",
        );
    }
}
