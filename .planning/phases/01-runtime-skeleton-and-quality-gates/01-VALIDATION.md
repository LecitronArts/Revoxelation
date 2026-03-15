---
phase: 01
slug: runtime-skeleton-and-quality-gates
status: draft
nyquist_compliant: false
wave_0_complete: false
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
| **Quick run command** | `cargo test --quiet stage_order -- --nocapture` |
| **Full suite command** | `cargo test --all-targets --all-features` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --quiet stage_order -- --nocapture`
- **After every plan wave:** Run `cargo test --all-targets --all-features`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01-01 | 1 | ECS-01 | integration | `cargo test --quiet stage_order -- --nocapture` | no (W0) | pending |
| 01-01-02 | 01-01 | 1 | ECS-02 | unit | `cargo test --quiet registration_boundary -- --nocapture` | no (W0) | pending |
| 01-02-01 | 01-02 | 2 | ECS-03 | unit | `cargo test --quiet event_serde_roundtrip -- --nocapture` | no (W0) | pending |
| 01-02-02 | 01-02 | 2 | QUAL-01 | integration | `cargo test --quiet quality_gate_artifacts -- --nocapture` | no (W0) | pending |

*Status: pending | green | red | flaky*

---

## Wave 0 Requirements

- [ ] `tests/phase1_stage_order.rs` - stage sequence and single-frame execution assertions for ECS-01.
- [ ] `tests/phase1_registration_boundaries.rs` - domain boundary registration checks for ECS-02.
- [ ] `tests/phase1_events.rs` - serde roundtrip and event sequencing checks for ECS-03.
- [ ] `tests/phase1_quality_gates.rs` - artifact existence and gate evidence checks for QUAL-01.

*Existing Cargo test infrastructure is present; no framework installation required.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| One-frame runtime trace is readable and reflects stage semantics | ECS-01 | Human readability and semantic sanity are not fully machine-checkable | Run one frame with logging enabled and confirm ordered trace entries for all required stages |

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all missing test references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
