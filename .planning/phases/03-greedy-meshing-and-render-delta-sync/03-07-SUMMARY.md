---
phase: 03-greedy-meshing-and-render-delta-sync
plan: 07
subsystem: meshing
tags: [packing, glsl, vertex-shader, vulkan, greedy-mesh, bit-packing, tdd]

requires:
  - phase: 03-06
    provides: alignment-safe SPIR-V decoder and shader module creation path

provides:
  - Non-degenerate quad packing: each vertex has a distinct world-space position
  - 7-bit coordinate encoding in pack_vertex (x|y|z|face in word0)
  - plane_axes() helper in packing.rs for corner expansion
  - Vertex shader decoding 7-bit coords and applying positive-face +1 offset
  - CLOCKWISE front face in mesh pipeline to compensate for Y-flipped debug projection
  - 4 new regression tests locking the gap-closure contracts

affects:
  - Phase 4 Movement (vertex positions are now correct world-space geometry)
  - Phase 3 human UAT re-verification

tech-stack:
  added: []
  patterns:
    - "7-bit packed vertex layout: word0 = x(7)|y(7)|z(7)|face(3)|skirt(1)"
    - "Face-offset expansion in vertex shader (not in CPU packing) per D-04"
    - "CLOCKWISE front face compensates for Y-flip in debug_project"

key-files:
  created: []
  modified:
    - src/meshing/packing.rs
    - shaders/chunk_mesh.vert
    - src/renderer/mesh_pipeline.rs
    - tests/phase3_gap_closure.rs
    - tests/phase3_meshing.rs

key-decisions:
  - "7-bit coordinate encoding chosen over 6-bit to accommodate corner values up to 64 (origin 0 + full-chunk size 64)"
  - "Positive-face +1 offset applied in vertex shader (D-04), not in pack_quad, keeping packed positions as geometric corners 0..64"
  - "CLOCKWISE front face compensates for debug_project Y-flip which reverses winding in window space (D-05)"

patterns-established:
  - "TDD red-green for source-contract tests: read shader/source file, assert string patterns"
  - "plane_axes() maps axis → (u_axis, v_axis) pair for quad corner expansion — same pattern in both packing.rs and greedy.rs"

requirements-completed:
  - MESH-01

duration: 8min
completed: 2026-03-22
---

# Phase 3 Plan 07: Gap Closure — Degenerate Quads, 7-bit Encoding, CLOCKWISE Front Face Summary

**7-bit pack_vertex encoding, plane_axes corner expansion in pack_quad, matching vertex shader decode, and CLOCKWISE pipeline front face — eliminating the degenerate zero-area triangle root cause**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-22T04:45:40Z
- **Completed:** 2026-03-22T04:53:58Z
- **Tasks:** 4 (3 TDD + 1 verification)
- **Files modified:** 5

## Accomplishments

- Widened pack_vertex from 6-bit to 7-bit coordinate encoding; added plane_axes() helper; pack_quad now computes actual distinct corner positions instead of encoding the origin for all 4 vertices
- Updated vertex shader decode_position to use 0x7Fu masks / >>7 / >>14 shifts and added face-offset expansion (face_index bits 21-23 → +1 along face-normal for positive faces)
- Changed pipeline front_face from COUNTER_CLOCKWISE to CLOCKWISE to compensate for debug_project Y-flip
- Full `cargo test` suite: 76+ tests across all suites, 0 failures

## Task Commits

Each task was committed atomically:

1. **Task 1: Widen pack_vertex to 7-bit coords and fix pack_quad corner expansion** - `8ef6cf0` (feat)
2. **Task 2: Update vertex shader decode + face-offset expansion** - `2cfdbe4` (feat)
3. **Task 3: Change pipeline front face to CLOCKWISE** - `005817d` (feat)
4. **Task 4: Full test suite and runtime verification** — included in plan metadata commit

## Files Created/Modified

- `src/meshing/packing.rs` — pack_vertex uses <<7/<<14/<<21 shifts; pack_quad computes corner positions via plane_axes; new plane_axes() private helper
- `shaders/chunk_mesh.vert` — decode_position uses 0x7Fu/>>7/>>14; added face_index extraction and face_offset expansion in main()
- `src/renderer/mesh_pipeline.rs` — front_face changed from COUNTER_CLOCKWISE to CLOCKWISE
- `tests/phase3_gap_closure.rs` — 4 new tests: mesh_01_pack_vertex_uses_7_bit_coordinate_encoding, mesh_01_pack_quad_produces_non_degenerate_quads, mesh_01_greedy_mesh_single_block_has_nonzero_position_spread, mesh_01_vertex_shader_decodes_7_bit_positions_and_expands_face_offset, mesh_01_mesh_pipeline_uses_clockwise_front_face_for_y_flip
- `tests/phase3_meshing.rs` — updated mesh_01_chunk_voxels_contract_and_packed_layout bit-pattern assertion to match 7-bit layout

## Decisions Made

- Used 7-bit coordinate encoding (D-03): corner values reach 64 (origin 0 + size 64), which requires 7 bits; new word0 = x(7)|y(7)|z(7)|face(3)|skirt(1)
- Positive-face offset applied in shader, not pack_quad (D-04): packed positions remain geometric corners 0..64; face index already in word0
- CLOCKWISE front face (D-05): debug_project negates clip.y, which reverses winding in window space; CLOCKWISE aligns the GPU's front-face test with outward-facing geometry

## Deviations from Plan

None — plan executed exactly as written. All three TDD cycles (RED confirmed failure, GREEN passed, no refactor step needed) followed the PLAN.md action steps precisely.

## Issues Encountered

- `cargo run` exited with code 143 (SIGTERM after 15-second timeout). This is expected behavior — the window process was terminated externally by the timeout wrapper, not a crash. The engine launched, printed the validation-layer unavailability message (known from prior plans), and was killed at the end of the observation window. No new blocker observed in stderr.

## User Setup Required

None — no external service configuration required.

## Next Phase Readiness

- All 3 gap-closure root causes are eliminated: pack_quad produces non-degenerate quads, the vertex shader decodes the correct positions and applies face offsets, and the pipeline front face matches the Y-flipped projection
- Phase 3 is ready for human UAT re-verification (visual confirmation that colored chunk surfaces render in the Revoxelation window)
- Phase 3 human UAT re-verification is the next step before transitioning to Phase 4

---
*Phase: 03-greedy-meshing-and-render-delta-sync*
*Completed: 2026-03-22*
