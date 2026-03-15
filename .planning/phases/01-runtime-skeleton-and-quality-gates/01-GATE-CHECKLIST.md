# Phase 01 Quality Gate Checklist

This checklist is mandatory for closing Phase 01 work. Quality gates are hard blockers, and completion claims are invalid without reproducible evidence.

## Required Gates

- [x] `writing-plans`
- [ ] `test-driven-development`
- [ ] `systematic-debugging`
- [ ] `verification-before-completion`
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