# Voxel Engine Pitfalls (Revoxelation)

## Proposed Phase Legend

- `P1 Streaming Core`: Player-centered chunk load/unload loop, job queues, ECS boundaries.
- `P2 Meshing + Render Sync`: Greedy meshing, mesh invalidation, GPU upload/update flow.
- `P3 Movement + Collision`: Fly/gravity movement, broadphase/narrowphase collision integration.
- `P4 Edit Latency`: Block place/destroy path and near-immediate visual/world-state feedback.
- `P5 Persistence`: Save/load of modified chunks and restart consistency.
- `P6 Renderer-Heavy Integration`: Multi-pass renderer orchestration under continuous world mutation.

## 1) Streaming Thrash Around Player Velocity Spikes

- Warning signs:
  - Chunk load/unload counts oscillate every few frames when sprinting/flying.
  - Background worker queues grow unbounded during sharp direction changes.
  - CPU time shifts from gameplay systems to bookkeeping/synchronization.
- Prevention strategy:
  - Use hysteresis rings (`load_radius > keep_radius > unload_radius`) to avoid frequent churn.
  - Prioritize chunk jobs by distance and camera direction, with cancellation for stale requests.
  - Cap per-frame stream mutations and enforce backpressure from worker queue depth.
- Phase mapping: `P1 Streaming Core`, `P6 Renderer-Heavy Integration`.

## 2) Cross-Thread Chunk State Races (Generated vs Meshed vs Uploaded)

- Warning signs:
  - Rare "missing chunk" visuals despite generation success logs.
  - Intermittent panic/assert failures around chunk lifecycle transitions.
  - Duplicate mesh builds or stale mesh data pushed after a newer edit/generation revision.
- Prevention strategy:
  - Model chunk lifecycle as explicit monotonic state machine with revision IDs.
  - Require compare-and-swap style revision checks before meshing/upload commits.
  - Emit structured state-transition telemetry for postmortem reconstruction.
- Phase mapping: `P1 Streaming Core`, `P2 Meshing + Render Sync`, `P6 Renderer-Heavy Integration`.

## 3) Greedy Meshing Producing Topology Artifacts at Chunk Borders

- Warning signs:
  - Visible seams/cracks where adjacent chunks meet.
  - Faces disappear or overdraw at boundaries after neighboring chunk updates.
  - Artifact frequency increases after partial chunk edits near edges.
- Prevention strategy:
  - Mesh with neighbor-aware face visibility queries (including ghost boundary sampling).
  - Treat border edits as multi-chunk invalidation events.
  - Add deterministic seam regression tests using fixed seeds and camera paths.
- Phase mapping: `P2 Meshing + Render Sync`, `P4 Edit Latency`.

## 4) Mesh Rebuild Storms From Fine-Grained Edit Events

- Warning signs:
  - Single block edits trigger many chunk remeshes and multi-frame stutter.
  - Edit latency rises nonlinearly as build area density increases.
  - GPU upload bandwidth spikes during rapid place/destroy actions.
- Prevention strategy:
  - Coalesce edits per chunk over short windows (frame/tick-level debounce).
  - Use dirty-region or dirty-column tracking instead of full-chunk remesh when feasible.
  - Maintain per-frame remesh/upload budgets with priority for player-visible chunks.
- Phase mapping: `P2 Meshing + Render Sync`, `P4 Edit Latency`, `P6 Renderer-Heavy Integration`.

## 5) Collision Using Divergent Ground Truth From Rendered Voxels

- Warning signs:
  - Player clips into blocks or floats above visually solid surfaces.
  - Movement jitter appears only after edits or chunk streaming transitions.
  - Replay with same inputs yields different contact outcomes.
- Prevention strategy:
  - Use one canonical voxel occupancy source for both collision and meshing inputs.
  - Version collision queries against chunk revision snapshots for deterministic contacts.
  - Separate broadphase cache invalidation rules from render cache invalidation, but link both to the same edit events.
- Phase mapping: `P3 Movement + Collision`, `P4 Edit Latency`.

## 6) Tunneling/Step-Resolution Failures at High Tick Variance

- Warning signs:
  - Fast downward or diagonal motion intermittently bypasses collisions.
  - Grounding state flickers between frames, causing jump/gravity glitches.
  - Behavior degrades under frame drops or debug builds.
