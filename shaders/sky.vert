#version 450

layout(location = 0) out vec2 v_uv;

void main() {
    // Fullscreen triangle trick: 3 vertices cover the entire screen.
    // No vertex buffer needed — positions generated from gl_VertexIndex.
    v_uv = vec2((gl_VertexIndex << 1) & 2, gl_VertexIndex & 2);
    gl_Position = vec4(v_uv * 2.0 - 1.0, 1.0, 1.0); // depth = 1.0 (far plane)
}
