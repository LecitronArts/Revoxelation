---
phase: 03-greedy-meshing-and-render-delta-sync
verified: 2026-03-22
verifier: external-audit
status: CONDITIONAL_PASS
requirement_ids: [MESH-01, MESH-02, MESH-03]
plans_audited: [03-01, 03-02, 03-03, 03-04, 03-05, 03-06, 03-07]
test_result: 76/76 PASS
---

# Phase 3 Verification Report
## Greedy Meshing and Render Delta Sync

**Phase Goal:** Build the meshing pipeline, GPU chunk pool, and render delta system.
**Requirement IDs:** MESH-01, MESH-02, MESH-03
**Plans executed:** 03-01 through 03-07 (7 plans, 4 of which were gap-closure iterations)
**Audit date:** 2026-03-22

---

## 1. Requirement ID Cross-Reference

Every Phase 3 requirement ID from `REQUIREMENTS.md` is accounted for below.

| Req ID | REQUIREMENTS.md checkbox | Traceability table | Plans referencing it | All plan must_haves covered | Tests passing |
|--------|--------------------------|-------------------|---------------------|-----------------------------|---------------|
| MESH-01 | `[x]` | Complete | 03-01, 03-02, 03-03, 03-04, 03-05, 03-06, 03-07 | YES | YES |
| MESH-02 | `[x]` | Complete | 03-01 | YES | YES |
| MESH-03 | `[ ]` ⚠️ STALE | Pending ⚠️ STALE | 03-02, 03-03, 03-04 | YES | YES |

**Finding — MESH-03 checkbox is stale:**
`REQUIREMENTS.md` has `[ ] MESH-03` and the traceability table reads "Pending". This contradicts the implementation evidence: all MESH-03 tests pass (`mesh_03_chunk_pool_slot_reuse_clears_metadata`, `mesh_03_deactivated_active_chunk_enqueues_remove_delta`, `mesh_03_delta_sync_updates_only_dirty_slots`, `mesh_03_submit_frame_uses_dense_indirect_draw_count`, and eight additional selectors). The checkbox and traceability table were not updated after phase execution. **Action required: mark `[x] MESH-03` and update traceability to "Complete".**

---

## 2. Automated Test Suite

```
cargo test
```

**Result: 76/76 PASS, 0 FAIL, 0 IGNORED**

| Test file | Tests | Result |
|-----------|-------|--------|
| `tests/phase3_meshing.rs` | 13 | ALL PASS |
| `tests/phase3_gap_closure.rs` | 22 | ALL PASS |
| `tests/phase1_*` (5 files) | 21 | ALL PASS |
| `tests/phase2_streaming.rs` | 2 | ALL PASS |
| `tests/phase25_vulkan.rs` | 2 | ALL PASS |
| Doc-tests | 0 | ALL PASS |

No regressions across any prior phase test file.

---

## 3. Must-Have Verification by Plan

### Plan 03-01 — Greedy Meshing Contracts (MESH-01, MESH-02)

| Must-Have Truth | Artifact / Evidence | Status |
|-----------------|---------------------|--------|
| Loaded chunk data can be converted into greedy surface meshes without remeshing the whole active world | `src/meshing/greedy.rs:build_greedy_mesh` + `Stage::MeshSync` bounded dirty batch in `scheduler.rs` | VERIFIED |
| Chunk-border changes trigger affected neighboring chunks to remesh so visible seams do not appear | `src/meshing/invalidation.rs:mark_face_neighbors_dirty` | VERIFIED |
| Coarse chunks add and remove skirts on the exact faces that border active finer LOD0 neighbors | `src/meshing/invalidation.rs:update_finer_neighbor_face_mask` + `greedy.rs:finer_neighbor_face_mask` | VERIFIED |

| Artifact | Expected | Present | Contains |
|----------|----------|---------|---------|
| `src/streaming/types.rs` | `pub struct ChunkVoxels` | ✅ line 34 | `pub struct ChunkVoxels` |
| `src/meshing/greedy.rs` | exports `build_greedy_mesh` | ✅ line 14 | `pub fn build_greedy_mesh` |
| `src/meshing/invalidation.rs` | exports `MeshDirtyCause`, `MeshingState` | ✅ lines 26, 41 | both present |
| `tests/phase3_meshing.rs` | `mesh_02_border_invalidation_marks_neighbors` | ✅ | test passes |

