# Phase 1 Research: Runtime Skeleton and Quality Gates

## Planning Intent
This research answers: "What do I need to know to PLAN this phase well?" for Phase 1 only.

Phase scope is foundation work, not gameplay delivery:
- deterministic frame stage skeleton
- enforceable subsystem registration boundaries
- serializable runtime command/event contracts
- mandatory workflow quality gates with evidence artifacts

## Implementation Architecture Notes and Constraints

### 1) Runtime Skeleton Contract (must be fixed and testable)
Define one frame contract with strict order and no dynamic stage insertion:
1. `Input`
2. `Simulation`
3. `WorldUpdate`
4. `MeshingSync`
5. `RenderSubmit`

Constraints:
- Stage order must be represented as a single source of truth (enum + ordered static array, or explicit runner list).
- Stage transitions must be observable (structured trace entries per stage begin/end).
- Development/test builds should hard-fail on invalid stage transitions or duplicate stage execution in one frame.
- Keep dataflow explicit at stage boundaries; avoid hidden mutable singleton state.

### 2) System Registration Boundaries (ECS-02)
Define narrow registration surfaces by domain and prevent direct cross-domain dependency:
- `world` boundary: authoritative chunk/player world state mutations.
- `meshing` boundary: mesh invalidation and mesh-ready intake only.
- `collision` boundary: read-only spatial query and movement resolution outputs.
- `persistence` boundary: dirty-state and flush/load orchestration only.

Recommended shape:
- `runtime/stages/*` or `ecs/stages/*`: stage runner and registry.
- `runtime/systems/<domain>/*`: systems grouped by domain boundary.
- `runtime/ports.rs` (or similar): traits/interfaces each boundary may call.

Coupling rules:
- A system can depend on shared contracts (`events`, `commands`, `ports`), not concrete modules from other domains.
- Renderer/GPU types must not appear in domain event payloads.
- Cross-boundary communication goes through typed commands/events or trait ports.

### 3) Command/Event Contract (ECS-03)
Use separate intent and fact streams:
- `Command`: requested action (`MoveIntent`, `EditBlock`, `RequestChunkActivate`)
- `Event`: observed fact (`PlayerMoved`, `BlockEdited`, `ChunkActivated`, `ChunkMeshed`)

Contract requirements:
- All public command/event payloads derive `Serialize` and `Deserialize`.
- Include deterministic ordering metadata: `frame_index`, `sequence` (monotonic within frame or global monotonic).
- Include stable identifiers (`entity_id`, `chunk_coord`, optional `revision`).
- Validation failures must be explicit and logged (never silently dropped).

Minimum Phase 1 event families:
- Player actions: movement/interaction intent and applied result.
- Chunk lifecycle: activation/deactivation/mesh-ready transitions.
- Block edits: command accepted/rejected and applied fact event.

### 4) Quality Gate Architecture (QUAL-01)
Phase 1 must embed workflow gates as artifacts, not informal notes.

Required gates in execution flow:
- `writing-plans`
- `test-driven-development`
- `systematic-debugging` (only when blocked/failing)
- `verification-before-completion`
- `requesting-code-review`
- `receiving-code-review`
- `finishing-a-development-branch`

Enforcement pattern:
- Each plan in phase 1 has a "Gate Checklist" section with pass/fail evidence.
- Completion claims are invalid without command evidence and result summaries.
- Exceptions require explicit record: reason, risk, remediation owner.

### 5) Current Repository Reality Constraint
Planning docs describe an existing `src/*` architecture, but this workspace currently has an empty `src/` directory.

Planning impact:
- Phase 1 plan must include a bootstrap skeleton task before boundary/event tasks.
- Use architecture docs as target structure, but validate actual file existence before referencing implementation paths in PLAN tasks.

## Risks and Mitigations

1. Risk: deterministic stage drift as new systems are added.
- Mitigation: lock stage runner API; add stage-order integration smoke test and per-frame stage trace assertion.

2. Risk: cross-module coupling introduced through convenience imports.
- Mitigation: domain port traits + module boundary linting/review checklist; forbid direct imports across domain implementation modules.

3. Risk: non-serializable or renderer-leaking event payloads.
- Mitigation: dedicated `runtime/events` crate/module with serde derives and no renderer dependency allowed.

4. Risk: event ordering non-determinism from mixed async completions.
- Mitigation: main-thread event commit point in `WorldUpdate`/`MeshingSync`; workers return data only.

