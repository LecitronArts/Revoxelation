---
phase: 6
slug: meshlet-pipeline
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-27
---

# Phase 6 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | Cargo.toml |
| **Quick run command** | `cargo test --lib` |
| **Full suite command** | `cargo test --all-targets` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib`
- **After every plan wave:** Run `cargo test --all-targets`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 06-01-01 | 01 | 1 | MSHL-01 | unit | `cargo test meshlet_generation` | ❌ W0 | ⬜ pending |
| 06-01-02 | 01 | 1 | MSHL-01 | unit | `cargo test meshlet_bounds` | ❌ W0 | ⬜ pending |
| 06-02-01 | 02 | 2 | MSHL-02 | integration | `cargo test meshlet_culling` | ❌ W0 | ⬜ pending |
| 06-02-02 | 02 | 2 | MSHL-02 | unit | `cargo test cull_toggle` | ❌ W0 | ⬜ pending |
| 06-03-01 | 03 | 2 | MSHL-03 | integration | `cargo test compute_indirect_path` | ❌ W0 | ⬜ pending |
| 06-04-01 | 04 | 3 | MSHL-04 | integration | `cargo test mesh_shader_fallback` | ❌ W0 | ⬜ pending |
| 06-05-01 | 05 | 3 | MSHL-05 | unit | `cargo test lod_dag` | ❌ W0 | ⬜ pending |
| 06-05-02 | 05 | 3 | MSHL-05 | manual | N/A (visual) | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `tests/phase6_meshlet.rs` — stubs for MSHL-01 (meshlet generation, bounds correctness)
- [ ] `tests/phase6_culling.rs` — stubs for MSHL-02 (culling modes, toggle behavior)
- [ ] `tests/phase6_pipeline.rs` — stubs for MSHL-03, MSHL-04 (compute vs mesh shader paths)
- [ ] `tests/phase6_lod.rs` — stubs for MSHL-05 (LOD DAG, boundary vertex sharing)

*Existing test infrastructure from Phase 1-5 covers framework setup. No new framework installation needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| LOD transition visual seamlessness | MSHL-05 | Requires visual inspection of rendered output | Fly camera through LOD transition zones, verify no visible seams or popping |
| Mesh shader vs compute path visual parity | MSHL-03/MSHL-04 | Requires GPU with VK_EXT_mesh_shader | Toggle paths in egui HUD, compare rendering output visually |
| Alpha dither quality | MSHL-05 | Subjective visual quality assessment | Observe LOD transitions at varying distances, verify smooth dither pattern |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
