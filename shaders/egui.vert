#version 450

layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_uv;
layout(location = 2) in vec4 in_color; // R8G8B8A8_UNORM, auto-normalized by hardware

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_color;

layout(push_constant) uniform PushConstants {
    vec2 screen_size; // width, height in points
} pc;

void main() {
    gl_Position = vec4(
        2.0 * in_pos.x / pc.screen_size.x - 1.0,
        2.0 * in_pos.y / pc.screen_size.y - 1.0,
        0.0,
        1.0
    );
    v_uv = in_uv;
    v_color = in_color;
}
