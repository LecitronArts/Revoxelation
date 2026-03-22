---
phase: 03-greedy-meshing-and-render-delta-sync
verified: 2026-03-22T10:47:33+08:00
status: human_needed
score: 3/3 must-haves verified
re_verification:
  previous_status: gaps_found
  previous_score: 2/3
  gaps_closed:
    - "Shared `decode_spirv_words` now replaces raw `bytemuck::cast_slice(bytes)` in both graphics and compute shader-module creation, removing the SPIR-V alignment panic from the live startup path."
    - "The live `cargo run` path now stays up after renderer bootstrap; stderr shows only the expected validation-layer fallback warning, not `TargetAlignmentGreaterAndInputNotAligned`."
  gaps_remaining: []
  regressions: []
human_verification:
  - "Confirm the `Revoxelation` window actually opens and displays chunk surfaces through the greedy-mesh renderer path."
  - "Visually confirm chunk borders do not show seams or unexpected skirts while chunks activate and unload."
  - "Trigger a localized remesh/unload and confirm the runtime behavior looks like delta-only updates rather than a full-world rebuild."
---

# Phase 3: Greedy Meshing and Render Delta Sync Verification Report

**Phase Goal:** Visible voxel surfaces are meshed efficiently and renderer sync updates only affected chunks instead of full-world uploads.
**Verified:** 2026-03-22T10:47:33+08:00
**Status:** human_needed
**Re-verification:** Yes - after executing `03-06`

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
| --- | --- | --- | --- |
| 1 | Visible chunk surfaces render using greedy meshing with incremental updates. | VERIFIED | `src/renderer/spirv.rs` now exposes `decode_spirv_words`, and both `src/renderer/mesh_pipeline.rs` and `src/renderer/cull_pipeline.rs` call it before `ShaderModuleCreateInfo::code(...)`. Fresh `cargo test --test phase3_gap_closure mesh_01_spirv_word_decoder_accepts_unaligned_byte_input -- --exact` passed, the full `cargo test --test phase3_gap_closure` suite passed, and a fresh `cargo run` stayed alive for 20 seconds after startup while emitting only the expected `VK_LAYER_KHRONOS_validation not available; continuing without validation layer.` warning. The prior `TargetAlignmentGreaterAndInputNotAligned` panic did not recur. |
| 2 | Border changes invalidate neighbor chunks correctly so seams are not visible at chunk edges. | VERIFIED | `src/meshing/invalidation.rs:15-167` still implements same-LOD face invalidation and finer-neighbor skirt masks, and fresh `cargo test --test phase3_meshing` coverage remains green through the full `cargo test` run. |
| 3 | Chunk edits and streaming updates apply through chunk-delta renderer uploads without full-world reupload. | VERIFIED | `src/runtime/scheduler.rs`, `src/renderer/chunk_pool.rs`, `src/renderer/cull_pipeline.rs`, and `src/renderer/mod.rs` continue to satisfy the dense-indirect and delta-sync contracts; fresh `cargo test --test phase3_gap_closure` and `cargo test` both passed with the Phase 3 gap-closure selectors included. |

**Score:** 3/3 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| --- | --- | --- | --- |
| `src/renderer/spirv.rs` | Shared alignment-safe SPIR-V byte-to-word decoding for Vulkan shader-module creation | VERIFIED | `decode_spirv_words(bytes)` rejects non-word-aligned lengths and converts any `&[u8]` into owned little-endian `u32` words without relying on slice alignment. |
| `src/renderer/mesh_pipeline.rs` | Graphics shader-module creation that accepts build.rs-produced SPIR-V bytes on the live runtime path | VERIFIED | `create_shader_module` now calls `decode_spirv_words(bytes)?` and passes the owned words into `ShaderModuleCreateInfo::code(&code)`. |
| `src/renderer/cull_pipeline.rs` | Compute shader-module creation that uses the same alignment-safe SPIR-V decoding contract | VERIFIED | The compute path now uses the shared decoder instead of `bytemuck::cast_slice(bytes)`, preventing the next startup step from failing on the same alignment assumption. |
| `src/app.rs` | Visible app bootstrap that installs the renderer and drives redraw | VERIFIED | The existing `app::run -> Renderer::new` startup path now survives both optional validation fallback and shader-module creation; the fresh `cargo run` session remained alive until manually terminated after 20 seconds. |
| `tests/phase3_gap_closure.rs` | Requirement coverage for optional debug bootstrap fallback plus the new SPIR-V alignment gap closure | VERIFIED | Fresh `cargo test --test phase3_gap_closure` passed the new selectors `mesh_01_spirv_word_decoder_accepts_unaligned_byte_input`, `mesh_01_spirv_word_decoder_rejects_non_word_aligned_length`, `mesh_01_pipeline_sources_stop_using_bytemuck_cast_slice_for_shader_modules`, `mesh_01_chunk_mesh_pipeline_uses_alignment_safe_spirv_decoder`, and `mesh_01_chunk_cull_pipeline_uses_alignment_safe_spirv_decoder`. |
| `tests/phase3_meshing.rs` | Requirement coverage for greedy meshing, invalidation, and delta sync | VERIFIED | Phase 3 meshing coverage remained green inside the fresh full `cargo test` run. |

