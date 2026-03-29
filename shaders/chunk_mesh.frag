#version 450
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_GOOGLE_include_directive : enable

#include "common.glsl"

layout(location = 0) flat in uint v_block_id;
layout(location = 1) in vec3 v_face_normal;
layout(location = 2) in vec2 v_uv;
layout(location = 5) in vec3 v_world_pos;

layout(location = 0) out vec4 out_color;

// BlockMaterial SSBO at bindless binding 8.
// Layout matches Rust #[repr(C)]: 16 x u16 packed into 8 x uint (u32) = 32 bytes.
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

// Push constants — camera_pos from CameraUniforms.
layout(push_constant) uniform PushConstants {
    mat4 view_proj;
    vec3 camera_pos;
} camera;

void main() {
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

    // PBR parameters: default metallic=0.0, roughness=0.8
    float metallic = 0.0;
    float roughness = 0.8;

    // Face normal (axis-aligned)
    vec3 N = normalize(v_face_normal);
    vec3 V = normalize(camera.camera_pos - v_world_pos);

    // Apply directional light with Cook-Torrance BRDF (LGHT-01).
    LightingParams lp = lighting_ssbo.lighting;
    vec3 lit_color = apply_directional_light(N, V, v_world_pos, texel.rgb, metallic, roughness, lp);

    // Accumulate point light contributions (LGHT-01).
    uint num_lights = min(point_light_data.point_light_count, point_light_data.max_point_lights);
    for (uint i = 0u; i < num_lights; i++) {
        lit_color += evaluate_point_light(N, V, v_world_pos, texel.rgb, metallic, roughness,
                                           point_light_data.point_lights[i]);
    }

    // Emissive contribution
    if ((flags & 0x01u) != 0u) {
        uint emissive_intensity_raw = (mat.emissive_bottom_intensity >> 16) & 0xFFFFu;
        float emissive_strength = float(emissive_intensity_raw) / 256.0;
        lit_color += texel.rgb * max(emissive_strength, 0.5);
    }

    out_color = vec4(lit_color, texel.a);
}
