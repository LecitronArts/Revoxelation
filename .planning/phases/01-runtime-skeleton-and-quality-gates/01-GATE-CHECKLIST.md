# Phase 01 Quality Gate Checklist

This checklist is mandatory for closing Phase 01 work. Quality gates are hard blockers, and completion claims are invalid without reproducible evidence.

## Required Gates

- [x] `writing-plans`
- [ ] `test-driven-development`
- [x] `systematic-debugging`
- [x] `verification-before-completion`
- [ ] `requesting-code-review`
- [ ] `receiving-code-review`
- [ ] `finishing-a-development-branch`

## Enforcement Rules

1. Every completed gate must have an evidence row with command output summary and pass/fail status.
2. Exceptions must include explicit reason, risk, and remediation or follow-up plan before closure is allowed.

## Gate Evidence Log

Use one row per gate execution.

| Gate | Command | Key Output | Pass/Fail | Owner | Explicit Reason | Risk | Remediation / Follow-Up Plan |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `writing-plans` | `cat .planning/phases/01-runtime-skeleton-and-quality-gates/01-05-PLAN.md` | plan objective/tasks/verification loaded for execution | Pass | @codex | plan-first execution is required before artifact or test changes | low | treat this checklist and the companion template as mandatory closure artifacts |
| `systematic-debugging` | `cargo test --quiet architecture_boundary_notes_present -- --nocapture` | selector failed due malformed architecture section array (`unknown start of token: \`) | Pass | @codex | Task 2 edit introduced a blocking syntax regression and required root-cause correction | medium | repaired the array block, reran selector to green, and retained explicit closure-heading coverage |
| `verification-before-completion` | `cargo test --quiet quality_gate_artifacts_present -- --nocapture` | quality gate artifact selector passed (1 passed, 0 failed) | Pass | @codex | validate gate artifact continuity before closure smoke/full runs | low | if this regresses, restore required headings/fields and rerun selector before proceeding |
| `verification-before-completion` | `cargo test --quiet architecture_boundary_notes_present -- --nocapture` | architecture boundary notes selector passed (1 passed, 0 failed) | Pass | @codex | closure requires architecture continuity evidence alongside gate artifacts | low | if this regresses, restore required architecture headings and rerun selector |
| `verification-before-completion` | `cargo test --quiet wave0_ -- --nocapture` | wave0 smoke selectors passed (quality + stage bootstrap checks green) | Pass | @codex | run fast closure smoke before expensive full-suite verification | low | if smoke fails, fix failing selector first and rerun smoke before full suite |
| `verification-before-completion` | `cargo test --all-targets --all-features` | full suite passed (events 4, observability 2, quality gates 4, boundaries 3, stage order 1) | Pass | @codex | closure claims require complete reproducible test evidence, not spot checks | medium | if full suite fails, block closure, capture failure output, remediate root cause, rerun full suite |