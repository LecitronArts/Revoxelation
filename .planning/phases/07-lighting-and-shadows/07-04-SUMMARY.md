---
phase: 07-lighting-and-shadows
plan: 04
subsystem: meshing, rendering
tags: [voxel-ao, ambient-occlusion, greedy-meshing, packed-vertex, glsl]

requires:
  - phase: 07-01
    provides: PBR lighting, ambient term in shaders
  - phase: 07-02
    provides: CSM shadow sampling, apply_directional_light_shadowed
provides:
  - Per-vertex voxel AO computed during greedy meshing (zero GPU cost)
  - AO values packed in word0 bits 24-25 of PackedVertex
  - decode_vertex_ao() in common.glsl
  - Smooth AO gradient via rasterizer interpolation
  - Quad diagonal flip for interpolation anisotropy fix
affects: [07-03-ssao]

tech-stack:
  added: []
  patterns:
    - "4-corner AO algorithm: side1/side2/diagonal neighbor check → AO 0-3"
    - "Quad diagonal flip: ao[0]+ao[2] < ao[1]+ao[3] → swap triangle split"
    - "AO modulates ambient term only, not direct lighting"

key-files:
  created:
    - tests/phase7_voxel_ao.rs
  modified:
    - src/meshing/greedy.rs
    - src/meshing/packing.rs
    - shaders/common.glsl
    - shaders/meshlet_draw.vert
    - shaders/meshlet_draw.frag
    - shaders/chunk_mesh.vert
    - shaders/chunk_mesh.frag
    - shaders/meshlet.mesh

key-decisions:
  - "LGHT-04-01: AO packed in word0 bits 24-25 (2 bits), repurposing former skirt bit (skirts removed in MSHL-05)"
  - "LGHT-04-02: AO curve non-linear [0.2, 0.5, 0.75, 1.0] for better visual contrast at dark end"
  - "LGHT-04-03: AO applied to ambient term only (subtract raw ambient, re-add with AO factor) — physically correct"
  - "LGHT-04-04: Quad diagonal flip when opposite corner AO sums differ — Minecraft-style interpolation fix"
  - "LGHT-04-05: is_opaque_for_ao uses sample_with_halo for cross-chunk boundary lookups — air=non-occluding"
  - "LGHT-04-06: Transparent block FLAG_TRANSPARENT check deferred — requires MaterialTable access in meshing (noted as TODO)"

requirements-completed: [LGHT-04]

duration: 45min
completed: 2026-03-29
---

# Phase 07 Plan 04: Voxel Ambient Occlusion Summary

**Classic 4-corner voxel AO computed during greedy meshing: per-vertex AO (0-3) packed into word0 bits 24-25, decoded in vertex shaders, smoothly interpolated by rasterizer, applied to ambient lighting term in fragment shaders with diagonal flip for anisotropy correction.**

## Performance

- **Duration:** 45 min
- **Started:** 2026-03-29T04:44:18Z
- **Completed:** 2026-03-29T05:29:31Z
- **Tasks:** 3
- **Files modified:** 12

## Accomplishments
- 4-corner voxel AO algorithm (side1/side2/diagonal check → AO 0-3) computed per-vertex during greedy meshing at zero GPU cost
- AO values packed into 2 free bits of PackedVertex word0 (bits 24-25), decoded by decode_vertex_ao() in all shader paths
- Quad diagonal flip prevents interpolation anisotropy artifacts when opposite corners have different AO
- AO modulates ambient lighting only (not direct sun), producing physically correct contact shadows
- Cross-chunk boundary AO uses existing sample_with_halo infrastructure — no new lookup code needed
- 7 unit tests verify AO correctness: open air, fully occluded, single neighbor, bit encoding, boundary, air non-occlusion

## Task Commits

Each task was committed atomically:

1. **Task 1: Voxel AO Calculation in Greedy Meshing** - `26bf780` (feat)
2. **Task 2: Vertex Shader AO Extraction + Fragment Shader Integration** - `2a64bdd` (feat)
3. **Task 3: AO Quality Fixes + Combined AO + Tests** - `9006f68` (test)

