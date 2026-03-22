# CONCERNS

## Scope
- Focus: technical debt, reliability, performance, and maintainability risks observed in the current `ash`/Vulkan runtime.
- Source scope: Rust host code under `src/` plus the build-time shader pipeline in `build.rs`.

## High-Risk Concerns
1. Window resize and surface-reconfigure handling are still missing.
- Evidence: `src/app.rs` handles `CloseRequested` and `RedrawRequested`, but not resize events; swapchain extent and graphics viewport are created once during startup.
- Risk: resize behavior can become incorrect or fail outright on real desktop use.

2. Render-submit failures are swallowed.
- Evidence: `src/runtime/scheduler.rs` calls `submit_frame` with `let _ = ...` inside `RenderSubmit`.
- Risk: the app can keep running while presentation or Vulkan submission errors are silently discarded.

3. Global singleton state makes lifecycle resets and tests fragile.
- Evidence: renderer, streaming state, and meshing state are all stored in `OnceLock<Mutex<...>>`.
- Risk: process-global state is hard to reinitialize cleanly, and tests must coordinate around shared state rather than isolated fixtures.

4. The egui path is still scaffolding, not a full rendered UI.
- Evidence: `EguiAshBackend::new` initializes `pipeline` to `vk::Pipeline::null()`, `paint` currently focuses on uploads/scratch buffers, and `src/app.rs` passes empty egui primitives each frame.
- Risk: readers may assume a working in-app debug UI exists when the current path is only partial plumbing.

## Reliability / Fragility
1. Stage tracing is easy to miss because logging is not initialized in the binary entrypoint.
- Evidence: `src/runtime/scheduler.rs` emits `log::info!`, but `src/main.rs` does not call `env_logger::init()`.
- Risk: operational visibility is weaker than dependency choices imply.

2. Vulkan feature requirements are strict.
- Evidence: `src/renderer/device.rs` requires `samplerAnisotropy`, `multiDrawIndirect`, and `drawIndirectFirstInstance`.
- Risk: integrated or older GPUs may be rejected even if they could run a reduced-feature path.

3. Several state transitions intentionally ignore errors.
- Evidence: scheduler paths use `let _ = state_store.transition_to(...)` and similar fire-and-forget calls.
- Risk: invalid lifecycle edges can disappear into silent no-op behavior unless a test catches them.

4. Chunk rendering capacity is fixed at compile time.
- Evidence: `src/renderer/chunk_pool.rs` fixes `MAX_RENDER_CHUNKS` and `MAX_QUADS_PER_CHUNK`.
- Risk: dense scenes can exhaust slot capacity or per-slot geometry budgets without any adaptive fallback.

## Performance Debt
1. The redraw strategy is effectively a busy frame loop.
- Evidence: `src/app.rs` requests a redraw on every `Event::AboutToWait`.
- Risk: unnecessary CPU/GPU work and poor idle behavior on laptops or low-power systems.

2. One-shot copy helpers stall the graphics queue.
- Evidence: `submit_one_shot_commands` in `src/renderer/mod.rs` ends each upload with `queue_wait_idle`.
- Risk: upload-heavy paths serialize GPU work and hurt frame pacing.

3. Chunk draw buffers use CPU-visible allocations for simplicity.
- Evidence: `src/renderer/chunk_pool.rs` allocates vertex, index, metadata, and indirect buffers with `MemoryLocation::CpuToGpu`.
- Risk: this is easy to update, but it leaves performance on the table versus staged GPU-only buffers for large scenes.

4. Meshing work is still performed on the main thread during `MeshSync`.
- Evidence: `src/runtime/scheduler.rs` runs `build_greedy_mesh` in the frame loop after background generation results arrive.
- Risk: large dirty batches can steal frame time even when chunk generation itself is already offloaded.

## Security and Process Notes
1. Runtime attack surface is currently low.
- Evidence: there are no network listeners, remote APIs, or database connectors in the live codebase.

2. Tooling hardening is still light.
- Evidence: dependencies are pinned in Cargo, but there is no visible CI or automated dependency-audit setup in the repository root.
- Risk: regressions or vulnerable crate updates rely on manual detection.

## Practical Mitigation Priorities
1. Add resize/recreate handling for swapchain-dependent renderer state.
2. Surface `submit_frame` failures to the app loop instead of discarding them.
3. Decide whether global `OnceLock` state is a temporary phase scaffold or a longer-term runtime choice.
4. Replace queue-idle upload helpers and CPU-visible draw buffers when chunk counts grow.
5. Either complete egui rendering or clearly mark it as non-production scaffolding in runtime-facing docs.
