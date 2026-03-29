// common.glsl — Shared shader definitions for Revoxelation (POLISH-04).
//
// Included by all shaders via #include "common.glsl".
// Contains shared struct definitions, utility functions, and constants
// to eliminate code duplication across the shader codebase.

// -----------------------------------------------------------------------
// GpuChunkInstance (64 bytes, matches Rust #[repr(C)] layout)
// -----------------------------------------------------------------------
//   aabb_min:     vec3  (12 bytes)
//   material_id:  uint  ( 4 bytes)
//   aabb_max:     vec3  (12 bytes)
//   lod_level:    uint  ( 4 bytes)
//   chunk_origin: vec3  (12 bytes)
//   chunk_scale:  float ( 4 bytes)
//   spawn_time:   float ( 4 bytes) — seconds since engine start (POLISH-08)
//   _pad_fade:    uint×3(12 bytes) — padding to 64 bytes
#ifndef COMMON_GLSL
#define COMMON_GLSL

struct GpuChunkInstance {
    vec3 aabb_min;
    uint material_id;
    vec3 aabb_max;
    uint lod_level;
    vec3 chunk_origin;
    float chunk_scale;
    float spawn_time;     // seconds since engine start (POLISH-08: fade-in)
    uint _pad_fade0;
    uint _pad_fade1;
    uint _pad_fade2;
};

// -----------------------------------------------------------------------
// GpuMeshlet (64 bytes, matches Rust #[repr(C)] layout)
// -----------------------------------------------------------------------
struct GpuMeshlet {
    vec3 center;           // bounding sphere center (local-space)
    float radius;          // bounding sphere radius
    vec3 cone_axis;        // orientation cone axis (normalized)
    float cone_cutoff;     // cos(half-angle); < -1.0 means degenerate (never cull)
    uint vertex_offset;    // into meshlet_vertex_buffer
    uint triangle_offset;  // into meshlet_tri_buffer
    uint vertex_count;     // max 64
    uint triangle_count;   // max 124
    uint chunk_slot;       // which chunk this meshlet belongs to
    uint lod_level;        // LOD level (0 = original, 1 = simplified)
    float parent_error;    // simplification error metric (MSHL-05)
    uint group_id;         // LOD group ID (MSHL-05)
};

// -----------------------------------------------------------------------
// Meshlet metadata constants
// -----------------------------------------------------------------------
const uint MESHLET_UINT32S = 16u; // 64 bytes / 4

// -----------------------------------------------------------------------
// Vertex decoding
// -----------------------------------------------------------------------

vec3 decode_position(uint word0) {
    uint x = word0 & 0x7Fu;
    uint y = (word0 >> 7) & 0x7Fu;
    uint z = (word0 >> 14) & 0x7Fu;
    return vec3(x, y, z);
}

// face_index: 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z
vec3 face_normal_from_index(uint fi) {
    if (fi == 0u) return vec3( 1.0, 0.0, 0.0);
    if (fi == 1u) return vec3(-1.0, 0.0, 0.0);
    if (fi == 2u) return vec3( 0.0, 1.0, 0.0);
    if (fi == 3u) return vec3( 0.0,-1.0, 0.0);
    if (fi == 4u) return vec3( 0.0, 0.0, 1.0);
    return            vec3( 0.0, 0.0,-1.0);
}

// -----------------------------------------------------------------------
// Bayer 8x8 dither matrix for smooth LOD transitions (MSHL-05)
// -----------------------------------------------------------------------
const float bayer8x8[64] = float[64](
     0.0/64.0, 32.0/64.0,  8.0/64.0, 40.0/64.0,  2.0/64.0, 34.0/64.0, 10.0/64.0, 42.0/64.0,
    48.0/64.0, 16.0/64.0, 56.0/64.0, 24.0/64.0, 50.0/64.0, 18.0/64.0, 58.0/64.0, 26.0/64.0,
    12.0/64.0, 44.0/64.0,  4.0/64.0, 36.0/64.0, 14.0/64.0, 46.0/64.0,  6.0/64.0, 38.0/64.0,
    60.0/64.0, 28.0/64.0, 52.0/64.0, 20.0/64.0, 62.0/64.0, 30.0/64.0, 54.0/64.0, 22.0/64.0,
     3.0/64.0, 35.0/64.0, 11.0/64.0, 43.0/64.0,  1.0/64.0, 33.0/64.0,  9.0/64.0, 41.0/64.0,
    51.0/64.0, 19.0/64.0, 59.0/64.0, 27.0/64.0, 49.0/64.0, 17.0/64.0, 57.0/64.0, 25.0/64.0,
    15.0/64.0, 47.0/64.0,  7.0/64.0, 39.0/64.0, 13.0/64.0, 45.0/64.0,  5.0/64.0, 37.0/64.0,
    63.0/64.0, 31.0/64.0, 55.0/64.0, 23.0/64.0, 61.0/64.0, 29.0/64.0, 53.0/64.0, 21.0/64.0
);

