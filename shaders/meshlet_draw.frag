#version 450
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_GOOGLE_include_directive : enable

#include "common.glsl"

layout(location = 0) flat in uint v_block_id;
layout(location = 1) in vec3 v_face_normal;
layout(location = 2) in vec2 v_uv;
layout(location = 3) flat in float v_lod_transition;
layout(location = 4) flat in float v_fade_alpha;
layout(location = 5) in vec3 v_world_pos;
layout(location = 6) in float v_voxel_ao;

layout(location = 0) out vec4 out_color;

// BlockMaterial SSBO at bindless binding 8.
// Layout matches Rust #[repr(C)]: 16 x u16 packed into 8 x uint (u32) = 32 bytes.
//   word0: top_texture (low 16) | side_texture (high 16)
//   word1: bottom_texture (low 16) | flags (high 16)
//   word2: top_mr (low 16) | side_mr (high 16)
//   word3: bottom_mr (low 16) | top_normal (high 16)
//   word4: side_normal (low 16) | bottom_normal (high 16)
//   word5: top_emissive (low 16) | side_emissive (high 16)
//   word6: bottom_emissive (low 16) | emissive_intensity (high 16)
//   word7: _pad0 (low 16) | _pad1 (high 16)
struct BlockMaterial {
    uint tex_top_side;        // word0
    uint tex_bottom_flags;    // word1
    uint mr_top_side;         // word2
    uint mr_bottom_norm_top;  // word3
    uint norm_side_bottom;    // word4
    uint emissive_top_side;   // word5
    uint emissive_bottom_intensity; // word6
    uint _pad;                // word7
};

layout(std430, set = 0, binding = 8) readonly buffer MaterialBuffer {
    BlockMaterial materials[];
} material_ssbo;

// Texture array sampler at bindless binding 9.
layout(set = 0, binding = 9) uniform sampler2DArray tex_array;

// CSM shadow maps at binding 16 (LGHT-02).
layout(set = 0, binding = 16) uniform sampler2DArrayShadow csm_shadow_maps;

// SSAO texture at binding 17 (LGHT-03).
layout(set = 0, binding = 17) uniform sampler2D ssao_texture;

// Lighting params SSBO at binding 18 (LGHT-01).
layout(std430, set = 0, binding = 18) readonly buffer LightingBuffer {
    LightingParams lighting;
} lighting_ssbo;

// Point light SSBO at binding 22 (LGHT-01).
layout(std430, set = 0, binding = 22) readonly buffer PointLightBuffer {
    uint point_light_count;
    uint max_point_lights;
    uint _pad[2];
    PointLight point_lights[];
} point_light_data;

// Push constants — camera_pos needed for V vector.
layout(push_constant) uniform PushConstants {
    mat4 view_proj;
    vec3 camera_pos;
    float screen_height;
    float sse_threshold;
    float current_time;
} pc;

void main() {
    // Alpha dither for LOD transitions (MSHL-05).
    if (v_lod_transition > MIN_LOD_DISTANCE) {
        float threshold = bayer_dither(ivec2(gl_FragCoord.xy));
        float alpha = 1.0 - v_lod_transition;
        if (alpha < threshold) {
            discard;
        }
    }

    BlockMaterial mat = material_ssbo.materials[v_block_id];

    uint top_tex    = mat.tex_top_side & 0xFFFFu;
    uint side_tex   = (mat.tex_top_side >> 16) & 0xFFFFu;
    uint bottom_tex = mat.tex_bottom_flags & 0xFFFFu;
    uint flags      = (mat.tex_bottom_flags >> 16) & 0xFFFFu;

    // Select texture index based on face normal
    uint tex_index;
    if (v_face_normal.y > 0.5) {
        tex_index = top_tex;
    } else if (v_face_normal.y < -0.5) {
        tex_index = bottom_tex;
    } else {
        tex_index = side_tex;
    }

    // Sample albedo texture
    vec4 texel = texture(tex_array, vec3(v_uv, float(nonuniformEXT(tex_index))));
    bool is_transparent = (flags & 0x02u) != 0u;
    if (is_transparent && texel.a < 0.5) {
        discard;
    }

    // Chunk fade-in via alpha dither (POLISH-08).
    float fade_alpha = v_fade_alpha;
    if (fade_alpha < 1.0) {
        float threshold = bayer_dither(ivec2(gl_FragCoord.xy));
        if (fade_alpha < threshold) {
            discard;
        }
    }

    // PBR parameters: default metallic=0.0, roughness=0.8
    float metallic = 0.0;
    float roughness = 0.8;

    // Face normal (axis-aligned, trivially cheap)
    vec3 N = normalize(v_face_normal);
    vec3 V = normalize(pc.camera_pos - v_world_pos);

    // Apply directional light with Cook-Torrance BRDF (LGHT-01).
    LightingParams lp = lighting_ssbo.lighting;

    // CSM shadow sampling (LGHT-02).
    // Compute linear view depth from camera for cascade selection.
    float view_depth = length(pc.camera_pos - v_world_pos);
    float shadow_factor = 1.0;
    if (lp.cascade_splits.x > 0.0) {
        shadow_factor = sample_shadow_csm(
            csm_shadow_maps,
            lp,
            v_world_pos,
            view_depth,
            max(lp.render_params.z, 1.0)
        );
    }

    vec3 lit_color = apply_directional_light_shadowed(N, V, v_world_pos, texel.rgb, metallic, roughness, lp, shadow_factor);

    // Voxel AO (LGHT-04) combined with SSAO (LGHT-03): apply to ambient term only.
    // Sample SSAO from binding 17 (screen-space AO computed by SSAO pass).
    vec2 screen_uv = gl_FragCoord.xy / max(lp.render_params.xy, vec2(1.0));
    float ssao = texture(ssao_texture, screen_uv).r;
    float final_ao = v_voxel_ao * ssao;
    // Subtract unmodulated ambient, re-add with AO applied.
    vec3 ambient_raw = lp.ambient_color * lp.ambient_intensity * texel.rgb;
    lit_color = lit_color - ambient_raw + ambient_raw * final_ao;

    // Accumulate point light contributions (LGHT-01).
    uint num_lights = min(point_light_data.point_light_count, point_light_data.max_point_lights);
    for (uint i = 0u; i < num_lights; i++) {
        lit_color += evaluate_point_light(N, V, v_world_pos, texel.rgb, metallic, roughness,
                                           point_light_data.point_lights[i]);
    }

    // Emissive contribution (self-illumination from emissive blocks).
    if ((flags & 0x01u) != 0u) {
        // FLAG_EMISSIVE — add self-glow
        uint emissive_intensity_raw = (mat.emissive_bottom_intensity >> 16) & 0xFFFFu;
        float emissive_strength = float(emissive_intensity_raw) / 256.0; // fixed-point 8.8
        lit_color += texel.rgb * max(emissive_strength, 0.5);
    }

    // Distance fog — applied after all lighting (LGHT-05).
    if (lp.fog_type != FOG_DISABLED) {
        float dist_to_camera = length(v_world_pos - pc.camera_pos);
        lit_color = apply_distance_fog(lit_color, lp.fog_color, dist_to_camera,
                                        lp.fog_start, lp.fog_end, lp.fog_density, lp.fog_type, v_world_pos);
    }

    out_color = vec4(lit_color, is_transparent ? texel.a : 1.0);
}
