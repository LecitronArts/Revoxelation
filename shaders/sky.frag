#version 450
#extension GL_GOOGLE_include_directive : enable

#include "common.glsl"

layout(location = 0) in vec2 v_uv;
layout(location = 0) out vec4 out_color;

// Sky params SSBO at binding 23 (LGHT-05).
struct SkyParams {
    vec3 sun_direction;
    float turbidity;
    vec3 sun_color;
    float sun_angular_radius;
    vec3 ground_albedo;
    uint atmosphere_model;
    mat4 inv_view_proj;
    vec3 camera_pos;
    float _pad;
};

layout(std430, set = 0, binding = 23) readonly buffer SkyParamsBuffer {
    SkyParams sky;
} sky_data;

// -----------------------------------------------------------------------
// Preetham Sky Model
// -----------------------------------------------------------------------

// Perez function: F(theta, gamma) = (1 + A*exp(B/cos(theta))) * (1 + C*exp(D*gamma) + E*cos^2(gamma))
float perez(float theta, float gamma, float A, float B, float C, float D, float E) {
    float cos_gamma = cos(gamma);
    float cos_theta = max(cos(theta), 0.001); // avoid division by zero
    return (1.0 + A * exp(B / cos_theta)) * (1.0 + C * exp(D * gamma) + E * cos_gamma * cos_gamma);
}

vec3 preetham_sky_color(vec3 view_dir, vec3 sun_dir, float turbidity) {
    // Sun angular position
    float sun_theta = acos(max(sun_dir.y, 0.001)); // angle from zenith
    float cos_view_sun = clamp(dot(view_dir, sun_dir), -1.0, 1.0);
    float gamma = acos(cos_view_sun); // angle between view and sun

    float view_theta = acos(max(view_dir.y, 0.001)); // angle from zenith for view

    // Preetham distribution coefficients for CIE Y, x, y as function of turbidity (T)
    float T = turbidity;

    // Luminance Y coefficients
    float AY = 0.1787 * T - 1.4630;
    float BY = -0.3554 * T + 0.4275;
    float CY = -0.0227 * T + 5.3251;
    float DY = 0.1206 * T - 2.5771;
    float EY = -0.0670 * T + 0.3703;

    // Chromaticity x coefficients
    float Ax = -0.0193 * T - 0.2592;
    float Bx = -0.0665 * T + 0.0008;
    float Cx = -0.0004 * T + 0.2125;
    float Dx = -0.0641 * T - 0.8989;
    float Ex = -0.0033 * T + 0.0452;

    // Chromaticity y coefficients
    float Ay = -0.0167 * T - 0.2608;
    float By = -0.0950 * T + 0.0092;
    float Cy = -0.0079 * T + 0.2102;
    float Dy = -0.0441 * T - 1.6537;
    float Ey = -0.0109 * T + 0.0529;

    // Evaluate Perez function for the view direction and normalize by zenith
    float fY = perez(view_theta, gamma, AY, BY, CY, DY, EY) /
               perez(0.0, sun_theta, AY, BY, CY, DY, EY);
    float fx = perez(view_theta, gamma, Ax, Bx, Cx, Dx, Ex) /
               perez(0.0, sun_theta, Ax, Bx, Cx, Dx, Ex);
    float fy = perez(view_theta, gamma, Ay, By, Cy, Dy, Ey) /
               perez(0.0, sun_theta, Ay, By, Cy, Dy, Ey);

    // Zenith luminance (approximate)
    float chi = (4.0 / 9.0 - T / 120.0) * (PI - 2.0 * sun_theta);
    // Clamp chi away from ±π/2 to prevent tan() singularity (M2 fix).
    chi = clamp(chi, -1.5, 1.5);
    float Yz = (4.0453 * T - 4.9710) * tan(chi) - 0.2155 * T + 2.4192;
    Yz = max(Yz, 0.0);

    // Zenith chromaticity (simplified model)
    float T2 = T * T;
    float st = sun_theta;
    float st2 = st * st;
    float st3 = st2 * st;

    float xz = (0.00166 * st3 - 0.00375 * st2 + 0.00209 * st + 0.0) * T2 +
               (-0.02903 * st3 + 0.06377 * st2 - 0.03202 * st + 0.00394) * T +
               (0.11693 * st3 - 0.21196 * st2 + 0.06052 * st + 0.25886);

    float yz = (0.00275 * st3 - 0.00610 * st2 + 0.00317 * st + 0.0) * T2 +
               (-0.04214 * st3 + 0.08970 * st2 - 0.04153 * st + 0.00516) * T +
               (0.15346 * st3 - 0.26756 * st2 + 0.06670 * st + 0.26688);

    // Apply Perez ratios to zenith values
    float Y = Yz * fY;
    float x = xz * fx;
    float y = yz * fy;

    // CIE xyY to XYZ
    float Y_over_y = Y / max(y, 0.001);
    float X = x * Y_over_y;
    float Z = (1.0 - x - y) * Y_over_y;

    // XYZ to linear sRGB
    vec3 rgb;
    rgb.r =  3.2406 * X - 1.5372 * Y - 0.4986 * Z;
    rgb.g = -0.9689 * X + 1.8758 * Y + 0.0415 * Z;
    rgb.b =  0.0557 * X - 0.2040 * Y + 1.0570 * Z;

    return max(rgb, vec3(0.0));
}

