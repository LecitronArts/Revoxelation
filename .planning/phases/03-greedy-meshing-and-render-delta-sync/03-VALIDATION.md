---
phase: 03
slug: greedy-meshing-and-render-delta-sync
status: ready
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-22
---

# Phase 03 - Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust unit tests + integration tests) |
| **Config file** | `Cargo.toml` |
| **Quick run command** | `cargo test --test phase3_meshing` |
| **Fast smoke command** | `cargo test --test phase3_gap_closure` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Cadence

- **After every task commit:** Run that task's exact selector from the table below.
- **After each wave completion:** Run the suite for that wave:
  `cargo test --test phase3_meshing` for `03-01` and `03-02`,
  `cargo test --test phase3_gap_closure` for `03-03` through `03-06`.
- **After every 2 tasks in a row:** Run `cargo test`.
- **Before `$gsd-verify-work`:** Full suite must be green.
- **Max feedback latency:** 15 seconds at selector level.

---

## Wave Ordering

- **Wave 1:** `03-01`
- **Wave 2:** `03-02` depends on `03-01`
- **Wave 3:** `03-03` depends on `03-02`
- **Wave 4:** `03-04` depends on `03-03`
- **Wave 5:** `03-05` depends on `03-04`
- **Wave 6:** `03-06` depends on `03-05`

The dependency chain is intentionally linear because each later gap-closure plan
consumes renderer/bootstrap contracts established by the previous plan.

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 03-01-01 | 03-01 | 1 | MESH-01 | unit | `cargo test --test phase3_meshing mesh_01_chunk_voxels_contract_and_packed_layout -- --exact` | yes | pending |
| 03-01-02 | 03-01 | 1 | MESH-02 | unit | `cargo test --test phase3_meshing mesh_02_border_invalidation_marks_neighbors -- --exact` | yes | pending |
| 03-01-03 | 03-01 | 1 | MESH-02 | unit | `cargo test --test phase3_meshing mesh_02_coarse_chunk_generates_skirts_only_for_flagged_faces -- --exact` | yes | pending |
| 03-02-01 | 03-02 | 2 | MESH-03 | unit | `cargo test --test phase3_meshing mesh_03_chunk_pool_slot_reuse_clears_metadata -- --exact` | yes | pending |
| 03-02-02 | 03-02 | 2 | MESH-03 | integration | `cargo test --test phase3_meshing mesh_03_deactivated_active_chunk_enqueues_remove_delta -- --exact` | yes | pending |
| 03-02-03 | 03-02 | 2 | MESH-01, MESH-03 | integration | `cargo test --test phase3_meshing mesh_03_build_script_and_indirect_submit_contract -- --exact` | yes | pending |
| 03-03-01 | 03-03 | 3 | MESH-01, MESH-03 | integration | `cargo test --test phase3_gap_closure mesh_03_dense_draw_list_swap_removes_sparse_slot_holes -- --exact` | yes | pending |
| 03-03-02 | 03-03 | 3 | MESH-01 | integration | `cargo test --test phase3_gap_closure mesh_03_vertex_shader_uses_metadata_for_world_placement -- --exact` | yes | pending |
| 03-04-01 | 03-04 | 4 | MESH-01, MESH-03 | integration | `cargo test --test phase3_gap_closure mesh_03_submit_frame_uses_dense_indirect_draw_count -- --exact` | yes | pending |
| 03-05-01 | 03-05 | 5 | MESH-01 | unit | `cargo test --test phase3_gap_closure mesh_01_missing_validation_layer_disables_optional_debug_bootstrap -- --exact` | existing file targeted; selector added during execution | green |
| 03-06-01 | 03-06 | 6 | MESH-01 | integration | `cargo test --test phase3_gap_closure mesh_01_spirv_word_decoder_accepts_unaligned_byte_input -- --exact` | existing file targeted; selector added during execution | green |

*Status: pending | green | red | flaky*

---

## Wave 0 Requirements

- [x] `tests/phase3_meshing.rs` exists and covers MESH-01, MESH-02, and MESH-03 selectors for the base Phase 3 plans.
- [x] `tests/phase3_gap_closure.rs` exists and reserves frame range `3100..3199` for `03-03` through `03-06` gap-closure selectors.
- [x] Scheduler/frame-index notes are explicit: `tests/phase3_meshing.rs` reserves `3000..3099`, and `tests/phase3_gap_closure.rs` reserves `3100..3199`.
- [x] The current plan set has a concrete selector target and file location for every task from `03-01` through `03-06`; no Phase 3 plan points at a missing test file.

Wave 0 is complete at planning level: the missing test-reference gap is closed,
and the remaining work is execution/verification of the later selectors.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Shader-module startup no longer panics on SPIR-V byte alignment and the real Vulkan window path still opens whether or not `VK_LAYER_KHRONOS_validation` is installed | MESH-01 | Requires live Vulkan runtime plus machine-specific layer availability | Run `cargo run`; confirm stderr does not contain `TargetAlignmentGreaterAndInputNotAligned`, then verify either the window opens directly or the next blocker appears after shader-module creation |
| Chunk surfaces render in the window with no visible seams at chunk borders | MESH-01, MESH-02 | Requires live Vulkan device plus visual inspection | Run `cargo run`, move through active chunks, and verify visible surfaces render while chunk edges do not show holes |
| Dirty-chunk updates avoid full-world reupload hitches | MESH-03 | Requires runtime behavior and instrumentation/log observation | Trigger several chunk updates and confirm logs or counters show slot-level uploads/command updates rather than world-wide buffer rebuilds |

---

## Validation Sign-Off

- [x] All tasks have runnable automated verify commands or an explicit existing-file dependency.
- [x] Sampling continuity: every task has an exact selector, and each wave has a suite-level follow-up command.
- [x] Wave 0 covers all missing test-reference gaps for `03-01` through `03-06`.
- [x] No watch-mode flags.
- [x] Feedback latency target < 15s at selector level and acceptable for `cargo test`.
- [x] `nyquist_compliant: true` set in frontmatter.

**Approval:** pending human verification