Key links confirmed:
- `ChunkJobOutcome::Generated(ChunkVoxels)` — `src/streaming/types.rs:185` ✅
- `MeshingState` / `mark_.*dirty` pattern in `scheduler.rs` ✅
- `PackedMesh|pack_vertex` in `src/meshing/packing.rs` ✅

---

### Plan 03-02 — GPU Chunk Pool and Render Delta Sync (MESH-01, MESH-03)

| Must-Have Truth | Artifact / Evidence | Status |
|-----------------|---------------------|--------|
| Visible chunks render in the window through shared GPU buffers and a single indirect draw path | `submit_frame_sequence` locked to `chunk_delta_uploads → compute_cull → indirect_barrier → render_pass → draw_indexed_indirect → egui`; no per-draw fallback path | VERIFIED |
| Streaming out or remeshing one chunk updates only that chunk's GPU slot instead of rebuilding the whole world | Slot-byte-range uploads; `record_chunk_delta_uploads` touches only changed ranges | VERIFIED |
| Chunks that leave the active set stop rendering because deactivation produces explicit remove deltas | `RenderDelta::Remove` pushed in `handle_job_result` on `Unloaded`; slot cleared in `prepare_remove` | VERIFIED |
| Unsupported Vulkan devices fail before startup rather than silently switching to a per-draw fallback | `device.rs:95-127` requires `sampler_anisotropy`, `multi_draw_indirect`, `draw_indirect_first_instance`; hard error returned | VERIFIED |

| Artifact | Expected | Present | Contains |
|----------|----------|---------|---------|
| `src/renderer/chunk_pool.rs` | `ChunkPool`, `SlotUpload`, `ChunkDrawMetadata` | ✅ lines 20, 33, 63, 205 | all present |
| `src/renderer/mesh_pipeline.rs` | `ChunkMeshPipeline` | ✅ line 6 | present |
| `src/renderer/cull_pipeline.rs` | `ChunkCullPipeline`, `dispatch` | ✅ lines 6, 168 | both present |
| `src/app.rs` | `run(` | ✅ line 15 | `pub fn run() -> Result<()>` |
| `tests/phase3_meshing.rs` | `mesh_03_delta_sync_updates_only_dirty_slots` | ✅ | test passes |

Key links confirmed:
- `RenderDelta|enqueue_chunk_delta` in `scheduler.rs → chunk_pool.rs` ✅
- `SHADER_WRITE|INDIRECT_COMMAND_READ` barrier pattern in `renderer/mod.rs` ✅
- `install_renderer|Renderer::new` in `app.rs` ✅

---

### Plan 03-03 — Non-Empty Payloads and Dense Draw Bookkeeping (MESH-01, MESH-03)

| Must-Have Truth | Artifact / Evidence | Status |
|-----------------|---------------------|--------|
| Streamed chunks produce deterministic non-empty voxel payloads | `job_runner.rs` generates floor-and-pillar pattern keyed from `ChunkKey` | VERIFIED |
| Stable storage slots can contain holes while renderer has dense draw order | `slot_to_draw_index` + `draw_index_to_slot` + swap-remove on deactivation | VERIFIED |
| Graphics path places chunk geometry in world space from per-chunk metadata | `ChunkDrawMetadata.chunk_origin` + `gl_InstanceIndex` in vertex shader | VERIFIED |

| Artifact | Expected | Present |
|----------|----------|---------|
| `src/streaming/job_runner.rs` | `ChunkJobOutcome::Generated` | ✅ |
| `src/renderer/chunk_pool.rs` | `SlotAllocator` with `slot_to_draw_index`, `draw_index_to_slot` | ✅ lines 67-68 |
| `shaders/chunk_mesh.vert` | `gl_InstanceIndex` | ✅ line 36 |
| `tests/phase3_gap_closure.rs` | `mesh_03_dense_draw_list_swap_removes_sparse_slot_holes` | ✅ test passes |

---

### Plan 03-04 — Dense Indirect Compute Wiring (MESH-01, MESH-03)

