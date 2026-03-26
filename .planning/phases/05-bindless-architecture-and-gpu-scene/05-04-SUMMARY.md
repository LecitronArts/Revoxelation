---
phase: 05-bindless-architecture-and-gpu-scene
plan: 04
subsystem: renderer
tags: [vulkan, bindless, material, texture-array, glsl, ssbo]

requires:
  - phase: 05-bindless-architecture-and-gpu-scene (plan 02)
    provides: BindlessTable with unified set 0, register_buffer/register_image API
provides:
  - BlockMaterial struct (8 bytes) with per-face texture indices
  - MaterialTable with 8 block types + air, SSBO upload at binding 8
  - TextureArray (16x16 RGBA8, 256 layers) with 10 procedural textures at binding 9
  - Fragment shader sampling texture array via nonuniformEXT
affects: [phase-06-meshlet-pipeline, phase-07-lighting-shadows]

tech-stack:
  added: [image (0.25, for future PNG loading)]
  patterns: [procedural-texture-generation, per-face-material-lookup, bindless-texture-sampling]

key-files:
  created:
    - src/renderer/material.rs
    - src/renderer/texture_array.rs
  modified:
    - src/renderer/mod.rs
    - src/renderer/bindless.rs
    - shaders/chunk_mesh.vert
    - shaders/chunk_mesh.frag
    - Cargo.toml
    - tests/phase5_bindless.rs

key-decisions:
  - "BlockMaterial: 4 x u16 (top/side/bottom texture + flags) = 8 bytes, #[repr(C)] with bytemuck"
  - "10 procedural textures generated in Rust (no PNG loading yet): dirt, grass_top, grass_side, stone, sand, log_bark, log_end, planks, leaves, water"
  - "Material SSBO at binding 8, texture array at binding 9 — matching BindlessTable reserved slots"
  - "Fragment shader uses face normal threshold (y > 0.5 / y < -0.5) for top/bottom/side selection"
  - "Vertex shader outputs v_block_id (flat uint), v_face_normal (vec3), v_uv (vec2) — replaces v_color"
  - "nonuniformEXT used on texture array index for descriptor indexing safety"

patterns-established:
  - "Per-face material lookup: face_normal selects top/side/bottom texture index from BlockMaterial"
  - "Procedural texture generation: hash-based noise for deterministic pixel patterns"

requirements-completed: [BIND-04]

duration: 16min
completed: 2026-03-26
---

# Phase 5 Plan 04: Block Materials and Texture Array Summary

**BlockMaterial system with per-face texture indices, procedural 2D texture array (10 layers), and bindless fragment shader sampling via SSBO binding 8 + texture array binding 9**

## Performance

- **Duration:** 16 min
- **Started:** 2026-03-26T04:33:32Z
- **Completed:** 2026-03-26T04:49:41Z
- **Tasks:** 4
- **Files modified:** 9

## Accomplishments
- BlockMaterial struct (8 bytes) with per-face top/side/bottom texture indices for 8 block types
- Procedural 2D texture array (16x16 RGBA8, 256 max layers, 10 initial layers) uploaded via staging
- Material SSBO registered at bindless binding 8; texture array at binding 9
- Fragment shader samples texture array using face-normal-derived material lookup with nonuniformEXT

## Task Commits

Each task was committed atomically:

1. **Task 1: Define BlockMaterial and MaterialTable** - `838d2e5` (feat) — TDD: 3 tests (size, 8 types, grass per-face)
2. **Task 2: Create procedural texture array and GPU upload** - `37f4c68` (feat)
3. **Task 3: Upload material SSBO and register bindless bindings** - `4a1e1a9` (feat)
4. **Task 4: Update vertex and fragment shaders for texture sampling** - `51e4e1c` (feat)

## Files Created/Modified
- `src/renderer/material.rs` - BlockMaterial struct, MaterialTable with 8 block types, SSBO upload
- `src/renderer/texture_array.rs` - TextureArray with VkImage 2D array, procedural generation, GPU upload
- `src/renderer/mod.rs` - Added material and texture_array modules, Renderer fields for cleanup
- `shaders/chunk_mesh.vert` - Outputs v_block_id, v_face_normal, v_uv; removed v_color hash coloring
- `shaders/chunk_mesh.frag` - Material SSBO lookup + texture array sampling with nonuniformEXT
- `Cargo.toml` - Added image crate dependency
- `tests/phase5_bindless.rs` - 3 new TDD tests for BlockMaterial

## Decisions Made
- BlockMaterial uses u16 fields (not u32) to keep struct at 8 bytes — sufficient for 65535 texture layers
- Procedural textures use hash-based noise (no external crate) for simple deterministic patterns
- Fragment shader face normal threshold at ±0.5 for top/bottom detection (robust for axis-aligned faces)
- UV coordinates passed from packed vertex data (bits 16-23 = u, 24-31 = v)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Phase 3 tests used removed metadata_shadow API**
- **Found during:** Task 4 (shader updates, full test suite run)
- **Issue:** Plan 05-03 renamed metadata_shadow to instance_shadow; phase3 tests still referenced old name
- **Fix:** Updated 2 references in tests/phase3_gap_closure.rs
- **Files modified:** tests/phase3_gap_closure.rs
- **Verification:** All 22 phase3 tests pass
- **Committed in:** 51e4e1c (part of Task 4 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor test maintenance. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- BIND-04 satisfied: block material system with distinct per-face textures
- Plan 05 (Phase 5 Plan 5) is next if it exists, or Phase 5 is complete
- Ready for visual verification: `cargo run` should show 8 block types with distinct textures
- TextureArray and MaterialTable infrastructure ready for Phase 7 PBR extension

---
*Phase: 05-bindless-architecture-and-gpu-scene*
*Completed: 2026-03-26*
