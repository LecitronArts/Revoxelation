#version 450

layout(location = 0) in uvec2 in_packed;
layout(location = 0) out vec3 v_color;

struct ChunkDrawMetadata {
    vec3 aabb_min;
    uint slot_id;
    vec3 aabb_max;
    uint first_index;
    int vertex_offset;
    uint index_count;
    uint lod_level;
    uint _padding0;
    vec3 chunk_origin;
    float chunk_scale;
};

layout(std430, set = 0, binding = 0) readonly buffer ChunkMetadataBuffer {
    ChunkDrawMetadata metadata[];
} chunk_metadata;

vec3 decode_position(uint word0) {
    uint x = word0 & 0x7Fu;
    uint y = (word0 >> 7) & 0x7Fu;
    uint z = (word0 >> 14) & 0x7Fu;
    return vec3(x, y, z);
}

vec4 debug_project(vec3 world_position) {
    // Camera at origin, view along +Z, top-down-ish oblique
    // World range: LOD0 chunks span roughly [-256..320] per axis
    float range = 400.0;
    vec3 centered = world_position / range;
    // Vulkan NDC depth [0,1]
    float depth = centered.z * 0.5 + 0.5;
    // Y-up: negate Y for Vulkan clip (top = -Y in Vulkan NDC)
    return vec4(centered.x, -centered.y, depth, 1.0);
}

void main() {
    ChunkDrawMetadata metadata = chunk_metadata.metadata[gl_InstanceIndex];

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

    vec3 local = (pos + face_offset) * metadata.chunk_scale;
    uint block_id = in_packed.y & 0xFFFFu;
    vec3 world_position = metadata.chunk_origin + local;
    gl_Position = debug_project(world_position);
    v_color = vec3(
        float((block_id % 5u) + 1u) / 6.0,
        float(((block_id / 5u) % 5u) + 1u) / 6.0,
        float(((block_id / 25u) % 5u) + 1u) / 6.0
    );
}
