---
phase: 01
slug: runtime-skeleton-and-quality-gates
status: ready
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-15
---

# Phase 01 - Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust `cargo test` |
| **Config file** | none - default Cargo test harness |
| **Quick run command** | `cargo test --quiet wave0_ -- --nocapture` |
| **Fast smoke command** | `cargo test --quiet stage_order_locked_to_input_sim_world_meshsync_render -- --nocapture` |
| **Full suite command** | `cargo test --all-targets --all-features` |
| **Estimated runtime** | quick: 5-15s, smoke: 10-25s, full: 45-90s |

---

## Sampling Cadence

- **After every task commit:** Run that task's selector command from the table below.
- **After every 2 tasks:** Run `cargo test --quiet wave0_ -- --nocapture`.
- **After each wave completion:** Run one fast deterministic smoke selector from the active requirement area.
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 45 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01-01 | 0 | ECS-01 | smoke | `cargo test --quiet wave0_stage_selector_bootstrap -- --nocapture` | created in task | pending |
| 01-01-02 | 01-01 | 0 | ECS-02/ECS-03 | smoke | `cargo test --quiet wave0_boundary_selector_bootstrap -- --nocapture` | created in task | pending |
| 01-01-03 | 01-01 | 0 | QUAL-01 | smoke | `cargo test --quiet wave0_quality_gate_selector_bootstrap -- --nocapture` | created in task | pending |
| 01-02-01 | 01-02 | 1 | ECS-01 | integration | `cargo test --quiet stage_order_locked_to_input_sim_world_meshsync_render -- --nocapture` | yes (from 01-01) | pending |
| 01-02-02 | 01-02 | 1 | ECS-01 | unit | `cargo test --quiet structured_logs_include_frame_stage_event -- --nocapture` | yes (from 01-01) | pending |
| 01-02-03 | 01-02 | 1 | ECS-01 | unit | `cargo test --quiet hud_overlay_exposes_stage_progress -- --nocapture` | yes (from 01-01) | pending |
| 01-03-01 | 01-03 | 1 | ECS-02 | unit | `cargo test --quiet boundary_registers_in_domain_systems -- --nocapture` | yes (from 01-01) | pending |
| 01-03-02 | 01-03 | 1 | ECS-02 | unit | `cargo test --quiet boundary_rejects_cross_domain_registration -- --nocapture` | yes (from 01-01) | pending |
| 01-03-03 | 01-03 | 1 | ECS-02 | doc-check | `rg -n "Stage Spine|Boundary Contracts|Cross-Domain Rules|Observability Handoff|MeshSync" .planning/phases/01-runtime-skeleton-and-quality-gates/01-ARCHITECTURE-BOUNDARIES.md` | yes (created in 01-03) | pending |
| 01-04-01 | 01-04 | 2 | ECS-03 | unit | `cargo test --quiet event_serde_roundtrip_models -- --nocapture` | yes (from 01-01) | pending |
| 01-04-02 | 01-04 | 2 | ECS-03 | unit | `cargo test --quiet invalid_command_rejected_with_reason -- --nocapture` | yes (from 01-01) | pending |
| 01-04-03 | 01-04 | 2 | ECS-03 | integration | `cargo test --quiet one_frame_event_flow_is_monotonic -- --nocapture` | yes (from 01-01) | pending |
| 01-05-01 | 01-05 | 3 | QUAL-01 | integration | `cargo test --quiet quality_gate_artifacts_present -- --nocapture` | yes (from 01-01) | pending |
| 01-05-02 | 01-05 | 3 | QUAL-01 | integration | `cargo test --quiet architecture_boundary_notes_present -- --nocapture` | yes (from 01-03) | pending |
| 01-05-03 | 01-05 | 3 | QUAL-01 | smoke | `cargo test --quiet wave0_ -- --nocapture` | yes | pending |

*Status: pending | green | red | flaky*

---

## Wave 0 Requirements

- [x] `tests/phase1_stage_order.rs` - Wave 0 selector bootstrap and MeshSync naming lock fixture.
- [x] `tests/phase1_observability.rs` - Wave 0 selector bootstrap for structured log/HUD checks.
- [x] `tests/phase1_registration_boundaries.rs` - Wave 0 boundary selector bootstrap.
- [x] `tests/phase1_events.rs` - Wave 0 event selector bootstrap.
- [x] `tests/phase1_quality_gates.rs` - Wave 0 quality-gate selector bootstrap.

Wave 0 is complete at planning level via `01-01-PLAN.md`; implementation wave tasks consume these selectors.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| One-frame structured log readability and operator usefulness | ECS-01 | JSON/structured schema can be machine-checked, but readability and field clarity still need human review | Run one frame with logging enabled and confirm ordered stage transitions for Input -> Simulation -> WorldUpdate -> MeshSync -> RenderSubmit |
| HUD/overlay clarity for stage progression | ECS-01 | Automated tests confirm presence, not UX legibility during runtime observation | Run one frame and verify overlay text makes current stage and completed stages obvious |

---

## Validation Sign-Off

- [x] All tasks have runnable automated verify commands at task completion
- [x] Sampling continuity: no 2 consecutive tasks without automated verify
- [x] Wave 0 dependency chain closes missing test-reference gaps
- [ ] No watch-mode flags
- [x] Feedback latency target < 45s through selector-first cadence
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
