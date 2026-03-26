---
phase: 5
slug: bindless-architecture-and-gpu-scene
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-26
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | Cargo.toml |
| **Quick run command** | `cargo test --lib -- phase5` |
| **Full suite command** | `cargo test -- phase5` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --lib -- phase5`
- **After every plan wave:** Run `cargo test -- phase5`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 05-01-01 | 01 | 1 | BIND-01 | unit | `cargo test -- phase5_vulkan12_feature_check` | W0 | pending |
| 05-01-02 | 01 | 1 | BIND-01 | unit | `cargo test -- phase5_graceful_error_missing_features` | W0 | pending |
| 05-02-01 | 02 | 1 | BIND-02 | unit | `cargo test -- phase5_bindless_table_creation` | W0 | pending |
| 05-02-02 | 02 | 1 | BIND-02 | integration | `cargo test -- phase5_shared_set0_pipelines` | W0 | pending |
| 05-03-01 | 03 | 2 | BIND-03 | unit | `cargo test -- phase5_scene_buffer_layout` | W0 | pending |
| 05-03-02 | 03 | 2 | BIND-03 | integration | `cargo test -- phase5_buffer_count_reduction` | W0 | pending |
| 05-04-01 | 04 | 2 | BIND-04 | unit | `cargo test -- phase5_block_material_ssbo` | W0 | pending |
| 05-04-02 | 04 | 2 | BIND-04 | visual | manual render check | N/A | pending |
| 05-05-01 | 05 | 3 | BIND-05 | unit | `cargo test -- phase5_dynamic_capacity_grow` | W0 | pending |
| 05-05-02 | 05 | 3 | BIND-05 | integration | `cargo test -- phase5_indirect_count_draw` | W0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `tests/phase5_bindless.rs` — stubs for BIND-01 through BIND-05
- [ ] Feature check unit tests (mock device without Vulkan 1.2 features)
- [ ] BindlessTable creation and binding verification tests
- [ ] Scene buffer layout and size calculation tests
- [ ] BlockMaterial SSBO packing tests
- [ ] Dynamic capacity growth trigger and buffer copy tests

*Existing cargo test infrastructure covers framework setup.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Different block_ids show distinct textures | BIND-04 | Visual rendering output | Run app, observe 8 block types render with different face textures |
| Rendering output unchanged after buffer merge | BIND-03 | Visual regression | Compare screenshots before/after scene buffer migration |
| No visible artifacts during capacity grow | BIND-05 | Runtime visual | Load >1024 chunks, verify no flickering during buffer growth |

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
