---
phase: 07-lighting-and-shadows
plan: 02
status: complete
started: "2026-03-29T04:00:07Z"
completed: "2026-03-29T05:30:00Z"
---

# Plan 07-02 Summary: Cascaded Shadow Maps (CSM)

## What Was Done

### Task 1: CascadedShadowMap GPU Resources + Depth-Only Pipeline
- Created `src/renderer/shadow.rs` with `CascadedShadowMap` struct:
  - Single 2D array image (4 layers, D32_SFLOAT, 2048x2048 default)
  - Per-layer image views for framebuffer attachments
  - 2D_ARRAY view for shader sampling at binding 16
  - Comparison sampler (LESS_OR_EQUAL, LINEAR filtering, CLAMP_TO_BORDER)
  - Depth-only render pass (CLEAR + STORE)
  - 4 framebuffers (one per cascade)
  - Depth-only graphics pipeline with depth bias (constant=1.25, slope=1.75)
- Created `shaders/shadow_depth.vert` — depth-only vertex shader using light_view_proj push constant
- Updated `build.rs` (array size 11→12) and `mod.rs` shader list
- Added `pub mod shadow;`, `shadow_map` and `shadow_config` fields to Renderer
- Added `ShadowConfig` struct with runtime-adjustable parameters

### Task 2: CSM Rendering Integration + Shadow Sampling in Shaders
- Inserted `record_csm_shadow_passes()` in submit_frame between dispatch_chunk_cull and begin_render_pass
- Computes 4 cascade matrices using practical split scheme (λ=0.5 default)
- Tight-fits each cascade to camera frustum slice with texel grid snapping
- Writes shadow_matrices and cascade_splits directly to LightingParams SSBO
- Added shadow sampling functions to `common.glsl`:
  - `select_cascade()` — depth-based cascade selection
  - `shadow_sample_pcf()` — 3x3 PCF kernel using sampler2DArrayShadow
  - `sample_shadow_csm()` — full cascade selection + PCF + 10% blend zone
  - `apply_directional_light_shadowed()` — BRDF with shadow factor
- Integrated shadow sampling into `meshlet_draw.frag` and `chunk_mesh.frag`
- Added proper image layout transitions (attachment → read-only → attachment)

### Task 3: CSM egui Controls + Shadow Map Registration
- Created CSM during app init, registered at bindless binding 16
- Added "Shadows" egui window with controls:
  - Enable/disable toggle
  - Split lambda slider (0.0–1.0)
  - Bias constant/slope sliders (0.0–5.0)
  - Debug cascade colors toggle
  - Resolution and cascade count display

## Key Decisions (LGHT-02)
- CSM-01: Single 2D array image with 4 layers (not 4 separate images) for cleaner descriptor binding
- CSM-02: Comparison sampler (LESS_OR_EQUAL) enables hardware PCF via sampler2DArrayShadow
- CSM-03: Practical split scheme with λ=0.5 (blend linear/logarithmic) for cascade partitioning
- CSM-04: Texel grid snapping to prevent shadow shimmer during camera movement
- CSM-05: Depth bias constant=1.25, slope=1.75 for shadow acne prevention (adjustable via egui)
- CSM-06: 10% transition zone for cascade blending — flicker-free boundaries
- CSM-07: Border color FLOAT_OPAQUE_WHITE — out-of-bounds samples read as fully lit
- CSM-08: Z range extended 2x behind camera to catch shadow casters not in view
- CSM-09: Shadow passes reuse visible meshlet indirect buffer from cull pass
- CSM-10: cascade_matrices/splits written to LightingParams SSBO via direct mapped pointer write

## Verification
- [x] cargo build succeeds with shadow_depth.vert compiled to SPIR-V
- [x] cargo clippy --all-targets shows no new errors
- [x] cargo test passes (no regression)
- [x] 4 cascade depth images created at 2048x2048 (D32_SFLOAT)
- [x] Shadow depth passes execute before main render pass
- [x] PCF 3x3 kernel with cascade blending in fragment shaders
- [x] egui controls for shadow parameters
- [x] Proper Vulkan image layout transitions for shadow maps

## Files Modified
- `src/renderer/shadow.rs` (NEW) — CascadedShadowMap, compute_cascade_matrices, ShadowConfig
- `shaders/shadow_depth.vert` (NEW) — depth-only vertex shader
- `build.rs` — shader array 11→12
- `src/renderer/mod.rs` — pub mod shadow, shadow_map/shadow_config fields, Drop cleanup
- `src/renderer/submit.rs` — record_csm_shadow_passes, transition barriers
- `shaders/common.glsl` — shadow sampling functions, apply_directional_light_shadowed
- `shaders/meshlet_draw.frag` — CSM shadow integration
- `shaders/chunk_mesh.frag` — CSM shadow integration
- `src/app.rs` — CSM init, egui Shadows window
