use std::collections::BTreeSet;
use std::path::Path;

const EVENTS_ENTRYPOINT: &str = "src/runtime/events/mod.rs";
const EVENT_FIXTURES: [&str; 3] = ["block_placed", "block_removed", "chunk_invalidated"];

#[test]
fn wave0_events_selector_bootstrap() {
    assert!(
        EVENTS_ENTRYPOINT.contains("events"),
        "events entrypoint fixture must keep `events` anchor",
    );

    let unique: BTreeSet<&str> = EVENT_FIXTURES.into_iter().collect();
    assert_eq!(
        unique.len(),
        EVENT_FIXTURES.len(),
        "Event fixtures must remain unique",
    );

    if Path::new(EVENTS_ENTRYPOINT).exists() {
        let event_source = std::fs::read_to_string(EVENTS_ENTRYPOINT)
            .expect("events source should be readable when present");
        assert!(
            event_source.to_lowercase().contains("event"),
            "events entrypoint drifted: expected event anchor token",
        );
    }
}
