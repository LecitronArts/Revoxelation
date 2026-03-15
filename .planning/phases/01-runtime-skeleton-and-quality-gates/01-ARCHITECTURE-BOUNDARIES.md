# Phase 01 Runtime Architecture and Boundaries

## Stage Spine

Phase 01 locks scheduler stage order to:

1. Input
2. Simulation
3. WorldUpdate
4. MeshSync
5. RenderSubmit

`MeshSync` is a fixed stage identifier and must remain unchanged across runtime code, tests, and documentation to preserve deterministic selector behavior.

## Boundary Contracts

Runtime system registration is constrained to four explicit domain registries owned by `RuntimeBoundaryRegistry`:

- `world`: authoritative world-state and player-state mutation systems.
- `meshing`: mesh invalidation and mesh-ready intake systems.
- `collision`: collision/spatial query and movement resolution systems.
- `persistence`: dirty-state, save/load orchestration systems.

Each system declares its owning domain through `DomainSystem::DOMAIN` and registers through a typed boundary interface (`world_mut().register::<T>()`, `meshing_mut().register::<T>()`, etc.).

## Cross-Domain Rules

Boundary registration enforces fail-fast rules in `BoundaryRegistryCore`:

- Cross-domain registration is rejected when a system's declared domain does not match the target boundary.
- Duplicate registration is rejected when a system name is already present in the target boundary.
- Rejections return deterministic `RegistrationError.reason` strings so failures are stable for tests and future diagnostics.

These guards prevent silent coupling drift and ensure boundary violations are visible at registration time, not deferred to runtime side effects.

## Observability Handoff

Boundary failures surface through explicit error values (`Result<(), RegistrationError>`) from placeholder and future system registration paths.

Handoff expectations for later phases:

- Structured runtime logs include registration failures with domain/system/reason context.
- HUD/overlay can render boundary failure summaries sourced from the same rejection reason strings.
- Diagnostics consumers must treat rejection messages as deterministic interfaces suitable for assertions and operator feedback.