## Files Created/Modified
- `src/meshing/greedy.rs` — Added compute_corner_ao, compute_quad_ao, is_opaque_for_ao
- `src/meshing/packing.rs` — Extended pack_vertex with ao param, pack_quad with ao+diagonal flip
- `shaders/common.glsl` — Added decode_vertex_ao() function
- `shaders/meshlet_draw.vert` — v_voxel_ao output at location 6
- `shaders/meshlet_draw.frag` — AO applied to ambient term, TODO(07-03) marker for SSAO
- `shaders/chunk_mesh.vert` — v_voxel_ao output at location 6
- `shaders/chunk_mesh.frag` — AO applied to ambient term
- `shaders/meshlet.mesh` — v_voxel_ao[] output for mesh shader path
- `tests/phase7_voxel_ao.rs` — 7 unit tests for AO correctness
- `tests/phase3_meshing.rs` — Updated pack_vertex calls, fixed skirt_vertex_count
- `tests/phase3_gap_closure.rs` — Updated pack_vertex/pack_quad calls
- `tests/phase6_meshlet.rs` — Updated pack_vertex calls

## Decisions Made
- Repurposed word0 bit 24 (former skirt bit, disabled in MSHL-05) for AO; skirt code was already removed
- Non-linear AO curve [0.2, 0.5, 0.75, 1.0] for better visual contrast at dark end vs linear [0.25, 0.5, 0.75, 1.0]
- AO applied via ambient subtraction/re-addition pattern rather than post-multiply — cleaner separation from direct lighting
- Transparent block AO exclusion deferred (requires MaterialTable access in meshing context) — noted as TODO

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Rust parser ambiguity with `<` on `as u16` cast**
- **Found during:** Task 1 (pack_quad diagonal flip)
- **Issue:** `ao[0] as u16 + ao[2] as u16 < ao[1] as u16` parsed as generic args
- **Fix:** Used `u16::from()` with intermediate variables
- **Files modified:** src/meshing/packing.rs
- **Verification:** cargo build succeeds
- **Committed in:** 26bf780

**2. [Rule 1 - Bug] skirt_vertex_count test false-positive with AO bits**
- **Found during:** Task 1 (test updates)
- **Issue:** Old test checked `word0 & (1 << 24)` for skirt detection — now AO uses those bits
- **Fix:** Made skirt_vertex_count always return 0 (skirts were disabled in MSHL-05)
- **Files modified:** tests/phase3_meshing.rs
- **Verification:** All 13 phase3_meshing tests pass
- **Committed in:** 26bf780

**3. [Rule 1 - Bug] Test setup used nonexistent MeshDirtyCause::NewlyActivated variant**
- **Found during:** Task 3 (unit tests)
- **Issue:** Test used MeshDirtyCause::NewlyActivated which doesn't exist
- **Fix:** Changed to MeshDirtyCause::GeneratedPayload
- **Files modified:** tests/phase7_voxel_ao.rs
- **Verification:** All 7 AO tests pass
- **Committed in:** 9006f68

**4. [Rule 1 - Bug] AO test geometry misunderstanding — AO samples at face level not behind face**
- **Found during:** Task 3 (unit tests)
- **Issue:** Initial tests placed blocks behind the face (same level as main block), but AO samples in the air space at face level
- **Fix:** Redesigned test geometry to place neighbor blocks at face_pos level (y+1 for +Y faces)
- **Files modified:** tests/phase7_voxel_ao.rs
- **Verification:** test_corner_ao_fully_occluded and test_single_neighbor_ao pass
- **Committed in:** 9006f68

---

**Total deviations:** 4 auto-fixed (4 bugs)
**Impact on plan:** All fixes necessary for correctness. No scope creep.

## Issues Encountered
None — all issues resolved via deviation rules.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Voxel AO complete — ready for 07-03 SSAO integration (TODO marker in shaders)
- v_voxel_ao at location 6 in all shader paths (meshlet_draw, chunk_mesh, meshlet.mesh)
- decode_vertex_ao() in common.glsl available for any shader that needs AO

---
*Phase: 07-lighting-and-shadows*
*Completed: 2026-03-29*
