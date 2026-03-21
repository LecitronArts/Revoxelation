---
phase: 03
slug: greedy-meshing-and-render-delta-sync
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-22
---

# Phase 03 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (Rust unit tests + integration tests) |
| **Config file** | `Cargo.toml` |
| **Quick run command** | `cargo test --test phase3_meshing` |
| **Full suite command** | `cargo test` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --test phase3_meshing`
- **After every plan wave:** Run `cargo test`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 03-01-01 | 01 | 1 | MESH-01 | unit | `cargo test --test phase3_meshing mesh_01_chunk_voxels_contract_and_packed_layout -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-02 | 01 | 1 | MESH-02 | unit | `cargo test --test phase3_meshing mesh_02_border_invalidation_marks_neighbors -- --exact` | ❌ W0 | ⬜ pending |
| 03-01-03 | 01 | 1 | MESH-02 | unit | `cargo test --test phase3_meshing mesh_02_coarse_chunk_generates_skirts_only_for_flagged_faces -- --exact` | ❌ W0 | ⬜ pending |
| 03-02-01 | 02 | 2 | MESH-03 | unit | `cargo test --test phase3_meshing mesh_03_chunk_pool_slot_reuse_clears_metadata -- --exact` | ❌ W0 | ⬜ pending |
| 03-02-02 | 02 | 2 | MESH-03 | integration | `cargo test --test phase3_meshing mesh_03_deactivated_active_chunk_enqueues_remove_delta -- --exact` | ❌ W0 | ⬜ pending |
| 03-02-03 | 02 | 2 | MESH-01, MESH-03 | integration | `cargo test --test phase3_meshing mesh_03_build_script_and_indirect_submit_contract -- --exact` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/phase3_meshing.rs` — requirement-level integration coverage for MESH-01, MESH-02, and MESH-03
- [ ] `src/meshing/greedy.rs` tests — greedy merge keys, halo reads, packed-quad edge cases, and per-face coarse-skirt generation/removal
- [ ] `src/renderer/chunk_pool.rs` tests — slot reuse, metadata clearing, and indirect command template updates
- [ ] Scheduler/frame-index test notes — reserve unique frame ranges because runtime integration tests share `OnceLock` global state, including the active-chunk deactivation -> remove-delta path

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Chunk surfaces render in the window with no visible seams at chunk borders | MESH-01, MESH-02 | Requires live Vulkan device + visual inspection | Run `cargo run`, move through active chunks, verify visible surfaces render and chunk edges do not show holes |
| LOD0 activation forces neighboring coarse chunks to regenerate skirts correctly | MESH-02 | Requires runtime camera movement and perceptual seam check | Move across an LOD boundary, verify the lower-detail chunk regenerates skirt geometry when the finer neighbor becomes active |
| Dirty-chunk updates avoid full-world reupload hitches | MESH-03 | Requires runtime behavior and instrumentation/log observation | Trigger several chunk updates, confirm logs or counters show slot-level uploads/command updates rather than world-wide buffer rebuilds |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
