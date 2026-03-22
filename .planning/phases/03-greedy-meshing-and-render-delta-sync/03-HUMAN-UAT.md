---
status: diagnosed
phase: 03-greedy-meshing-and-render-delta-sync
source: [03-VERIFICATION.md]
started: 2026-03-22T10:47:33+08:00
updated: 2026-03-22T12:10:00+08:00
---

## Current Test

[testing complete]

## Tests

### 1. Visible Window Path
expected: `cargo run` opens a `Revoxelation` window and displays chunk surfaces through the greedy-mesh renderer path.
result: issue
reported: "窗口打开了，标题是 Revoxelation，但只显示灰紫色清屏背景，没有渲染任何 chunk 表面。控制台显示 VK_LAYER_KHRONOS_validation not available; continuing without validation layer."
severity: major

### 2. Border Seam Check
expected: Adjacent chunks and LOD boundaries show no visible holes, and skirts only appear on the expected coarse faces.
result: skipped
reason: 测试 1 无渲染输出，无法验证

### 3. Delta-Only Update Check
expected: Localized remesh/unload activity changes only the affected chunk without a visible full-world rebuild.
result: skipped
reason: 测试 1 无渲染输出，无法验证

## Summary

total: 3
passed: 0
issues: 1
pending: 0
skipped: 2

## Gaps

- truth: "cargo run opens a Revoxelation window and displays chunk surfaces through the greedy-mesh renderer path"
  status: failed
  reason: "User reported: 窗口打开了但只显示灰紫色清屏背景，没有渲染任何chunk表面"
  severity: major
  test: 1
  root_cause: "pack_quad() encodes all 4 vertices with identical position (quad.origin) in word0; vertex shader only reads word0 for position and never expands corners using UV offsets from word1 — all triangles are degenerate (zero area)"
  artifacts:
    - path: "src/meshing/packing.rs"
      issue: "pack_quad() passes quad.origin as local_xyz for all 4 corners; varying offsets [0,0],[w,0],[w,h],[0,h] only stored in word1 UV bits"
    - path: "shaders/chunk_mesh.vert"
      issue: "decode_position(in_packed.x) reads identical x,y,z from word0 for all 4 vertices; never reads face index (bits 18-20) or UV offsets (word1 bits 16-31) to expand quad corners"
  missing:
    - "Vertex shader must decode face index from word0 bits 18-20 and UV offsets from word1 bits 16-31, map (face, u, v) to world-space corner displacement"
    - "Alternatively: pack_quad() could compute actual corner positions and encode each corner's true [x,y,z] into word0"
  debug_session: ""