float bayer_dither(ivec2 coord) {
    int x = coord.x & 7;
    int y = coord.y & 7;
    return bayer8x8[y * 8 + x];
}

// -----------------------------------------------------------------------
// LOD transition alpha computation (MSHL-05, POLISH-01)
//
// Computes a transition factor [0..1] for alpha dither at LOD boundaries.
// 0.0 = fully opaque, 1.0 = fully transparent at boundary.
//
// Requires screen_height and sse_threshold from push constants (not hardcoded).
// -----------------------------------------------------------------------
const float MIN_LOD_DISTANCE = 0.001;

float compute_lod_transition(float parent_error, vec3 camera_pos, vec3 world_position,
                             float screen_height, float sse_threshold) {
    if (parent_error >= 1e30) {
        return 0.0;
    }
    float dist = length(camera_pos - world_position);
    if (dist < MIN_LOD_DISTANCE) dist = MIN_LOD_DISTANCE;
    float projected = parent_error * screen_height / (2.0 * dist);
    return clamp(1.0 - abs(projected - sse_threshold) / max(sse_threshold, MIN_LOD_DISTANCE), 0.0, 1.0);
}

// -----------------------------------------------------------------------
// CameraUniforms push constant layout (80 bytes)
// Declared as a struct but NOT as a layout — each shader declares its own
// layout(push_constant) with the appropriate offset.
// -----------------------------------------------------------------------
struct CameraUniforms {
    mat4 view_proj;
    vec3 camera_pos;
    float _pad;
};

// -----------------------------------------------------------------------
// PBR Lighting (LGHT-01)
// -----------------------------------------------------------------------
const float PI = 3.14159265359;

// Lighting params SSBO (binding 18)
struct LightingParams {
    vec3 sun_direction;
    float sun_intensity;
    vec3 sun_color;
    float ambient_intensity;
    vec3 ambient_color;
    float time_of_day;
    mat4 shadow_matrices[4];
    vec4 cascade_splits;
    vec3 fog_color;
    float fog_density;
    float fog_start;
    float fog_end;
    uint fog_type;
    uint _lp_pad;
};

// Point light struct (matches Rust PointLight #[repr(C)])
struct PointLight {
    vec3 position;
    float radius;
    vec3 color;
    float intensity;
};

// GGX Normal Distribution Function
float distribution_ggx(float NdotH, float roughness) {
    float a = roughness * roughness;
    float a2 = a * a;
    float denom = NdotH * NdotH * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom + 1e-7);
}

// Smith-GGX Geometry Function (Schlick-GGX approximation)
float geometry_schlick_ggx(float NdotV, float roughness) {
    float r = roughness + 1.0;
    float k = (r * r) / 8.0;
    return NdotV / (NdotV * (1.0 - k) + k + 1e-7);
}

float geometry_smith(float NdotV, float NdotL, float roughness) {
    return geometry_schlick_ggx(NdotV, roughness) * geometry_schlick_ggx(NdotL, roughness);
}

