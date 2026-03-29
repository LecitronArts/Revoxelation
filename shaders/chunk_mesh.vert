#version 450
#extension GL_GOOGLE_include_directive : enable

#include "common.glsl"

layout(location = 0) in uvec2 in_packed;

layout(location = 0) flat out uint v_block_id;
layout(location = 1) out vec3 v_face_normal;
layout(location = 2) out vec2 v_uv;
layout(location = 5) out vec3 v_world_pos;
layout(location = 6) out float v_voxel_ao;

// Unified scene_buffer (D-07). Region 0 = GpuChunkInstance[capacity].
// Vertex shader only needs region 0, so we can safely declare the SSBO
// with an unsized GpuChunkInstance array.
layout(std430, set = 0, binding = 0) readonly buffer SceneBuffer {
    GpuChunkInstance instances[];
} scene_data;

layout(push_constant) uniform PushConstants {
    mat4 view_proj;
    vec3 camera_pos;
} camera;

void main() {
    // gl_InstanceIndex = firstInstance = slot_id (D-04)
    GpuChunkInstance inst = scene_data.instances[gl_InstanceIndex];

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
    gl_Position = camera.view_proj * vec4(world_position, 1.0);
    v_world_pos = world_position;

    // Extract block_id from word1 low 16 bits
    v_block_id = word1 & 0xFFFFu;

    // Face normal for fragment shader material lookup
    v_face_normal = face_normal_from_index(face_index);

    // UV coordinates from packed vertex (bits 16-23 = u, bits 24-31 = v)
    float u = float((word1 >> 16) & 0xFFu);
    float v = float((word1 >> 24) & 0xFFu);
    v_uv = vec2(u, v);

    // Voxel AO (LGHT-04): decode per-vertex AO from word0 bits 24-25.
    v_voxel_ao = decode_vertex_ao(word0);
}
