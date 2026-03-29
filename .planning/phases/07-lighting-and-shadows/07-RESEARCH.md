# Phase 7: Lighting and Shadows — Research

**Researched:** 2026-03-29
**Method:** Direct codebase exploration + domain knowledge (Context7 MCP unavailable)

## 1. Current Codebase State

### Renderer Architecture
- **submit_frame** decomposed into named sub-functions: `wait_fence_and_prepare`, `acquire_image`, `begin_command_buffer`, `dispatch_chunk_cull`, `begin_render_pass`, `draw_meshlets`, `draw_egui`, `generate_hiz`, `present`
- **Render pass**: 4-attachment MSAA (MSAA color, MSAA depth, resolve color/swapchain, resolve depth/Hi-Z)
- **Clear color**: hardcoded `[0.1, 0.1, 0.15, 1.0]` — will be replaced by sky rendering
- **Double-buffered frames**: 2 in-flight, fence-synced

### BindlessTable (bindless.rs)
- **16 bindings** (0-15), all PARTIALLY_BOUND + UPDATE_AFTER_BIND
- Bindings 0-9: scene, indirect, frustum, draw count, Hi-Z, material, texture array
- Bindings 10-15: meshlet meta/vertex/tri/visible/indirect/count
- **Needs extension**: BINDING_COUNT must increase for shadow maps (4 cascade depth images), AO texture, sky texture, PBR texture arrays (MR, normal, emissive), point light SSBO
- Estimate: need ~8-10 more bindings → BINDING_COUNT from 16 to ~26

### BlockMaterial (material.rs)
- 8 bytes: `top_texture: u16, side_texture: u16, bottom_texture: u16, flags: u16`
- Flags: `FLAG_EMISSIVE = 0x01`, `FLAG_TRANSPARENT = 0x02`
- **Needs extension for PBR**: add MR/normal/emissive texture indices per face
- Current 8 bytes → needs expansion to ~24-32 bytes for 4 PBR maps × 3 faces

### TextureArray (texture_array.rs)
- Fixed 16×16 RGBA8, 256 max layers, mipmap chain, aniso filtering
- Procedurally generated textures (11 layers)
- **Needs**: Separate texture arrays for MR, normal, emissive maps
- **Mixed resolution**: Need either multiple VkImage arrays (16×16 + 32×32) or atlas approach

### PackedVertex (packing.rs)
- `uvec2` = 8 bytes per vertex
- word0: x(7) + y(7) + z(7) + face(3) + skirt(1) = 25 bits used of 32
- word1: block_id(16) + u(8) + v(8) = 32 bits fully used
- **7 free bits in word0** (bits 25-31), sufficient for AO (8 bits for 4 corners × 2 bits each)
- Can pack 4-corner AO into bits 24-31 of word0 (repurpose skirt bit 24 + use bits 25-31)

### Greedy Meshing (greedy.rs)
- `build_greedy_mesh` → `pack_quad` → meshoptimizer split → MeshletMesh
- Border skirt disabled (MSHL-05)
- **Integration point**: AO calculation goes between face visibility check and pack_quad call

### Hi-Z Pyramid (hiz.rs)
- R32_SFLOAT depth pyramid with full mip chain
- Generated each frame via compute shader (2×2 max downsample)
- Depth image transition: DEPTH_STENCIL_ATTACHMENT → SHADER_READ_ONLY → back
- **CSM can reuse**: same depth-only rendering pattern, separate render pass

### Camera (camera.rs)
- FPS camera with position, yaw, pitch, fov_y, near, far
- CameraUniforms: view_proj (64B) + camera_pos (12B) + pad (4B) = 80 bytes
- **Push constant budget**: Vulkan guarantees ≥128 bytes; current meshlet path uses 88B
- Need to pass sun_direction, time_of_day etc. — may need UBO instead of push constants

