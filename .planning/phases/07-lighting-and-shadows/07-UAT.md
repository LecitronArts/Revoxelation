---
status: testing
phase: 07-lighting-and-shadows
source: [07-01-SUMMARY.md, 07-02-SUMMARY.md, 07-03-SUMMARY.md, 07-04-SUMMARY.md, 07-05-SUMMARY.md]
started: "2026-04-03T00:00:00Z"
updated: "2026-04-03T00:00:00Z"
---

## Current Test

number: 2
name: Lighting egui Controls
expected: |
  An egui "Lighting" window should be visible with sliders for sun elevation, azimuth, intensity, and ambient intensity. Adjusting sun elevation should visibly rotate the light direction (e.g., low elevation = long shadows, overhead = top-lit). Adjusting ambient intensity should brighten/darken shadowed areas.
awaiting: user response

## Tests

### 1. PBR Directional Lighting
expected: Run the application. Scene should show blocks lit by a directional sun light with visible highlights and shading — not flat-colored. Blocks should have specular reflections where sun hits at glancing angle, and darker sides facing away. Overall 3D feel with depth.
result: pass

### 2. Lighting egui Controls
expected: An egui "Lighting" window should be visible with sliders for sun elevation, azimuth, intensity, and ambient intensity. Adjusting sun elevation should visibly rotate the light direction (e.g., low elevation = long shadows, overhead = top-lit). Adjusting ambient intensity should brighten/darken shadowed areas.
result: [pending]

### 3. Cascaded Shadow Maps
expected: Blocks should cast shadows onto other blocks and the ground. Shadows should have soft edges (not razor-sharp pixelated). Moving the camera should not cause shadow flickering or swimming. Shadows should be visible at different distances from the camera.
result: [pending]

### 4. Shadow egui Controls
expected: An egui "Shadows" window should be visible with an enable/disable toggle, lambda slider, bias sliders, and debug cascade colors toggle. Toggling shadows off should remove all block shadows. Enabling "debug cascade colors" should tint the scene in different colors showing which cascade covers each area.
result: [pending]

### 5. Voxel Ambient Occlusion
expected: Block junctions (where blocks meet at edges and corners) should appear darker than open faces. The darkening should be smooth and gradual — NOT a hard on/off shadow. Interior corners (like concave block arrangements) should be noticeably darker than exposed faces.
result: [pending]

### 6. Screen-Space Ambient Occlusion (SSAO)
expected: In addition to voxel AO, there should be a softer, broader darkening effect in concavities and near surfaces. The effect should be subtle. An egui SSAO control panel should allow toggling SSAO on/off — toggling off should make the scene look slightly flatter. Algorithm dropdown should offer GTAO, HBAO+, and Classic options.
result: [pending]

### 7. Procedural Sky
expected: Instead of a flat solid-color background, the sky should show a procedural gradient — blue at zenith, lighter toward horizon, with a visible sun disk. The sky should wrap around in all directions as the camera rotates.
result: [pending]

### 8. Day-Night Cycle
expected: In the egui "Atmosphere" panel, there should be a day-night cycle with a time slider and play/pause. Advancing time should transition: warm sunrise colors → bright day → warm sunset → dark blue night with stars and moonlight. The sun disk should move across the sky matching the time of day.
result: [pending]

### 9. Distance Fog
expected: Far-away blocks/terrain should gradually fade into the sky color. The fog color should match the current sky horizon color (e.g., warm orange during sunset, blue during day). An egui fog control panel should allow adjusting fog density and distance.
result: [pending]

### 10. Combined Lighting Quality
expected: All effects together — PBR lighting + shadows + voxel AO + SSAO + sky + fog — should create a cohesive scene with depth and atmosphere. No obvious visual artifacts (black patches, z-fighting in shadows, bright/dark seams between effects). The scene should look dramatically better than flat-colored blocks.
result: [pending]

## Summary

total: 10
passed: 1
issues: 0
pending: 9
skipped: 0

## Gaps

[none yet]
