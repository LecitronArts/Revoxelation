# Revoxelation Coding Conventions

## Scope and language profile
- The codebase is Rust-first and exported from `src/lib.rs`, with a thin binary entry in `src/main.rs`.
- The crate uses Rust `edition = "2024"` in `Cargo.toml`.
- Current architectural conventions assume a `winit` application shell, a fixed-stage runtime, and an `ash`/Vulkan renderer.

## Module and file organization
- Top-level library modules are `app`, `runtime`, `streaming`, `meshing`, and `renderer`.
- `src/app.rs` stays focused on OS-facing startup and redraw plumbing; it should not own streaming policy or deep Vulkan setup details.
- `src/runtime/**` owns stage sequencing, domain boundaries, event/command contracts, and frame tracing.
- `src/streaming/**` owns chunk lifecycle state, octree math, SSE decisions, and job orchestration.
- `src/meshing/**` owns dirty propagation, neighbor-aware greedy meshing, and packed geometry formats.
- `src/renderer/*.rs` is intentionally flat rather than nested; each file owns a single Vulkan concern such as instance creation, device selection, swapchain management, chunk buffers, or pipeline setup.

## Naming conventions
- Types use `PascalCase`: `Renderer`, `ChunkKey`, `ChunkJobOutcome`, `RuntimeDomain`, `RuntimeHudOverlay`.
- Functions, locals, and fields use `snake_case`: `run_frame`, `pick_physical_device`, `pending_render_deltas`, `fine_chunk_boundary_mask`.
- Constants use `SCREAMING_SNAKE_CASE`: `STAGE_ORDER`, `CHUNK_VOXEL_COUNT`, `MAX_RENDER_CHUNKS`, `MAX_RETRIES`.
- Integration-test files are phase-prefixed (`phase1_*`, `phase2_*`, `phase25_*`, `phase3_*`) and individual test names remain descriptive snake_case sentences.
- Serialized runtime enums use explicit serde tagging plus `rename_all = "snake_case"` to keep externalized event shapes predictable.

## API visibility and ownership conventions
- Public surface area is exported from `src/lib.rs`, then narrowed through `pub use` re-exports inside modules such as `src/runtime/mod.rs` and `src/meshing/mod.rs`.
- Renderer construction and global access happen through the explicit pair `install_renderer(...)` and `renderer_state()`.
- Helper functions that do not need to leave a module stay private or `pub(crate)`, especially low-level Vulkan allocation helpers.
- Several renderer submodules remain public because compile-check and integration tests intentionally reference concrete types such as `DeviceContext`, `SwapchainContext`, and `FrameData`.

## Error handling conventions
- App/bootstrap/Vulkan setup paths use `anyhow::Result` for ergonomic propagation (`src/app.rs`, `src/renderer/*.rs`).
- Runtime frame execution itself returns a concrete `FrameExecution` snapshot rather than `Result`; some stage boundaries intentionally absorb or ignore lower-level failures to keep the frame loop advancing.
- Chunk-lifecycle failures are modeled in domain types such as `ChunkState::Error { ... }` and `ChunkJobOutcome::Failed(String)`.
- `expect(...)` and `unwrap(...)` are mostly concentrated in tests and startup assumptions, not general application control flow.

## Data layout and serialization conventions
- GPU-facing structs use `#[repr(C)]` and `bytemuck::{Pod, Zeroable}` where binary layout matters (`GuiVertex`, `ChunkDrawMetadata`, packed mesh data types).
- Typed chunk payloads are explicit: `ChunkVoxels` validates exact payload length instead of passing raw byte blobs around the pipeline.
- Runtime commands, events, and sequence metadata derive `Serialize`/`Deserialize` so tests can lock wire-format behavior.

## Test conventions
- Integration tests live under `tests/` and are organized by phase/regression scope.
- Inline unit tests live next to implementation in `src/streaming/*.rs`, `src/runtime/scheduler.rs`, and `src/runtime/boundaries/*.rs`.
- Because runtime and renderer state use `OnceLock`, runtime-oriented integration tests reserve distinct frame-index ranges rather than assuming complete process reset between tests.

## Shader workflow conventions
- Shader sources live under `shaders/` as authoritative GLSL.
- `build.rs` is responsible for compiling shader sources to SPIR-V; new shader files should be added there and to `renderer::shader_source_files()`.
- Vulkan pipeline modules load compiled shader bytes from `OUT_DIR`, not from ad hoc runtime disk reads.
