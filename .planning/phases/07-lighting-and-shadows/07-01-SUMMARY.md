---
phase: 07-lighting-and-shadows
plan: 01
status: complete
started: "2026-03-29"
completed: "2026-03-29"
commits:
  - d85c9ec: "feat(07-01): extend BindlessTable to 25 bindings, expand BlockMaterial to 32B PBR, add LightingState + PointLightManager"
  - 4ebe510: "feat(07-01): add Cook-Torrance PBR BRDF to shaders with directional + point light evaluation"
  - 8099165: "feat(07-01): create PBR texture arrays, wire LightingState/PointLightManager init + upload, add egui lighting controls"
  - ed73587: "feat(07-01): update BlockMaterial size test for 32B PBR expansion, finalize point light system"
---

# Plan 07-01 Summary: Directional Light + PBR Lighting Model

## Objective
Establish PBR lighting foundation: extend BindlessTable to 25 bindings, expand BlockMaterial for PBR textures, create MR/normal/emissive texture arrays, add Cook-Torrance BRDF to fragment shaders, implement directional light with configurable sun direction, and build a point light system for emissive blocks.

## What Changed

### Task 1: Extend BindlessTable + BlockMaterial + LightingState
- **bindless.rs**: BINDING_COUNT 16 -> 25 with 9 new named constants (bindings 16-24). Pool sizes updated: STORAGE_BUFFER 14->17, COMBINED_IMAGE_SAMPLER 2->7, added STORAGE_IMAGE 1.
- **material.rs**: BlockMaterial expanded from 8B (4 x u16) to 32B (16 x u16) with PBR fields: MR/normal/emissive texture indices per face, emissive_intensity, flags (FLAG_HAS_MR, FLAG_HAS_NORMAL, FLAG_HAS_EMISSIVE_MAP, FLAG_IS_32X32). Compile-time 32B assertion.
- **lighting.rs**: New module. LightingParams #[repr(C)] struct (sun, ambient, CSM placeholders, fog placeholders). LightingState with double-buffered SSBOs at binding 18, sun elevation/azimuth angles, per-frame update.
- **point_light.rs**: New module. PointLight (position, radius, color, intensity = 32B). PointLightManager with double-buffered SSBOs at binding 22, MAX_VISIBLE_POINT_LIGHTS = 64.
- **mod.rs**: Added pub mod lighting/point_light, fields on Renderer, Drop cleanup order.

### Task 2: PBR BRDF in Shaders
- **common.glsl**: Added PI, LightingParams struct, PointLight struct, GGX NDF, Smith geometry, Schlick Fresnel, cook_torrance_brdf(), apply_directional_light(), evaluate_point_light(), bayer_dither().
- **meshlet_draw.frag**: Full PBR rewrite - reads 32B BlockMaterial, bindings 18/22, Cook-Torrance BRDF with directional + point lights, emissive self-glow.
- **chunk_mesh.frag**: Same PBR lighting as meshlet path (no LOD dither).
- **meshlet_draw.vert / chunk_mesh.vert / meshlet.mesh**: Added v_world_pos output for fragment V vector.
- **mesh_pipeline.rs**: Push constant stage flags updated to VERTEX|FRAGMENT (all 3 pipeline paths).

### Task 3: PBR Texture Arrays + Lighting Upload + egui Controls
- **texture_array.rs**: Factory functions new_mr_array_16(), new_normal_array_16(), new_emissive_array_16() with create_pbr_texture_array() shared helper. 16x16 RGBA8, 256 layers, mipmaps, aniso, registered at bindings 19/20/21.
- **app.rs**: Initialize LightingState, PointLightManager, and PBR texture arrays during startup. Added egui "Lighting" window with sun elevation/azimuth/intensity, ambient intensity, and time of day sliders.
- **submit.rs**: Per-frame LightingState.update() and PointLightManager.upload() calls before draw commands.
- **mod.rs**: Added mr/normal/emissive_texture_array fields, Drop cleanup.

### Task 4: Point Light System Finalization
- **tests/phase5_bindless.rs**: Updated phase5_block_material_size assertion from 8B to 32B.
- PointLightManager fully wired (created in Tasks 1+3), upload per-frame in submit.rs.

## Decisions
- 16x16 PBR arrays only (no 32x32 initially) - no blocks currently use FLAG_IS_32X32
- Default PBR: metallic=0.0, roughness=0.8 (dielectric, slightly rough)
- Sun default: 45 deg elevation, 135 deg azimuth, intensity 2.0, warm white
- Ambient default: intensity 0.3, cool blue-tinted
- Point light SSBO empty initially (no emissive blocks in default material table)
- gather_emissive_lights deferred - no emissive blocks exist yet in the default block set

## Verification
- cargo build: OK (13 pre-existing Rust 2024 unsafe warnings only)
- cargo test: 23 passed, 0 failed
- cargo clippy: No new warnings (pre-existing only)
- All shaders compile via build.rs (shaderc)