| Must-Have Truth | Artifact / Evidence | Status |
|-----------------|---------------------|--------|
| Compute consumes chunk metadata and dense draw-slot data, not a no-op shader | `shaders/chunk_cull.comp` reads metadata/templates/draw-slots, copies stable templates into dense indirect list | VERIFIED |
| Indirect draw stays correct when stable slots are sparse | Draw submits `active_draw_count()` from dense indirect buffer, not sparse slot count | VERIFIED |
| Only changed slots and draw-list entries are updated per remesh/unload | `prepare_upload`/`prepare_remove` touch only affected stable-slot ranges and the single swapped dense draw entry | VERIFIED |

| Artifact | Expected | Present |
|----------|----------|---------|
| `src/renderer/chunk_pool.rs` | GPU dense draw-slot and dense indirect buffers | ✅ line 216, 266 |
| `src/renderer/cull_pipeline.rs` | descriptor-backed compute with 4 buffer bindings | ✅ |
| `shaders/chunk_cull.comp` | reads `draw_slots[]`, writes `dense_indirect[]` | ✅ |
| `tests/phase3_gap_closure.rs` | `mesh_03_submit_frame_uses_dense_indirect_draw_count` | ✅ test passes |

---

### Plan 03-05 — Optional Validation Layer Bootstrap (MESH-01)

| Must-Have Truth | Artifact / Evidence | Status |
|-----------------|---------------------|--------|
| `cargo run` reaches bootstrap setup even when `VK_LAYER_KHRONOS_validation` absent | `create_instance` enumerates layers, emits fallback warning, continues | VERIFIED |
| Debug builds enable validation diagnostics when available | `InstanceBootstrap.debug` gates loader/messenger creation | VERIFIED |
| Real `app::run → Renderer::new` path unchanged | `mod.rs` consumes `InstanceBootstrap`, no alternate bootstrap added | VERIFIED |

| Artifact | Expected | Present |
|----------|----------|---------|
| `src/renderer/instance.rs` | `InstanceBootstrap`, `InstanceDebugConfig`, `create_instance`, `resolve_debug_instance_config`, `VALIDATION_LAYER_NAME`, `DEBUG_UTILS_EXTENSION_NAME` | ✅ lines 8-60 |
| `src/renderer/mod.rs` | gates debug utils from `InstanceBootstrap.debug`, not unconditional `Some(...)` | ✅ |
| `tests/phase3_gap_closure.rs` | `mesh_01_missing_validation_layer_disables_optional_debug_bootstrap` | ✅ test passes |

---

### Plan 03-06 — Alignment-Safe SPIR-V Decoder (MESH-01)

| Must-Have Truth | Artifact / Evidence | Status |
|-----------------|---------------------|--------|
| Startup no longer panics on SPIR-V byte alignment | `decode_spirv_words` replaces `bytemuck::cast_slice(bytes)` in both pipelines | VERIFIED |
| Both graphics and compute use owned little-endian `u32` words | `mesh_pipeline.rs:251` and `cull_pipeline.rs:213` both call `decode_spirv_words(bytes)?` | VERIFIED |
| `bytemuck::cast_slice` is absent from shader-module creation paths | `grep bytemuck::cast_slice mesh_pipeline.rs cull_pipeline.rs` → no output | VERIFIED |

| Artifact | Expected | Present |
|----------|----------|---------|
| `src/renderer/spirv.rs` | `pub fn decode_spirv_words(bytes: &[u8]) -> Result<Vec<u32>>` | ✅ line 3 |
| `src/renderer/mesh_pipeline.rs` | `use super::spirv::decode_spirv_words` + no `bytemuck::cast_slice` | ✅ line 4 |
| `src/renderer/cull_pipeline.rs` | same pattern | ✅ line 4 |
| `tests/phase3_gap_closure.rs` | `mesh_01_spirv_word_decoder_accepts_unaligned_byte_input` | ✅ test passes |

---

### Plan 03-07 — Non-Degenerate Quads and 7-Bit Encoding (MESH-01)

| Must-Have Truth | Artifact / Evidence | Status |
|-----------------|---------------------|--------|
| `pack_quad()` encodes four distinct world-space corner positions | `plane_axes()` helper + corner expansion loop in `packing.rs:38-47`; `mesh_01_pack_quad_produces_non_degenerate_quads` passes | VERIFIED |
| Vertex shader decodes 7-bit coordinates and adds +1 face-normal offset for positive faces | `decode_position` uses `0x7Fu`, `>> 7`, `>> 14`; `face_offset` logic in `main()` | VERIFIED |
| `cargo run` renders visible chunk surfaces | Root cause (degenerate triangles) confirmed eliminated; process stays alive past shader startup; **visual surface confirmation is PENDING human UAT** | PARTIAL ⚠️ |