5. Risk: quality gates become paperwork and get skipped.
- Mitigation: gate artifacts required in phase directory, and "verification-before-completion" evidence required before marking plan done.

6. Risk: scope creep into streaming/meshing feature delivery.
- Mitigation: Phase 1 does placeholder systems and contracts only; no deep streaming/meshing implementation.

7. Risk: mismatch between planning assumptions and actual codebase state.
- Mitigation: first executable plan task should validate/create runtime skeleton files and update planning references.

## Plan Decomposition Guidance for Executable PLAN.md Files

Use the roadmap split (2 plans) and keep each plan independently verifiable.

### Plan 01-01: Deterministic Scheduler + Boundary Registration
Goal:
- deliver fixed frame stage runner
- implement boundary-safe registration APIs

Recommended task slices:
1. Create runtime skeleton modules and stage enum/order source of truth.
2. Implement stage runner with stage transition tracing.
3. Add domain registration surfaces (`world`, `meshing`, `collision`, `persistence`) and compile-time boundaries.
4. Add placeholder systems for each stage and a one-frame smoke run harness.
5. Add tests for fixed order and boundary registration behavior.

Exit criteria:
- One frame executes in required stage order.
- System registration demonstrates domain boundaries without cross-module imports.

### Plan 01-02: Serializable Events + Quality Gate Hooks
Goal:
- deliver command/event contracts for required domains
- embed enforceable workflow artifacts for QUAL-01

Recommended task slices:
1. Define command and event schemas for player actions/chunk lifecycle/block edits.
2. Implement deterministic sequencing metadata and validation/rejection paths.
3. Add serialization roundtrip tests and ordering assertions.
4. Create phase workflow artifacts/checklists for all required superpowers gates.
5. Add integration smoke path proving emit/consume flow across stage runner.

Exit criteria:
- Runtime can emit and consume required serializable events.
- Workflow artifacts prove gate enforcement with command evidence.

### PLAN.md authoring pattern (for both plans)
Each executable PLAN.md should include:
- Scope boundaries and explicit non-goals.
- Atomic tasks with file ownership.
- Verification commands and expected signals.
- Gate checklist with evidence fields:
  - command run
  - key output
  - pass/fail
  - reviewer/owner notes
- Rollback or remediation notes for failed validation.

## Validation Architecture

Validation should mix fast unit checks with one deterministic integration smoke path.

### Unit-level validation
- Stage order tests:
  - assert exact sequence `Input -> Simulation -> WorldUpdate -> MeshingSync -> RenderSubmit`.
  - assert no stage duplication in a single frame.
- Boundary tests:
  - registration accepts in-domain systems.
  - registration rejects/disallows cross-domain coupling paths.
- Event schema tests:
  - serde roundtrip for each command/event type.
  - rejection path tests for invalid payload/state.
- Ordering tests:
  - monotonic `sequence` behavior within deterministic frame progression.

### Integration validation
- One-frame smoke test that:
  1. registers placeholder systems by boundary
  2. executes one frame
  3. emits sample player/chunk/edit events
  4. consumes events at expected stage boundary
  5. records deterministic trace output

### Quality-gate validation
- Evidence file(s) in phase directory capturing:
  - which gate ran
  - command(s)
  - output summary
  - status
- Hard blockers:
  - missing gate evidence
  - failed tests without documented systematic-debugging artifact
  - completion claim without verification-before-completion output

## Requirement Coverage Map

| Requirement | What must be implemented in Phase 1 | Validation evidence |
|---|---|---|
| ECS-01 | Fixed deterministic stage runner with exact required order and frame execution hook | Stage-order unit test + one-frame integration trace proving required order |
| ECS-02 | Domain-constrained registration boundaries (`world/meshing/collision/persistence`) with no direct cross-module coupling | Registration boundary tests + code review checklist confirming boundary rules |
| ECS-03 | Serializable command/event contracts for player actions, chunk lifecycle, and block edits with deterministic sequencing | Serde roundtrip tests + emit/consume integration smoke path + ordering assertions |
| QUAL-01 | Workflow artifacts enforcing required superpowers gates across plan lifecycle | Gate checklist artifacts with command evidence, pass/fail, and remediation notes |

## Planning Checklist for Next Step
Before writing PLAN.md files, confirm:
- exact runtime skeleton file layout to create in this repository state
- ownership split between 01-01 and 01-02 with no overlap ambiguity
- verification commands available in current toolchain
- artifact filenames/locations for gate evidence under this phase directory