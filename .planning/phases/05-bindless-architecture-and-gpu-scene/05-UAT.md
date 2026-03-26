---
status: complete
phase: 05-bindless-architecture-and-gpu-scene
source: [05-01-SUMMARY.md, 05-02-SUMMARY.md, 05-03-SUMMARY.md, 05-04-SUMMARY.md, 05-05-SUMMARY.md]
started: 2026-03-26T12:30:00Z
updated: 2026-03-26T12:30:00Z
---

## Current Test

number: 7
name: No Rendering Artifacts (Major)
expected: |
  No completely black faces, no flickering triangles, no missing chunk faces
  (other than known Z-fighting on chunk boundaries which is deferred).
  Chunks form recognizable voxel structures with floors and pillars.
awaiting: user response

## Tests

### 1. Application Starts with Vulkan 1.2
expected: Run `cargo run`. The application window opens without crashes or Vulkan errors. Console should NOT show "Vulkan 1.2 feature(s) missing" errors. Window title shows "Revoxelation".
result: PASS

### 2. Chunks Render with Distinct Block Textures
expected: Voxel chunks are visible in the scene. Blocks should display textured surfaces (dirt brown, stone gray, sand yellow, etc.) — NOT solid flat colors or all-black faces.
result: PASS

### 3. Grass Blocks Show Per-Face Textures
expected: Grass blocks have a green top face, brown/earthy side faces, and a darker bottom face. The three faces should be visually distinct from each other.
result: PASS (fixed — viewport Y-flip + side texture UV mapping)

### 4. Multiple Block Types Distinguishable
expected: At least 3-4 different block types are visible with clearly different textures (e.g., dirt, grass, stone, sand, wood). They should NOT all look the same.
result: PASS

### 5. HUD Displays Chunk Statistics
expected: A "Debug" window (egui overlay) is visible showing frame count and chunk statistics in the format "Chunks: X/Y | Slots: X/Y | Frame: Xms".
result: PASS

### 6. Camera Movement Works
expected: WASD keys move the camera forward/backward/left/right. Space goes up, Shift goes down. Mouse moves the view direction. Chunks remain correctly positioned during movement.
result: PASS (fixed — viewport Y-flip resolved Space/Shift swap)

### 7. No Rendering Artifacts (Major)
expected: No completely black faces, no flickering triangles, no missing chunk faces (other than known Z-fighting on chunk boundaries which is deferred). Chunks form recognizable voxel structures with floors and pillars.
result: PASS

## Summary

total: 7
passed: 7
issues: 0
pending: 0
skipped: 0

## Gaps

### GAP-01: Top/Bottom Face Texture Swap — RESOLVED
severity: major
test: 3
description: Grass blocks had top and bottom textures swapped.
root_cause: Missing Vulkan viewport Y-flip. Vulkan clip-space is Y-down but glam perspective_rh produces Y-up. Without negative-height viewport, face normals and camera Y direction were inverted.
fix: Negative-height viewport in mesh_pipeline.rs (y=height, height=-height). Also fixed X-face UV mapping (swap du/dv) and grass side texture V direction.

### GAP-02: Space/Shift Camera Controls Swapped — RESOLVED
severity: minor
test: 6
description: Space key moved camera down, Shift moved up. Should be opposite.
root_cause: Same as GAP-01 — missing viewport Y-flip inverted the visual Y direction.
fix: Same viewport Y-flip fix resolved this.
fix_plan: [pending]