### Key Link Verification

| From | To | Via | Status | Details |
| --- | --- | --- | --- | --- |
| `build.rs` | `src/renderer/spirv.rs` | Compiled shader artifacts remain raw `artifact.as_binary_u8()` byte blobs that runtime code must decode safely | WIRED | Runtime startup keeps the existing build output and decodes it into owned `u32` words only at shader-module creation time. |
| `src/renderer/spirv.rs` | `src/renderer/mesh_pipeline.rs` | `ChunkMeshPipeline::create_shader_module` decodes SPIR-V bytes through the shared helper | WIRED | Graphics startup now depends on `decode_spirv_words(bytes)?` rather than raw byte casting. |
| `src/renderer/spirv.rs` | `src/renderer/cull_pipeline.rs` | `ChunkCullPipeline::create_shader_module` uses the same helper so compute startup cannot fail on the next step | WIRED | Compute startup now shares the same decoding contract and error behavior. |
| `src/app.rs` | `src/renderer/mod.rs` | `app::run -> Renderer::new` live runtime bootstrap | WIRED | The fresh `cargo run` session reached a stable running state without the prior alignment panic. |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| --- | --- | --- | --- | --- |
| MESH-01 | 03-01, 03-02, 03-03, 03-04, 03-05, 03-06 | Engine can generate greedy meshes for visible chunk surfaces and update them incrementally. | SATISFIED | The live startup path no longer aborts in shader-module creation, the new SPIR-V regression tests pass, and `cargo run` remains alive after renderer bootstrap with the previous panic absent. |
| MESH-02 | 03-01 | Chunk-border updates correctly invalidate neighbor meshes to avoid visible seams. | SATISFIED | Border invalidation and skirt coverage remain green in Phase 3 meshing tests. |
| MESH-03 | 03-02, 03-03, 03-04 | Renderer integration supports chunk-delta updates so chunk edits do not require full world reupload. | SATISFIED | Dense draw bookkeeping, metadata-driven placement, compute wiring, and delta-only uploads remain green in the fresh test runs. |

### Anti-Patterns Found

None in the current Phase 3 scope. The previous raw `bytemuck::cast_slice(bytes)` shader-module startup assumption has been removed from both graphics and compute paths.

### Human Verification Required

Automated checks are green, but three user-visible behaviors still require a human confirmation pass and were persisted to `03-HUMAN-UAT.md`.

### 1. Visible Window Path

**Test:** Run `cargo run` on supported Vulkan hardware and visually inspect the window.  
**Expected:** A `Revoxelation` window opens and displays chunk surfaces through the greedy-mesh + dense-indirect path.  
**Why human:** The current automated evidence proves the process stays alive and the old panic is gone, but only a human can confirm the visible frame contents.

### 2. Border Seam Check

**Test:** Observe adjacent chunks and LOD boundaries while chunks activate and unload.  
**Expected:** No holes appear at chunk edges, and skirts appear/disappear only on the expected coarse faces.  
**Why human:** Seam correctness is ultimately a visual runtime behavior.

### 3. Delta-Only Update Check

**Test:** Trigger a single remesh and a single unload while observing the running renderer.  
**Expected:** Only the affected chunk changes; there is no visible full-world rebuild behavior.  
**Why human:** Confirms runtime behavior beyond compile-time and unit-test coverage.

### Summary

`03-06` closed the last known Phase 3 blocker. Shader-module startup no longer depends on aligned `&[u8]` input, the same alignment-safe decoder now protects both graphics and compute pipeline creation, and the live runtime path no longer crashes with `TargetAlignmentGreaterAndInputNotAligned`.

Phase 3 is therefore out of gap-closure mode and into human verification mode. The remaining work is not another implementation plan unless a human runtime check reports a new issue.

---

_Verified: 2026-03-22T10:47:33+08:00_  
_Verifier: Codex execute-phase orchestration + fresh command evidence_
