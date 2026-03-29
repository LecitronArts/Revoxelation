#version 450
#extension GL_ARB_shader_draw_parameters : enable
#extension GL_GOOGLE_include_directive : enable

#include "common.glsl"

layout(location = 0) in uvec2 in_packed;

layout(location = 0) flat out uint v_block_id;
layout(location = 1) out vec3 v_face_normal;
layout(location = 2) out vec2 v_uv;
layout(location = 3) flat out float v_lod_transition;
layout(location = 4) flat out float v_fade_alpha;
layout(location = 5) out vec3 v_world_pos;
layout(location = 6) out float v_voxel_ao;

// Unified scene_buffer (D-07). Region 0 = GpuChunkInstance[capacity].
layout(std430, set = 0, binding = 0) readonly buffer SceneBuffer {
    GpuChunkInstance instances[];
} scene_data;

// GpuMeshlet metadata SSBO (binding 10, 64 bytes per meshlet = 16 uint32).
layout(std430, set = 0, binding = 10) readonly buffer MeshletMetaBuffer {
    uint data[];
} meshlet_meta;

// Visible meshlet indices (binding 13, output of meshlet_cull.comp).
layout(std430, set = 0, binding = 13) readonly buffer VisibleMeshletBuffer {
    uint visible_meshlets[];
} visible_buf;

layout(push_constant) uniform PushConstants {
    mat4 view_proj;
    vec3 camera_pos;
    float screen_height;  // POLISH-01: from push constant, not hardcoded
    float sse_threshold;  // POLISH-01: from push constant, not hardcoded
    float current_time;   // POLISH-08: seconds since engine start, for chunk fade-in
} pc;

// GpuMeshlet loader — reads chunk_slot from raw uint array.
uint load_meshlet_chunk_slot(uint meshlet_id) {
    uint base = meshlet_id * MESHLET_UINT32S;
    return meshlet_meta.data[base + 12]; // chunk_slot is at offset 12
}

// Load parent_error (float at offset 14) for LOD transition.
float load_meshlet_parent_error(uint meshlet_id) {
    uint base = meshlet_id * MESHLET_UINT32S;
    return uintBitsToFloat(meshlet_meta.data[base + 14]);
}

// Load lod_level (uint at offset 13).
uint load_meshlet_lod_level(uint meshlet_id) {
    uint base = meshlet_id * MESHLET_UINT32S;
    return meshlet_meta.data[base + 13];
}

void main() {
    // gl_DrawID indexes into visible_meshlet_buffer -> get meshlet_id
    uint meshlet_id = visible_buf.visible_meshlets[gl_DrawIDARB];

    // meshlet_id -> GpuMeshlet.chunk_slot -> GpuChunkInstance
    uint chunk_slot = load_meshlet_chunk_slot(meshlet_id);
    GpuChunkInstance inst = scene_data.instances[chunk_slot];

    uint word0 = in_packed.x;
    uint word1 = in_packed.y;

    vec3 pos = decode_position(word0);

    uint face_index = (word0 >> 21) & 0x7u;
    uint face_axis = face_index / 2u;
    bool is_positive = (face_index % 2u) == 0u;
    vec3 face_offset = vec3(0.0);
    if (is_positive) {
        if (face_axis == 0u) face_offset.x = 1.0;
        else if (face_axis == 1u) face_offset.y = 1.0;
        else face_offset.z = 1.0;
    }

    vec3 local = (pos + face_offset) * inst.chunk_scale;
    vec3 world_position = inst.chunk_origin + local;
    gl_Position = pc.view_proj * vec4(world_position, 1.0);
    v_world_pos = world_position;

    // Extract block_id from word1 low 16 bits
    v_block_id = word1 & 0xFFFFu;

    // Face normal for fragment shader material lookup
    v_face_normal = face_normal_from_index(face_index);

    // UV coordinates from packed vertex (bits 16-23 = u, bits 24-31 = v)
    float u = float((word1 >> 16) & 0xFFu);
    float v = float((word1 >> 24) & 0xFFu);
    v_uv = vec2(u, v);

    // LOD transition factor for alpha dither (MSHL-05).
    // Uses parameterized screen_height and sse_threshold from push constants (POLISH-01).
    float parent_error = load_meshlet_parent_error(meshlet_id);
    v_lod_transition = compute_lod_transition(
        parent_error, pc.camera_pos, world_position,
        pc.screen_height, pc.sse_threshold
    );

    // Chunk fade-in alpha (POLISH-08): 0→1 over FADE_DURATION seconds.
    const float FADE_DURATION = 0.5;
    float fade_alpha = clamp((pc.current_time - inst.spawn_time) / FADE_DURATION, 0.0, 1.0);
    v_fade_alpha = fade_alpha;

    // Voxel AO (LGHT-04): decode per-vertex AO from word0 bits 24-25.
    v_voxel_ao = decode_vertex_ao(word0);
}
