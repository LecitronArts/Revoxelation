#version 450
#extension GL_EXT_nonuniform_qualifier : require
#extension GL_GOOGLE_include_directive : enable

#include "common.glsl"

layout(location = 0) flat in uint v_block_id;
layout(location = 1) in vec3 v_face_normal;
layout(location = 2) in vec2 v_uv;
layout(location = 3) flat in float v_lod_transition;
layout(location = 4) flat in float v_fade_alpha;

layout(location = 0) out vec4 out_color;

// BlockMaterial SSBO at bindless binding 8.
// Layout matches Rust #[repr(C)]: 4 x u16 packed into 2 x uint (u32).
//   word0: top_texture (low 16) | side_texture (high 16)
//   word1: bottom_texture (low 16) | flags (high 16)
struct BlockMaterial {
    uint tex_top_side;      // top_texture (low 16), side_texture (high 16)
    uint tex_bottom_flags;  // bottom_texture (low 16), flags (high 16)
};

layout(std430, set = 0, binding = 8) readonly buffer MaterialBuffer {
    BlockMaterial materials[];
} material_ssbo;

// Texture array sampler at bindless binding 9.
layout(set = 0, binding = 9) uniform sampler2DArray tex_array;

void main() {
    // Alpha dither for LOD transitions (MSHL-05).
    // v_lod_transition ranges from 0 (fully opaque) to 1 (fully transparent at boundary).
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

    // Select texture index based on face normal:
    //   +Y (normal.y > 0.5) -> top
    //   -Y (normal.y < -0.5) -> bottom
    //   else -> side
    uint tex_index;
    if (v_face_normal.y > 0.5) {
        tex_index = top_tex;
    } else if (v_face_normal.y < -0.5) {
        tex_index = bottom_tex;
    } else {
        tex_index = side_tex;
    }

    // Sample texture array with nonuniformEXT for descriptor indexing safety
    vec4 texel = texture(tex_array, vec3(v_uv, float(nonuniformEXT(tex_index))));

    // Chunk fade-in via alpha dither (POLISH-08).
    // Newly spawned chunks fade from transparent to opaque over ~0.5s.
    float fade_alpha = v_fade_alpha;
    if (fade_alpha < 1.0) {
        float threshold = bayer_dither(ivec2(gl_FragCoord.xy));
        if (fade_alpha < threshold) {
            discard;
        }
    }

    out_color = texel;
}