| Artifact | Expected | Present |
|----------|----------|---------|
| `src/meshing/packing.rs` | 7-bit shifts (`<<7`, `<<14`, `<<21`), `plane_axes()`, expanded corners | ✅ lines 17-46 |
| `shaders/chunk_mesh.vert` | `0x7Fu`, `>> 7`, `>> 14`, `face_offset` | ✅ lines 24-53 |
| `src/renderer/mesh_pipeline.rs` | `FrontFace::CLOCKWISE`, no `COUNTER_CLOCKWISE` | ✅ line 138 |
| `tests/phase3_gap_closure.rs` | `mesh_01_pack_quad_produces_non_degenerate_quads` | ✅ test passes |
| `tests/phase3_meshing.rs` | bit-pattern assertion updated to 7-bit layout | ✅ |

---

## 4. Discrepancies

| # | Location | Discrepancy | Severity | Action Required |
|---|----------|-------------|----------|-----------------|
| D-1 | `REQUIREMENTS.md` line 24 | `MESH-03` checkbox is `[ ]` (unchecked); traceability table says "Pending" — contradicts passing tests and phase summaries | STALE ARTIFACT | Check `[x] MESH-03`; update traceability row to "Complete" |
| D-2 | `03-VERIFICATION.md` | Was written after 03-06, before 03-07; does not reflect the degenerate-triangle fix or the 03-07 test additions | STALE ARTIFACT | This document supersedes it |
| D-3 | `03-HUMAN-UAT.md` / `03-07-SUMMARY.md` | Visual confirmation that colored chunk surfaces render in the window has NOT been obtained — `cargo run` process was SIGTERM'd after 15 s with no visual observation recorded | OPEN ITEM | Human must run `cargo run` and confirm chunk surfaces are visible |
| D-4 | `STATE.md` | `completed_phases: 4` — Phase 3 is still the current active phase and not formally closed | STALE ARTIFACT | Update to 3 completed phases; close Phase 3 only after human UAT passes |

---

## 5. Requirement Status Summary

| Requirement | Description | Automated Tests | Runtime | Status |
|-------------|-------------|-----------------|---------|--------|
| MESH-01 | Engine generates greedy meshes for visible chunk surfaces and updates incrementally | 22 selectors pass (gap closure) + 7 meshing selectors | Degenerate triangle root cause eliminated; visual output PENDING | **PARTIAL** — automated PASS, human UAT PENDING |
| MESH-02 | Chunk-border updates invalidate neighbor meshes to avoid seams | 4 selectors pass (`mesh_02_*`) | N/A (logic only) | **PASS** |
| MESH-03 | Renderer supports chunk-delta updates (no full-world reupload) | 10 selectors pass (`mesh_03_*`) | Slot-level upload path confirmed by source inspection | **PASS** (REQUIREMENTS.md checkbox stale — see D-1) |

---

## 6. Overall Verdict

**CONDITIONAL PASS**

All 76 automated tests pass. All plan must-haves have corresponding code and passing test coverage. Three requirement IDs are accounted for and linked to concrete artifacts.

Phase 3 is blocked from a **FULL PASS** by one open item:

> **D-3 — Human must visually confirm that colored chunk surfaces render in the Revoxelation window.**

The degenerate-triangle root cause was diagnosed (`pack_quad` encoded identical origin for all 4 vertices) and fixed in 03-07 (corner expansion via `plane_axes`, 7-bit encoding, CLOCKWISE front face). The `cargo run` process now survives shader-module creation and renderer bootstrap. Visual confirmation was not captured in any session log.

**Required actions before phase closure:**

1. Run `cargo run`, observe the window for at least 10 seconds, and confirm colored chunk surfaces are visible.
2. Update `REQUIREMENTS.md`: mark `[x] MESH-03` and update its traceability row to "Complete".
3. Update `STATE.md` to reflect Phase 3 as complete once UAT passes.

---

*Phase: 03-greedy-meshing-and-render-delta-sync*
*Verified: 2026-03-22*
*Verifier: external audit*
