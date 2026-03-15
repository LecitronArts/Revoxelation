# Phase 01 Quality Gate Checklist

This checklist is mandatory for closing Phase 01 work. Every gate requires evidence captured with the companion template.

## Required Gates

- [x] `writing-plans`
- [ ] `test-driven-development`
- [ ] `systematic-debugging`
- [x] `verification-before-completion`
- [ ] `requesting-code-review`
- [ ] `receiving-code-review`
- [ ] `finishing-a-development-branch`

## Gate Evidence Log

Use one row per gate execution.

| Gate | Command | Key Output | Pass/Fail | Owner | Explicit Reason | Risk | Remediation / Follow-Up Plan |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `writing-plans` | `cat .planning/phases/01-runtime-skeleton-and-quality-gates/01-05-PLAN.md` | plan objective/tasks/verification for QUAL-01 loaded | Pass | @codex | implementation required a concrete plan contract before edits | low | keep plan as source of truth for additional closeout edits |
| `verification-before-completion` | `cargo test --quiet wave0_ -- --nocapture` | all wave0 selectors passed; 0 failed | Pass | @codex | closure smoke gate must be green before full suite run | low | if smoke fails, fix failing selector and rerun smoke first |
| `verification-before-completion` | `cargo test --all-targets --all-features` | phase integration suite passed (events 4, observability 2, quality gates 4, boundaries 3, stage order 1) | Pass | @codex | phase closure requires complete test evidence, not spot checks | medium | if suite fails, block closure, capture failure artifact, and remediate before summary |