### Shader Pipeline (build.rs)
- shaderc with `#include "common.glsl"` support
- SPIR-V 1.5 target (Vulkan 1.2)
- Performance optimization enabled
- 11 shader files currently compiled

## 2. Technical Research: PBR Lighting

### Cook-Torrance BRDF
- **Diffuse**: Lambertian = albedo / π
- **Specular**: D(GGX/Trowbridge-Reitz) × G(Smith-GGX) × F(Schlick) / (4 × NdotL × NdotV)
- Normal Distribution Function: `D = α² / (π × ((NdotH² × (α² - 1) + 1)²))`
- Geometry Function: `G = G1(N,V) × G1(N,L)` where `G1 = 2NdotX / (NdotX + sqrt(α² + (1-α²)×NdotX²))`
- Fresnel: `F = F0 + (1 - F0) × (1 - VdotH)^5`
- For voxels: face normals are axis-aligned (6 directions), so NdotL/NdotV are cheap

### Normal Mapping in Voxel Context
- Face normals are flat (6 axis-aligned directions)
- Normal map perturbs in tangent space → need TBN matrix per face
- For axis-aligned faces, TBN is trivially constructible from face direction
- 16×16 normal maps add subtle surface detail to flat block faces

### Point Light System for Emissive Blocks
- Clustered forward or deferred needed for many point lights
- **Simpler approach for v1**: Forward with max N visible point lights (e.g., 32-64)
- Point light SSBO: position, color, radius, intensity per light
- Attenuation: `1 / (1 + linear*d + quadratic*d²)` or physically-based inverse-square with radius cutoff
- Light discovery: scan active chunks for emissive blocks, sort by distance, take top N

## 3. Technical Research: Cascaded Shadow Maps

### CSM Implementation
- 4 cascades, each with its own depth-only render pass
- Cascade split: practical split scheme `C_i = lerp(near × (far/near)^(i/N), near + (far-near)×i/N, λ)`
- λ = 0.5 is common compromise between logarithmic and uniform split
- Each cascade: orthographic projection from light direction, tightly fitted to camera frustum slice

### Vulkan CSM Resources
- **Depth images**: 4 × VkImage (D32_SFLOAT), configurable resolution (default 2048²)
- **Render pass**: depth-only, no color attachment, VK_ATTACHMENT_STORE_OP_STORE
- **Pipeline**: vertex-only pipeline (no fragment shader, or minimal for alpha-test)
- **Descriptor**: 4 cascade depth images as COMBINED_IMAGE_SAMPLER in bindless set
- **Shadow matrices**: 4 × mat4 light-space VP, passed via UBO

### Cascade Blending
- In fragment shader: determine cascade index from fragment depth
- At cascade boundaries: sample both cascades, lerp based on distance within transition zone
- Transition zone = ~10% of cascade range at boundary

### PCF Soft Shadows
- 3×3 or 5×5 kernel, sample depth map with offsets
- Compare each sample: `shadow += (depth < sample) ? 1.0 : 0.0`
- Divide by sample count for soft shadow factor
- Can use `textureGather` for efficient 2×2 block sampling

## 4. Technical Research: SSAO

### GTAO (Ground Truth Ambient Occlusion)
- Horizon-based: march along directions in screen space, find max elevation angle
- Compute shader: full-screen dispatch, reads depth buffer + normals
- Output: R8 or R16F single-channel AO texture
- Bilateral blur pass to smooth noise while preserving edges
- Typical: 8-16 directions, 4-8 steps per direction
- Performance: ~0.5-0.8ms at 1080p on modern GPUs

### HBAO+
- Similar to GTAO but with per-pixel random rotation and temporal accumulation
- More complex, slightly better quality

### Classic SSAO (John Chapman)
- Hemisphere kernel (32-64 samples), compare depth at each offset
- Cheaper per-sample but needs more samples for quality
- Performance: ~0.3-0.5ms at 1080p with 32 samples

