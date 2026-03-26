#version 450

layout(location = 0) in uvec2 in_packed;

layout(location = 0) flat out uint v_block_id;
layout(location = 1) out vec3 v_face_normal;
layout(location = 2) out vec2 v_uv;

// GpuChunkInstance (48 bytes, matches Rust #[repr(C)] layout):
//   aabb_min:     vec3  (12 bytes)
//   material_id:  uint  ( 4 bytes)
//   aabb_max:     vec3  (12 bytes)
//   lod_level:    uint  ( 4 bytes)
//   chunk_origin: vec3  (12 bytes)
//   chunk_scale:  float ( 4 bytes)
struct GpuChunkInstance {
    vec3 aabb_min;
    uint material_id;
    vec3 aabb_max;
    uint lod_level;
    vec3 chunk_origin;
    float chunk_scale;
};

// Unified scene_buffer (D-07). Region 0 = GpuChunkInstance[capacity].
// Vertex shader only needs region 0, so we can safely declare the SSBO
// with an unsized GpuChunkInstance array.
layout(std430, set = 0, binding = 0) readonly buffer SceneBuffer {
    GpuChunkInstance instances[];
} scene_data;

layout(push_constant) uniform CameraUniforms {
    mat4 view_proj;
    vec3 camera_pos;
} camera;

vec3 decode_position(uint word0) {
    uint x = word0 & 0x7Fu;
    uint y = (word0 >> 7) & 0x7Fu;
    uint z = (word0 >> 14) & 0x7Fu;
    return vec3(x, y, z);
}

// face_index: 0=+X, 1=-X, 2=+Y, 3=-Y, 4=+Z, 5=-Z
vec3 face_normal_from_index(uint fi) {
    // Look-up table for 6 face normals
    if (fi == 0u) return vec3( 1.0, 0.0, 0.0);
    if (fi == 1u) return vec3(-1.0, 0.0, 0.0);
    if (fi == 2u) return vec3( 0.0, 1.0, 0.0);
    if (fi == 3u) return vec3( 0.0,-1.0, 0.0);
    if (fi == 4u) return vec3( 0.0, 0.0, 1.0);
    return            vec3( 0.0, 0.0,-1.0);
}

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

    // Extract block_id from word1 low 16 bits
    v_block_id = word1 & 0xFFFFu;

    // Face normal for fragment shader material lookup
    v_face_normal = face_normal_from_index(face_index);

    // UV coordinates from packed vertex (bits 16-23 = u, bits 24-31 = v)
    float u = float((word1 >> 16) & 0xFFu);
    float v = float((word1 >> 24) & 0xFFu);
    v_uv = vec2(u, v);
}