// -----------------------------------------------------------------------
// Hosek-Wilkie Sky Model (Simplified)
// -----------------------------------------------------------------------
vec3 hosek_wilkie_sky_color(vec3 view_dir, vec3 sun_dir, float turbidity, vec3 ground_albedo) {
    // Simplified Hosek-Wilkie approximation
    float cos_sun_zenith = max(sun_dir.y, 0.001);
    float cos_view_sun = clamp(dot(view_dir, sun_dir), -1.0, 1.0);
    float gamma = acos(cos_view_sun);
    float cos_gamma = cos(gamma);

    float view_theta = acos(max(view_dir.y, 0.001));

    float T = turbidity;

    // Hosek-Wilkie model has 9 coefficients per channel;
    // we use a simplified 5-coefficient approximation here.
    float A = -1.0 - 0.32 * T;
    float B = -0.2 + 0.15 * T;
    float C = 3.0 + 0.3 * T;
    float D = -3.0 - 0.4 * T;
    float E = 0.45;

    float f = (1.0 + A * exp(B / max(cos(view_theta), 0.01))) *
              (1.0 + C * exp(D * gamma) + E * cos_gamma * cos_gamma);

    // Base sky color with turbidity influence
    vec3 zenith_color = mix(vec3(0.15, 0.3, 0.6), vec3(0.5, 0.55, 0.6), clamp(T / 10.0, 0.0, 1.0));

    // Ground albedo contribution (subtle)
    zenith_color += ground_albedo * 0.05;

    vec3 sky = zenith_color * max(f, 0.0) * 0.5;

    // Horizon brightening
    float horizon = 1.0 - max(view_dir.y, 0.0);
    horizon = horizon * horizon;
    sky += vec3(0.3, 0.25, 0.2) * horizon * (1.0 + T * 0.1);

    return max(sky, vec3(0.0));
}

// -----------------------------------------------------------------------
// Sun Disk Rendering
// -----------------------------------------------------------------------
vec3 render_sun(vec3 view_dir, vec3 sun_dir, float angular_radius, vec3 sun_color) {
    float cos_angle = dot(view_dir, sun_dir);
    float cos_radius = cos(angular_radius);

    if (cos_angle > cos_radius) {
        // Inside the sun disk — smooth falloff from center
        float t = (cos_angle - cos_radius) / (1.0 - cos_radius);
        t = clamp(t, 0.0, 1.0);
        // Limb darkening: center is bright, edge dims
        float limb = sqrt(t);
        return sun_color * limb * 50.0; // High intensity for sun disk
    }

    // Corona / glow around the sun
    float angle = acos(clamp(cos_angle, -1.0, 1.0));
    float glow = exp(-angle * angle / (angular_radius * angular_radius * 8.0));
    return sun_color * glow * 2.0;
}

// -----------------------------------------------------------------------
// Night sky (stars + moon glow)
// -----------------------------------------------------------------------
vec3 night_sky(vec3 view_dir, float sun_elevation) {
    // Fade from sky to dark as sun goes below horizon
    float night_factor = clamp(-sun_elevation / 0.15, 0.0, 1.0);
    if (night_factor < 0.01) return vec3(0.0);

    // Base night sky color (very dark blue)
    vec3 base = vec3(0.005, 0.007, 0.015) * night_factor;

    // Simple star field using hash
    // We use the view direction to create a pseudo-random pattern
    float star_hash = fract(sin(dot(floor(view_dir * 500.0), vec3(12.9898, 78.233, 45.5432))) * 43758.5453);
    if (star_hash > 0.998) {
        float brightness = (star_hash - 0.998) / 0.002;
        base += vec3(brightness * 0.3) * night_factor;
    }

    return base;
}

void main() {
    // Reconstruct view direction from UV + inverse view_proj
    vec4 clip = vec4(v_uv * 2.0 - 1.0, 1.0, 1.0);
    vec4 world = sky_data.sky.inv_view_proj * clip;
    vec3 view_dir = normalize(world.xyz / world.w - sky_data.sky.camera_pos);

    vec3 sun_dir = normalize(sky_data.sky.sun_direction);
    float sun_elevation = sun_dir.y; // sin of elevation angle

    vec3 sky_color;
    if (sky_data.sky.atmosphere_model == 0u) {
        sky_color = preetham_sky_color(view_dir, sun_dir, sky_data.sky.turbidity);
    } else {
        sky_color = hosek_wilkie_sky_color(view_dir, sun_dir,
                                            sky_data.sky.turbidity, sky_data.sky.ground_albedo);
    }

    // Add sun disk (only when sun is above horizon)
    if (sun_elevation > -0.05) {
        float sun_factor = clamp((sun_elevation + 0.05) / 0.1, 0.0, 1.0);
        sky_color += render_sun(view_dir, sun_dir, sky_data.sky.sun_angular_radius,
                                 sky_data.sky.sun_color) * sun_factor;
    }

    // Night sky contribution
    sky_color += night_sky(view_dir, sun_elevation);

    // Below-horizon darkening: view directions below the horizon fade to ground color
    if (view_dir.y < 0.0) {
        float below = clamp(-view_dir.y / 0.3, 0.0, 1.0);
        vec3 ground_color = sky_data.sky.ground_albedo * 0.1;
        sky_color = mix(sky_color, ground_color, below);
    }

    // Tone mapping (Reinhard)
    sky_color = sky_color / (sky_color + vec3(1.0));

    // Gamma correction
    sky_color = pow(sky_color, vec3(1.0 / 2.2));

    out_color = vec4(sky_color, 1.0);
}