### SSAO Pipeline in Vulkan
1. **Input**: resolved depth buffer (already available from Hi-Z source) + reconstructed normals
2. **Compute pass**: ssao_compute.comp → R8 AO texture
3. **Blur pass**: ssao_blur.comp → bilateral blur (horizontal + vertical)
4. **Composite**: multiply AO into lighting in the main fragment shader
5. **Half-resolution option**: compute SSAO at half-res for performance, upscale

### Normal Reconstruction from Depth
- Can reconstruct view-space normals from depth buffer using cross-product of screen-space derivatives
- `normal = normalize(cross(dFdx(viewPos), dFdy(viewPos)))` equivalent in compute
- Avoids needing a separate normal G-buffer

## 5. Technical Research: Voxel AO

### Classic 4-Corner AO (Minecraft-style)
- For each vertex of a face, check 3 neighboring voxels: side1, side2, corner
- AO level (0-3): `ao = (side1 + side2 == 2) ? 0 : 3 - (side1 + side2 + corner)`
- 0 = fully occluded (dark), 3 = fully open (bright)
- 2 bits per corner × 4 corners = 8 bits per quad vertex
- Interpolate in fragment shader for smooth gradient across face

### Integration with Greedy Meshing
- **Challenge**: greedy merge must only merge faces with identical AO patterns
- MergeKey needs to include AO values → reduces merge efficiency slightly
- Alternative: compute AO per-vertex after merge (sample at merged quad corners)
- **Recommendation**: compute AO at each quad corner position, store in packed vertex

### Packed Vertex AO Storage
- word0 bits 24-31 are available (bit 24 = skirt flag, now unused; bits 25-31 = free)
- Pack 4 AO values × 2 bits = 8 bits into word0[24:31]
- Layout: `ao0(2) | ao1(2) | ao2(2) | ao3(2)` in bits 24-31
- Fragment shader: extract AO for current corner via `gl_VertexIndex % 4` mapping, interpolate

## 6. Technical Research: Sky and Atmosphere

### Preetham Model
- Analytical model based on zenith luminance + sky chromaticity
- Inputs: sun direction (θs, φs), turbidity T
- Output: sky color for any view direction
- Fast: few trig operations per pixel
- Good for clear-sky rendering

### Hosek-Wilkie Model
- More physically accurate, handles wider range of atmospheric conditions
- 6th-order polynomial fit to Rayleigh+Mie scattering tables
- Inputs: same as Preetham + albedo (ground reflectance)
- Better sunset/sunrise colors

### Sky Rendering Approach
- Full-screen triangle or quad behind all geometry (depth = 1.0)
- Compute sky color per-pixel from view direction + sun position
- Alternatively: precompute into cubemap (update when sun moves significantly)

### Day-Night Cycle
- Sun direction: rotate around an axis at configurable speed
- `sun_dir = (cos(angle), sin(angle), 0)` in simplified 2D orbit
- Light color temperature: warm at dawn/dusk (3000K-4000K), neutral at noon (6500K)
- Night: switch to moon directional light (dim, blue-shifted)
- Smooth transition: crossfade sun→moon over ~10° below horizon

### Distance Fog
- **Linear**: `fog = clamp((end - dist) / (end - start), 0, 1)`
- **Exponential**: `fog = exp(-density × dist)` or `exp(-(density × dist)²)`
- **Height fog**: `fog = exp(-density × (dist × ray.y))` — denser at lower altitudes
- Fog color = sky color at horizon → seamless blend with sky

## 7. Resource Budget Analysis

### New Bindless Bindings Needed
| Binding | Type | Description |
|---------|------|-------------|
| 16 | COMBINED_IMAGE_SAMPLER×4 | CSM cascade depth maps (array or 4 separate) |
| 17 | COMBINED_IMAGE_SAMPLER | SSAO result texture |
| 18 | STORAGE_BUFFER | Shadow matrices UBO (4 × mat4 + split distances) |
| 19 | COMBINED_IMAGE_SAMPLER | MR texture array |
| 20 | COMBINED_IMAGE_SAMPLER | Normal map texture array |
| 21 | COMBINED_IMAGE_SAMPLER | Emissive texture array |
| 22 | STORAGE_BUFFER | Point light SSBO |
| 23 | STORAGE_BUFFER | Lighting params UBO (sun dir, time, fog params) |