// Schlick Fresnel Approximation
vec3 fresnel_schlick(float cos_theta, vec3 F0) {
    return F0 + (1.0 - F0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// Full Cook-Torrance BRDF evaluation
// Returns combined diffuse + specular for a single light direction.
vec3 cook_torrance_brdf(vec3 N, vec3 V, vec3 L, vec3 albedo, float metallic, float roughness) {
    vec3 H = normalize(V + L);
    float NdotH = max(dot(N, H), 0.0);
    float NdotV = max(dot(N, V), 0.0);
    float NdotL = max(dot(N, L), 0.0);
    float HdotV = max(dot(H, V), 0.0);

    // Fresnel reflectance at normal incidence (F0)
    vec3 F0 = mix(vec3(0.04), albedo, metallic);

    // Cook-Torrance specular BRDF
    float D = distribution_ggx(NdotH, roughness);
    float G = geometry_smith(NdotV, NdotL, roughness);
    vec3  F = fresnel_schlick(HdotV, F0);

    vec3 numerator = D * G * F;
    float denominator = 4.0 * NdotV * NdotL + 1e-4;
    vec3 specular = numerator / denominator;

    // Energy conservation: diffuse = (1 - F) * (1 - metallic)
    vec3 kD = (vec3(1.0) - F) * (1.0 - metallic);
    vec3 diffuse = kD * albedo / PI;

    return diffuse + specular;
}

// Apply directional light using Cook-Torrance BRDF
vec3 apply_directional_light(vec3 N, vec3 V, vec3 world_pos, vec3 albedo,
                              float metallic, float roughness, LightingParams lp) {
    vec3 L = normalize(lp.sun_direction);
    float NdotL = max(dot(N, L), 0.0);

    vec3 brdf = cook_torrance_brdf(N, V, L, albedo, metallic, roughness);
    vec3 direct = brdf * lp.sun_color * lp.sun_intensity * NdotL;

    // Ambient term
    vec3 ambient = lp.ambient_color * lp.ambient_intensity * albedo;

    return direct + ambient;
}

// Evaluate a single point light contribution using Cook-Torrance BRDF
vec3 evaluate_point_light(vec3 N, vec3 V, vec3 world_pos, vec3 albedo,
                           float metallic, float roughness, PointLight light) {
    vec3 to_light = light.position - world_pos;
    float dist = length(to_light);
    if (dist > light.radius) return vec3(0.0);
    vec3 L = to_light / dist; // normalize

    // Attenuation: inverse-square with radius cutoff
    float attenuation = light.intensity / (1.0 + dist * dist);
    // Smooth falloff near radius boundary
    float falloff = 1.0 - smoothstep(light.radius * 0.75, light.radius, dist);
    attenuation *= falloff;

    vec3 brdf = cook_torrance_brdf(N, V, L, albedo, metallic, roughness);
    float NdotL = max(dot(N, L), 0.0);

    return brdf * light.color * attenuation * NdotL;
}

// Apply directional light using Cook-Torrance BRDF with shadow factor (LGHT-02).
vec3 apply_directional_light_shadowed(vec3 N, vec3 V, vec3 world_pos, vec3 albedo,
                              float metallic, float roughness, LightingParams lp, float shadow) {
    vec3 L = normalize(lp.sun_direction);
    float NdotL = max(dot(N, L), 0.0);

    vec3 brdf = cook_torrance_brdf(N, V, L, albedo, metallic, roughness);
    vec3 direct = brdf * lp.sun_color * lp.sun_intensity * NdotL * shadow;

    // Ambient term (not affected by shadow)
    vec3 ambient = lp.ambient_color * lp.ambient_intensity * albedo;

    return direct + ambient;
}

// -----------------------------------------------------------------------
// Voxel AO decoding (LGHT-04)
// -----------------------------------------------------------------------
// Bits 24-25 of word0 encode per-vertex AO (0=dark, 3=bright).
// Convert to float: 0->0.2, 1->0.5, 2->0.75, 3->1.0
float decode_vertex_ao(uint word0) {
    uint ao_bits = (word0 >> 24) & 0x3u;
    // AO curve: slightly non-linear for better visual contrast
    const float ao_values[4] = float[4](0.2, 0.5, 0.75, 1.0);
    return ao_values[ao_bits];
}

// -----------------------------------------------------------------------
// CSM Shadow Sampling (LGHT-02)
// -----------------------------------------------------------------------

// Select cascade index from view-space depth (linear depth from camera).
uint select_cascade(float view_depth, vec4 cascade_splits) {
    for (uint i = 0u; i < 4u; i++) {
        if (view_depth < cascade_splits[i]) {
            return i;
        }
    }
    return 3u;
}

// PCF shadow sampling with 3x3 kernel using sampler2DArrayShadow.
// shadow_coord.xy = UV in shadow map, shadow_coord.z = reference depth.
float shadow_sample_pcf(sampler2DArrayShadow csm, vec3 shadow_coord, uint layer, float texel_size) {
    float shadow = 0.0;
    for (int x = -1; x <= 1; x++) {
        for (int y = -1; y <= 1; y++) {
            vec2 offset = vec2(float(x), float(y)) * texel_size;
            // sampler2DArrayShadow: texture(sampler, vec4(uv, layer, ref_depth))
            shadow += texture(csm, vec4(shadow_coord.xy + offset, float(layer), shadow_coord.z));
        }
    }
    return shadow / 9.0;
}

// Full CSM shadow sampling with cascade selection and blending.
// Returns shadow factor: 1.0 = fully lit, 0.0 = fully shadowed.
float sample_shadow_csm(sampler2DArrayShadow csm, LightingParams lp,
                         vec3 world_pos, float view_depth, float shadow_resolution) {
    uint cascade = select_cascade(view_depth, lp.cascade_splits);
    float texel_size = 1.0 / shadow_resolution;

    // Project world position into shadow space.
    vec4 shadow_pos = lp.shadow_matrices[cascade] * vec4(world_pos, 1.0);
    vec3 shadow_coord = shadow_pos.xyz / shadow_pos.w;
    // Map from [-1,1] to [0,1] for UV, z already in [0,1] for Vulkan.
    shadow_coord.xy = shadow_coord.xy * 0.5 + 0.5;

    // Out-of-bounds check — fragments outside shadow map are fully lit.
    if (shadow_coord.x < 0.0 || shadow_coord.x > 1.0 ||
        shadow_coord.y < 0.0 || shadow_coord.y > 1.0 ||
        shadow_coord.z < 0.0 || shadow_coord.z > 1.0) {
        return 1.0;
    }

    float shadow = shadow_sample_pcf(csm, shadow_coord, cascade, texel_size);

    // Cascade blending: blend with next cascade in 10% transition zone.
    if (cascade < 3u) {
        float split_near = (cascade == 0u) ? 0.0 : lp.cascade_splits[cascade - 1u];
        float split_far = lp.cascade_splits[cascade];
        float range = split_far - split_near;
        float blend_zone = range * 0.1; // 10% transition zone
        float dist_to_edge = split_far - view_depth;

        if (dist_to_edge < blend_zone && blend_zone > 0.0) {
            // Sample next cascade.
            uint next_cascade = cascade + 1u;
            vec4 next_shadow_pos = lp.shadow_matrices[next_cascade] * vec4(world_pos, 1.0);
            vec3 next_coord = next_shadow_pos.xyz / next_shadow_pos.w;
            next_coord.xy = next_coord.xy * 0.5 + 0.5;

            float next_shadow = shadow_sample_pcf(csm, next_coord, next_cascade, texel_size);
            float blend_factor = dist_to_edge / blend_zone;
            shadow = mix(next_shadow, shadow, blend_factor);
        }
    }

    return shadow;
}

// -----------------------------------------------------------------------
// Distance Fog (LGHT-05)
// -----------------------------------------------------------------------
// fog_type: 0=linear, 1=exponential, 2=exponential squared, 3=height
vec3 apply_distance_fog(vec3 color, vec3 fog_color, float dist, float fog_start, float fog_end,
                        float fog_density, uint fog_type, vec3 world_pos) {
    float fog_factor;
    if (fog_type == 0u) {
        // Linear fog
        fog_factor = clamp((fog_end - dist) / (fog_end - fog_start + 0.001), 0.0, 1.0);
    } else if (fog_type == 1u) {
        // Exponential fog
        fog_factor = exp(-fog_density * dist);
    } else if (fog_type == 2u) {
        // Exponential squared fog
        float f = fog_density * dist;
        fog_factor = exp(-f * f);
    } else {
        // Height fog: denser at lower altitudes
        float height_factor = exp(-fog_density * max(world_pos.y, 0.0) * 0.01);
        fog_factor = exp(-fog_density * dist * height_factor);
    }
    fog_factor = clamp(fog_factor, 0.0, 1.0);
    return mix(fog_color, color, fog_factor);
}

#endif // COMMON_GLSL
