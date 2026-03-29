#version 450
#extension GL_ARB_shader_draw_parameters : enable
#extension GL_GOOGLE_include_directive : enable

#include "common.glsl"

layout(location = 0) in uvec2 in_packed;

// Scene buffer for chunk instance data (binding 0).
layout(std430, set = 0, binding = 0) readonly buffer SceneBuffer {
    GpuChunkInstance instances[];
} scene_data;

// Meshlet metadata SSBO (binding 10).
layout(std430, set = 0, binding = 10) readonly buffer MeshletMetaBuffer {
    uint data[];
} meshlet_meta;

// Visible meshlet indices (binding 13).
layout(std430, set = 0, binding = 13) readonly buffer VisibleMeshletBuffer {
    uint visible_meshlets[];
} visible_buf;

layout(push_constant) uniform ShadowPushConstants {
    mat4 light_view_proj;
} pc;

// Load chunk_slot from meshlet metadata (offset 12 in the 16-uint struct).
uint load_meshlet_chunk_slot(uint meshlet_id) {
    uint base = meshlet_id * MESHLET_UINT32S;
    return meshlet_meta.data[base + 12];
}

void main() {
    // gl_DrawID indexes into visible_meshlet_buffer -> get meshlet_id
    uint meshlet_id = visible_buf.visible_meshlets[gl_DrawIDARB];

    // meshlet_id -> GpuMeshlet.chunk_slot -> GpuChunkInstance
    uint chunk_slot = load_meshlet_chunk_slot(meshlet_id);
    GpuChunkInstance inst = scene_data.instances[chunk_slot];

    uint word0 = in_packed.x;

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
    gl_Position = pc.light_view_proj * vec4(world_position, 1.0);
}