**Total**: 24 bindings (up from 16)

### New Shader Files
| Shader | Type | Purpose |
|--------|------|---------|
| shadow_depth.vert | Vertex | CSM depth-only rendering |
| ssao_compute.comp | Compute | SSAO calculation |
| ssao_blur.comp | Compute | SSAO bilateral blur |
| sky.vert + sky.frag | Graphics | Fullscreen sky rendering |

### Push Constants vs UBO
- Current push constants: 88 bytes (MeshletDrawPushConstants)
- Vulkan minimum guarantee: 128 bytes
- Lighting needs: sun_direction(12) + sun_color(12) + time(4) + ambient(12) + fog params(16) = ~56 bytes
- **Approach**: Keep camera in push constants, move lighting/shadow data to SSBO/UBO (binding 18, 23)

### GPU Memory Budget (at 1080p)
- CSM: 4 × 2048² × 4B (D32_SFLOAT) = 64MB
- SSAO: 1920×1080 × 1B (R8) = ~2MB (+ blur intermediate)
- MR texture array: 256 × 16² × 4B = 4MB (+ 32² variant)
- Normal texture array: 256 × 16² × 4B = 4MB
- Emissive texture array: 256 × 16² × 4B = 4MB
- Point light SSBO: 64 × 32B = 2KB (negligible)
- Total new: ~78MB — acceptable for modern GPUs

## 8. Integration Risks and Mitigations

### Risk: Render Pass Restructuring
- Current: single MSAA render pass for all geometry + egui
- CSM needs: separate depth-only render passes (4×, before main pass)
- SSAO needs: compute passes between depth resolve and main lighting
- **Mitigation**: Insert CSM passes before main render pass, SSAO as compute between passes
- Sky renders as fullscreen quad in main pass (before or after geometry with depth test)

### Risk: Packed Vertex Format Change
- Adding AO bits to word0 changes vertex interpretation in ALL shaders
- **Mitigation**: AO bits in currently-unused bits 24-31, existing decode ignores those bits
- Shader changes are additive (read new bits), not breaking

### Risk: BlockMaterial Size Change
- Growing from 8 bytes to ~24+ bytes changes SSBO layout and shader struct
- **Mitigation**: Ensure all shaders use the new struct definition from common.glsl
- One-time migration, all shaders already reference material_ssbo

### Risk: Performance Budget
- Adding 4× CSM depth passes + SSAO compute + sky rendering
- **Mitigation**: CSM only renders active chunks (reuse cull results), SSAO at half-res option
- Budget target: total lighting overhead < 4ms at 1080p

## 9. Recommended Plan Decomposition

Based on dependency analysis:

1. **Plan 07-01**: PBR lighting foundation — expand BlockMaterial for 4 PBR maps, create MR/normal/emissive texture arrays, extend BindlessTable, implement Cook-Torrance BRDF in fragment shaders, add directional light
2. **Plan 07-02**: Voxel AO — implement 4-corner AO in greedy meshing, pack into vertex data, decode in shaders, integrate with lighting
3. **Plan 07-03**: Cascaded shadow maps — 4 cascade depth passes, shadow sampling in fragment shader, PCF, cascade blending, egui controls
4. **Plan 07-04**: SSAO — compute shader (GTAO default), bilateral blur, compositing, algorithm switching, half-res option
5. **Plan 07-05**: Sky/atmosphere + day-night + fog — sky shader, sun trajectory, moonlight, fog types, point lights from emissive blocks

---

*Phase: 07-lighting-and-shadows*
*Research completed: 2026-03-29*