- Prevention strategy:
  - Use swept AABB (or equivalent continuous test) for movement integration.
  - Clamp max integration delta and substep physics when frame delta exceeds threshold.
  - Keep movement tick and render interpolation explicitly decoupled.
- Phase mapping: `P3 Movement + Collision`, `P6 Renderer-Heavy Integration`.

## 7) Edit Path Latency Hidden Behind Eventual Consistency

- Warning signs:
  - Input confirms placement/destruction, but world visual update lags multiple frames.
  - Rapid alternating edits show reorder anomalies (later action appears first).
  - Server/network-ready event abstractions delay local single-player feedback.
- Prevention strategy:
  - Apply immediate local authoritative mutation for the edited block before deferred rebuild work.
  - Use ordered per-chunk edit sequence numbers to preserve intent order.
  - Split "logical mutation ack" from "mesh/materialized update complete" telemetry.
- Phase mapping: `P4 Edit Latency`, `P2 Meshing + Render Sync`.

## 8) Persistence Snapshot Corruption Under Concurrent Streaming

- Warning signs:
  - Reload after restart reverts some recent edits in streamed-out zones.
  - Save files contain mixed revisions for neighboring chunks.
  - Rare startup failures when reading partially written chunk data.
- Prevention strategy:
  - Write chunk persistence through atomic temp-file + rename protocol.
  - Persist chunk revision and checksum metadata; reject/regenerate corrupted payloads.
  - Gate save operations on stable chunk snapshots (copy-on-write or lock-minimized snapshotting).
- Phase mapping: `P5 Persistence`, `P1 Streaming Core`.

## 9) Persistence Format Drift Without Migration Discipline

- Warning signs:
  - New builds cannot load old worlds, or silently misinterpret block IDs.
  - Runtime panics during deserialization after schema changes.
  - "Unknown block" placeholders spread after updates.
- Prevention strategy:
  - Version world/chunk schema explicitly and enforce migration-on-load path.
  - Maintain block registry compatibility contracts (stable IDs or translation tables).
  - Add golden save/load compatibility tests across supported schema versions.
- Phase mapping: `P5 Persistence`.

## 10) Renderer Pass Coupling to World Update Order

- Warning signs:
  - Flicker/pop-in when chunk meshes update during multi-pass frame execution.
  - Depth/shadow inconsistencies appear only during heavy streaming/edit bursts.
  - Render graph ordering changes unexpectedly alter world correctness.
- Prevention strategy:
  - Introduce frame-stable world snapshot handles consumed by all render passes.
  - Stage GPU buffer swaps at explicit sync points (double-buffered mesh resources).
  - Define strict producer/consumer contracts between world update and render graph phases.
- Phase mapping: `P6 Renderer-Heavy Integration`, `P2 Meshing + Render Sync`, `P1 Streaming Core`.

## 11) GPU Memory Fragmentation From Frequent Chunk Mesh Churn

- Warning signs:
  - VRAM usage climbs over time in large editing sessions.
  - Sporadic long stalls during buffer allocation/reallocation.
  - Performance degrades without obvious CPU bottleneck.
- Prevention strategy:
  - Use pooled sub-allocation arenas for chunk mesh buffers.
  - Recycle buffers by size class and defer destruction to safe frame-latency windows.
  - Track allocation lifetime metrics to tune chunk mesh packing strategy.
- Phase mapping: `P6 Renderer-Heavy Integration`, `P2 Meshing + Render Sync`.

## 12) Unbounded Integration Complexity at ECS/System Boundaries

- Warning signs:
  - Systems depend on each other through hidden side effects rather than explicit events.
  - Minor edits in streaming or meshing regress movement/collision stability.
  - Team velocity drops because ownership boundaries are unclear.
- Prevention strategy:
  - Define explicit contracts: input/output components, events, and allowed mutation stages.
  - Keep heavy async jobs behind narrow interfaces with deterministic handoff points.
  - Add end-to-end scenario tests that combine streaming + edits + movement + persistence in one loop.
- Phase mapping: `P1 Streaming Core`, `P2 Meshing + Render Sync`, `P3 Movement + Collision`, `P4 Edit Latency`, `P5 Persistence`, `P6 Renderer-Heavy Integration`.
