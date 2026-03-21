#version 450

layout(location = 0) in uvec2 in_packed;
layout(location = 0) out vec3 v_color;

vec3 decode_position(uint word0) {
    uint x = word0 & 0x3Fu;
    uint y = (word0 >> 6) & 0x3Fu;
    uint z = (word0 >> 12) & 0x3Fu;
    return vec3(x, y, z);
}

void main() {
    vec3 local = decode_position(in_packed.x);
    uint block_id = in_packed.y & 0xFFFFu;
    vec3 centered = (local - vec3(32.0, 32.0, 32.0)) / vec3(32.0, 32.0, 96.0);
    gl_Position = vec4(centered.x, -centered.y, centered.z, 1.0);
    v_color = vec3(
        float((block_id % 5u) + 1u) / 6.0,
        float(((block_id / 5u) % 5u) + 1u) / 6.0,
        float(((block_id / 25u) % 5u) + 1u) / 6.0
    );
}
