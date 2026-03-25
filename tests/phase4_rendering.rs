//! Phase 4 rendering foundation tests.
//!
//! Source-grep tests that verify the OnceLock global state elimination
//! and App struct ownership model introduced by Plan 04-01.

// ---------------------------------------------------------------------------
// Task 1 tests
// ---------------------------------------------------------------------------

/// After Task 1: globals.rs deleted, no OnceLock in renderer/.
#[test]
fn rend_06_no_oncelock_in_renderer() {
    let renderer_dir = std::path::Path::new("src/renderer");

    // globals.rs should not exist.
    assert!(
        !renderer_dir.join("globals.rs").exists(),
        "src/renderer/globals.rs should be deleted"
    );

    // Check all .rs files in src/renderer/ for OnceLock.
    let mut oncelock_count = 0;
    for entry in std::fs::read_dir(renderer_dir).expect("src/renderer/ should exist") {
        let entry = entry.expect("directory entry should be readable");
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("should be able to read {}", path.display()));
            oncelock_count += content.matches("OnceLock").count();
        }
    }

    assert_eq!(
        oncelock_count, 0,
        "no file in src/renderer/ should contain OnceLock"
    );
}

#[test]
fn rend_06_app_struct_owns_renderer() {
    let app_source =
        std::fs::read_to_string("src/app.rs").expect("src/app.rs should exist");
    assert!(
        app_source.contains("struct App"),
        "src/app.rs should define struct App"
    );
    assert!(
        app_source.contains("renderer") && app_source.contains("Renderer"),
        "App struct should have a renderer: Renderer field"
    );
}

#[test]
fn rend_06_renderer_mod_no_globals_reexport() {
    let mod_source =
        std::fs::read_to_string("src/renderer/mod.rs").expect("src/renderer/mod.rs should exist");
    assert!(
        !mod_source.contains("pub mod globals"),
        "renderer/mod.rs should not declare pub mod globals"
    );
    assert!(
        !mod_source.contains("install_renderer"),
        "renderer/mod.rs should not re-export install_renderer"
    );
    assert!(
        !mod_source.contains("renderer_state"),
        "renderer/mod.rs should not re-export renderer_state"
    );
}

// ---------------------------------------------------------------------------
// Task 2 tests
// ---------------------------------------------------------------------------

#[test]
fn rend_06_scheduler_has_no_global_state() {
    let scheduler_source = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("src/runtime/scheduler.rs should exist");

    // No static OnceLock declarations.
    assert!(
        !scheduler_source.contains("OnceLock"),
        "scheduler.rs should not contain OnceLock"
    );

    // No static STREAMING or MESHING.
    assert!(
        !scheduler_source.contains("static STREAMING"),
        "scheduler.rs should not declare static STREAMING"
    );
    assert!(
        !scheduler_source.contains("static MESHING"),
        "scheduler.rs should not declare static MESHING"
    );
}

#[test]
fn rend_06_run_frame_takes_mutable_refs() {
    let scheduler_source = std::fs::read_to_string("src/runtime/scheduler.rs")
        .expect("src/runtime/scheduler.rs should exist");

    assert!(
        scheduler_source.contains("streaming: &mut StreamingState"),
        "run_frame should accept streaming: &mut StreamingState parameter"
    );
    assert!(
        scheduler_source.contains("meshing: &mut MeshingState"),
        "run_frame should accept meshing: &mut MeshingState parameter"
    );
    assert!(
        scheduler_source.contains("renderer: &mut Renderer")
            || scheduler_source.contains("renderer: &mut crate::renderer::Renderer"),
        "run_frame should accept renderer: &mut Renderer parameter"
    );
}

#[test]
fn rend_06_app_struct_owns_all_subsystems() {
    let app_source =
        std::fs::read_to_string("src/app.rs").expect("src/app.rs should exist");
    assert!(
        app_source.contains("streaming") && app_source.contains("StreamingState"),
        "App struct should have a streaming: StreamingState field"
    );
    assert!(
        app_source.contains("meshing") && app_source.contains("MeshingState"),
        "App struct should have a meshing: MeshingState field"
    );
}
