struct CameraGpu {
    position_lens: vec4<f32>,
    forward_fov: vec4<f32>,
    right_aspect: vec4<f32>,
    up_focus: vec4<f32>,
    clip_depth: vec4<f32>,
    resolution_frame: vec4<u32>,
};

struct TracerUniform {
    resolution_frame_chunks: vec4<u32>,
    chunk_map_info: vec4<u32>,
    emissive_info: vec4<u32>,
    importance_info: vec4<u32>,
    debug_map_stats: vec4<f32>,
    world_min: vec4<i32>,
    world_max: vec4<i32>,
    integrator: vec4<f32>,
    sun_dir: vec4<f32>,
    tuning_a: vec4<f32>,
    tuning_b: vec4<f32>,
    tuning_c: vec4<f32>,
    flags: vec4<u32>,
};

struct SvgfUniform {
    resolution_step: vec4<u32>,
    params: vec4<f32>,
    extras: vec4<f32>,
};

struct SurfaceSample {
    normal_material: vec4<f32>,
};

@group(0) @binding(0)
var output_tex: texture_storage_2d<__SVGF_STORAGE_FORMAT__, write>;
@group(0) @binding(1)
var<storage, read> accumulation: array<vec4<f32>>;
@group(0) @binding(2)
var<uniform> tracer: TracerUniform;
@group(0) @binding(3)
var<storage, read> surface_history: array<SurfaceSample>;
@group(0) @binding(4)
var<uniform> svgf: SvgfUniform;
@group(0) @binding(5)
var<storage, read_write> svgf_ping: array<vec4<f32>>;
@group(0) @binding(6)
var<storage, read_write> svgf_pong: array<vec4<f32>>;
@group(0) @binding(7)
var<uniform> camera: CameraGpu;
@group(0) @binding(8)
var<uniform> previous_camera: CameraGpu;
@group(0) @binding(9)
var<storage, read_write> debug_data: array<vec4<f32>>;

const OVERLAY_MODE_PROBE: u32 = 1u;
const OVERLAY_MODE_MOTION: u32 = 2u;
const OVERLAY_MODE_HISTORY_VALIDITY: u32 = 3u;
const OVERLAY_MODE_HISTORY_WEIGHT: u32 = 4u;
const OVERLAY_MODE_REJECT_REASON: u32 = 5u;
const OVERLAY_MODE_CLAMP_DIFF: u32 = 6u;
const OVERLAY_MODE_TEMPORAL_VARIANCE: u32 = 7u;

const REJECT_NONE: u32 = 0u;
const REJECT_INVALID_CURRENT: u32 = 1u;
const REJECT_OOB: u32 = 2u;
const REJECT_INVALID_PREVIOUS: u32 = 3u;
const REJECT_DEPTH: u32 = 4u;
const REJECT_NORMAL: u32 = 5u;
const REJECT_MOTION: u32 = 6u;

const EDGE_NORMAL_COS_THRESHOLD: f32 = 0.9063078; // cos(25deg)

fn in_bounds(pixel: vec2<i32>, resolution: vec2<i32>) -> bool {
    return all(pixel >= vec2<i32>(0)) && all(pixel < resolution);
}

fn pixel_index_of(pixel: vec2<i32>, width: u32) -> u32 {
    return u32(pixel.y) * width + u32(pixel.x);
}

fn history_read_slot() -> u32 {
    return tracer.flags.w & 1u;
}

fn history_write_slot() -> u32 {
    return (tracer.flags.w >> 1u) & 1u;
}

fn read_surface_slot(pixel_index: u32, slot: u32) -> SurfaceSample {
    let pixel_count = tracer.resolution_frame_chunks.x * tracer.resolution_frame_chunks.y;
    if slot == 0u {
        return surface_history[pixel_index];
    }
    return surface_history[pixel_index + pixel_count];
}

fn read_curr_surface(pixel_index: u32) -> SurfaceSample {
    return read_surface_slot(pixel_index, history_write_slot());
}

fn read_prev_surface(pixel_index: u32) -> SurfaceSample {
    return read_surface_slot(pixel_index, history_read_slot());
}

fn read_svgf_source(pixel_index: u32) -> vec4<f32> {
    if svgf.resolution_step.w == 0u {
        return svgf_ping[pixel_index];
    }
    return svgf_pong[pixel_index];
}

fn write_svgf_target(pixel_index: u32, value: vec4<f32>) {
    if svgf.resolution_step.w == 0u {
        svgf_pong[pixel_index] = value;
    } else {
        svgf_ping[pixel_index] = value;
    }
}

fn history_normal_reject_cos() -> f32 {
    return clamp(svgf.extras.z, 0.5, 0.999);
}

fn history_depth_reject_scale() -> f32 {
    return clamp(svgf.extras.w, 0.01, 0.5);
}

fn responsive_history_suppression(
    normal_cos: f32,
    depth_rel: f32,
    motion_pixels: f32,
    normal_threshold: f32,
    depth_threshold: f32,
) -> f32 {
    let normal_margin = clamp(
        (normal_cos - normal_threshold) / max(1.0 - normal_threshold, 1.0e-6),
        0.0,
        1.0,
    );
    let depth_margin = clamp(1.0 - depth_rel / max(depth_threshold, 1.0e-6), 0.0, 1.0);
    let motion_term = exp(-motion_pixels * 0.35);
    return clamp(normal_margin * depth_margin * motion_term, 0.0, 1.0);
}

fn safe_normalize(v: vec3<f32>) -> vec3<f32> {
    let len_sq = dot(v, v);
    if len_sq > 1.0e-8 {
        return v * inverseSqrt(len_sq);
    }
    return vec3<f32>(0.0, 0.0, 1.0);
}

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn reconstruct_world_position(pixel: vec2<i32>, depth: f32, cam: CameraGpu) -> vec3<f32> {
    let width = max(f32(cam.resolution_frame.x), 1.0);
    let height = max(f32(cam.resolution_frame.y), 1.0);
    let pixel_f = vec2<f32>(f32(pixel.x), f32(pixel.y));
    let uv = (pixel_f + vec2<f32>(0.5, 0.5)) / vec2<f32>(width, height);
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let tan_half_fov = tan(0.5 * cam.forward_fov.w);
    let depth_clamped = max(depth, cam.clip_depth.x);
    let x = ndc.x * depth_clamped * tan_half_fov * cam.right_aspect.w;
    let y = ndc.y * depth_clamped * tan_half_fov;
    return cam.position_lens.xyz
        + cam.forward_fov.xyz * depth_clamped
        + cam.right_aspect.xyz * x
        + cam.up_focus.xyz * y;
}

fn project_world_to_pixel(world_pos: vec3<f32>, cam: CameraGpu, resolution: vec2<i32>) -> vec2<i32> {
    let to_point = world_pos - cam.position_lens.xyz;
    let z = dot(to_point, cam.forward_fov.xyz);
    if z <= cam.clip_depth.x {
        return vec2<i32>(-1);
    }

    let tan_half_fov = tan(0.5 * cam.forward_fov.w);
    let aspect = max(cam.right_aspect.w, 1.0e-6);
    let x = dot(to_point, cam.right_aspect.xyz);
    let y = dot(to_point, cam.up_focus.xyz);
    let ndc = vec2<f32>(x / (z * tan_half_fov * aspect), y / (z * tan_half_fov));
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv >= vec2<f32>(1.0)) {
        return vec2<i32>(-1);
    }

    let width = max(f32(cam.resolution_frame.x), 1.0);
    let height = max(f32(cam.resolution_frame.y), 1.0);
    let pixel = vec2<i32>(i32(floor(uv.x * width)), i32(floor(uv.y * height)));
    if !in_bounds(pixel, resolution) {
        return vec2<i32>(-1);
    }
    return pixel;
}

fn local_luminance_variance(pixel: vec2<i32>, resolution: vec2<i32>, width: u32) -> f32 {
    var count = 0.0;
    var mean = 0.0;
    var mean_sq = 0.0;
    for (var oy: i32 = -1; oy <= 1; oy = oy + 1) {
        for (var ox: i32 = -1; ox <= 1; ox = ox + 1) {
            let neighbor = pixel + vec2<i32>(ox, oy);
            if !in_bounds(neighbor, resolution) {
                continue;
            }
            let value = accumulation[pixel_index_of(neighbor, width)].rgb;
            let luma = luminance(value);
            mean = mean + luma;
            mean_sq = mean_sq + luma * luma;
            count = count + 1.0;
        }
    }
    let inv_count = 1.0 / max(count, 1.0);
    let mu = mean * inv_count;
    return max(mean_sq * inv_count - mu * mu, 1.0e-6);
}

fn bilateral_weight(
    center_surface: SurfaceSample,
    sample_surface: SurfaceSample,
    center_luma: f32,
    sample_luma: f32,
    center_variance: f32,
) -> f32 {
    var normal_weight = 1.0;
    var depth_weight = 1.0;
    let center_valid = center_surface.normal_material.w > 0.0;
    let sample_valid = sample_surface.normal_material.w > 0.0;

    if center_valid && sample_valid {
        let center_normal = safe_normalize(center_surface.normal_material.xyz);
        let sample_normal = safe_normalize(sample_surface.normal_material.xyz);
        let normal_delta = max(0.0, 1.0 - clamp(dot(center_normal, sample_normal), 0.0, 1.0));
        normal_weight = exp(-normal_delta * normal_delta * max(svgf.params.x, 0.05) * 8.0);

        let depth_ref = max(center_surface.normal_material.w, 1.0e-4);
        let depth_delta =
            abs(center_surface.normal_material.w - sample_surface.normal_material.w) / depth_ref;
        let depth_sensitivity = max(svgf.params.y, 1.0);
        depth_weight = exp(-depth_delta * depth_sensitivity);
    } else {
        normal_weight = 0.25;
        depth_weight = 0.35;
    }

    let luma_sigma = max(
        1.0e-4,
        sqrt(max(center_variance, 1.0e-6)) * max(svgf.params.z, 0.25),
    );
    let luma_weight = exp(-abs(center_luma - sample_luma) / luma_sigma);
    return normal_weight * depth_weight * luma_weight;
}

fn aces_tonemap(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp(
        (color * (a * color + b)) / (color * (c * color + d) + e),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}

fn to_display_referred(radiance: vec3<f32>) -> vec3<f32> {
    let exposure = max(tracer.integrator.z, 1.0e-4);
    let cd_per_m2 = radiance * exposure * 0.31830988618;
    let mapped = aces_tonemap(cd_per_m2);
    return pow(mapped, vec3<f32>(1.0 / 2.2));
}

fn heatmap(value: f32) -> vec3<f32> {
    let x = clamp(value, 0.0, 1.0);
    let r = clamp(1.5 - abs(4.0 * x - 3.0), 0.0, 1.0);
    let g = clamp(1.5 - abs(4.0 * x - 2.0), 0.0, 1.0);
    let b = clamp(1.5 - abs(4.0 * x - 1.0), 0.0, 1.0);
    return vec3<f32>(r, g, b);
}

fn overlay_probe_debug(pixel: vec2<u32>, width: u32, base: vec3<f32>) -> vec3<f32> {
    if pixel.y >= 4u {
        return base;
    }

    let width_minus_one = select(width - 1u, 1u, width <= 1u);
    let x_norm = f32(pixel.x) / f32(width_minus_one);
    let avg_norm = clamp(tracer.debug_map_stats.x / 8.0, 0.0, 1.0);
    let observed_max_norm = clamp(tracer.debug_map_stats.y / 32.0, 0.0, 1.0);
    let load_norm = clamp(tracer.debug_map_stats.z, 0.0, 1.0);
    let budget_norm = clamp(f32(tracer.chunk_map_info.z) / 32.0, 0.0, 1.0);
    let dim = vec3<f32>(0.03, 0.03, 0.035);

    if pixel.y == 0u {
        return mix(
            dim,
            vec3<f32>(0.20, 0.90, 0.35),
            select(0.0, 1.0, x_norm <= avg_norm),
        );
    }
    if pixel.y == 1u {
        return mix(
            dim,
            vec3<f32>(0.95, 0.56, 0.18),
            select(0.0, 1.0, x_norm <= observed_max_norm),
        );
    }
    if pixel.y == 2u {
        return mix(
            dim,
            vec3<f32>(0.25, 0.62, 0.98),
            select(0.0, 1.0, x_norm <= load_norm),
        );
    }

    let observed = select(0.0, 1.0, x_norm <= observed_max_norm);
    let budget = select(0.0, 1.0, x_norm <= budget_norm);
    return vec3<f32>(budget, observed, observed);
}

fn reject_reason_color(reason: u32) -> vec3<f32> {
    switch reason {
        case REJECT_INVALID_CURRENT: {
            return vec3<f32>(0.85, 0.25, 0.25);
        }
        case REJECT_OOB: {
            return vec3<f32>(0.93, 0.55, 0.18);
        }
        case REJECT_INVALID_PREVIOUS: {
            return vec3<f32>(0.86, 0.30, 0.75);
        }
        case REJECT_DEPTH: {
            return vec3<f32>(0.95, 0.90, 0.18);
        }
        case REJECT_NORMAL: {
            return vec3<f32>(0.25, 0.70, 0.95);
        }
        case REJECT_MOTION: {
            return vec3<f32>(0.98, 0.35, 0.65);
        }
        default: {
            return vec3<f32>(0.12, 0.85, 0.24);
        }
    }
}

@compute @workgroup_size(8, 8, 1)
fn svgf_init_cs(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = tracer.resolution_frame_chunks.x;
    let height = tracer.resolution_frame_chunks.y;
    if gid.x >= width || gid.y >= height {
        return;
    }

    let resolution = vec2<i32>(i32(width), i32(height));
    let pixel = vec2<i32>(i32(gid.x), i32(gid.y));
    let pixel_index = gid.y * width + gid.x;
    let sample = accumulation[pixel_index];
    let current_surface = read_curr_surface(pixel_index);
    let current_variance = local_luminance_variance(pixel, resolution, width);

    var temporal_color = sample.rgb;
    var temporal_variance = current_variance;
    var history_weight = 0.0;
    var reject_reason: u32 = REJECT_NONE;
    var motion_pixels = 0.0;

    if tracer.resolution_frame_chunks.z > 0u {
        if current_surface.normal_material.w <= 0.0 {
            reject_reason = REJECT_INVALID_CURRENT;
        } else {
            let world_pos = reconstruct_world_position(pixel, current_surface.normal_material.w, camera);
            let previous_pixel = project_world_to_pixel(world_pos, previous_camera, resolution);

            if !in_bounds(previous_pixel, resolution) {
                reject_reason = REJECT_OOB;
            } else {
                let previous_index = pixel_index_of(previous_pixel, width);
                let previous_surface = read_prev_surface(previous_index);
                if previous_surface.normal_material.w <= 0.0 {
                    reject_reason = REJECT_INVALID_PREVIOUS;
                } else {
                    let current_normal = safe_normalize(current_surface.normal_material.xyz);
                    let previous_normal = safe_normalize(previous_surface.normal_material.xyz);
                    let normal_cos = clamp(dot(current_normal, previous_normal), 0.0, 1.0);
                    let normal_reject = history_normal_reject_cos();
                    let depth_reject = history_depth_reject_scale();
                    let depth_rel = abs(current_surface.normal_material.w - previous_surface.normal_material.w)
                        / max(current_surface.normal_material.w, 1.0e-4);

                    motion_pixels = length(
                        vec2<f32>(
                            f32(pixel.x - previous_pixel.x),
                            f32(pixel.y - previous_pixel.y),
                        ),
                    );

                    var motion_consistent = true;
                    let previous_world = reconstruct_world_position(
                        previous_pixel,
                        previous_surface.normal_material.w,
                        previous_camera,
                    );
                    let roundtrip_pixel = project_world_to_pixel(previous_world, camera, resolution);
                    if in_bounds(roundtrip_pixel, resolution) {
                        let roundtrip_error = length(
                            vec2<f32>(
                                f32(roundtrip_pixel.x - pixel.x),
                                f32(roundtrip_pixel.y - pixel.y),
                            ),
                        );
                        motion_pixels = max(motion_pixels, roundtrip_error);
                        let motion_limit =
                            1.5 + 0.6 * sqrt(max(current_surface.normal_material.w, 1.0));
                        motion_consistent = roundtrip_error <= motion_limit;
                    } else {
                        motion_consistent = false;
                    }

                    if normal_cos < normal_reject {
                        reject_reason = REJECT_NORMAL;
                    } else if depth_rel > depth_reject {
                        reject_reason = REJECT_DEPTH;
                    } else if !motion_consistent {
                        reject_reason = REJECT_MOTION;
                    } else {
                        let previous_filtered = read_svgf_source(previous_index);
                        let sigma_current = sqrt(max(current_variance, 1.0e-6));
                        let sigma_previous = sqrt(max(previous_filtered.a, 1.0e-6));

                        let normal_term = pow(normal_cos, max(svgf.params.x, 0.05));
                        let depth_term = exp(-depth_rel * (24.0 / max(svgf.params.y, 1.0)));
                        let motion_term = exp(-motion_pixels * 0.35);
                        let variance_term = clamp(
                            sigma_current / (sigma_current + sigma_previous + 1.0e-6),
                            0.15,
                            0.95,
                        );
                        let camera_motion_dampen =
                            select(1.0, 0.65, tracer.debug_map_stats.w > 0.5);
                        let responsive = responsive_history_suppression(
                            normal_cos,
                            depth_rel,
                            motion_pixels,
                            normal_reject,
                            depth_reject,
                        );

                        history_weight = clamp(
                            normal_term
                                * depth_term
                                * motion_term
                                * variance_term
                                * camera_motion_dampen
                                * responsive,
                            0.0,
                            0.97,
                        );
                        if responsive < 0.03 || history_weight < 0.005 {
                            history_weight = 0.0;
                            reject_reason = REJECT_MOTION;
                        }
                        temporal_color = mix(sample.rgb, previous_filtered.rgb, history_weight);
                        temporal_variance = mix(
                            current_variance,
                            previous_filtered.a,
                            history_weight * 0.9,
                        );
                    }
                }
            }
        }
    }

    let current_valid = current_surface.normal_material.w > 0.0;
    let current_normal = safe_normalize(current_surface.normal_material.xyz);
    let current_depth = max(current_surface.normal_material.w, 1.0e-4);
    var neighborhood_min = vec3<f32>(1.0e30, 1.0e30, 1.0e30);
    var neighborhood_max = vec3<f32>(-1.0e30, -1.0e30, -1.0e30);
    var neighborhood_mean = vec3<f32>(0.0);
    var neighborhood_mean_sq = vec3<f32>(0.0);
    var neighborhood_count = 0.0;

    for (var oy: i32 = -1; oy <= 1; oy = oy + 1) {
        for (var ox: i32 = -1; ox <= 1; ox = ox + 1) {
            let neighbor = pixel + vec2<i32>(ox, oy);
            if !in_bounds(neighbor, resolution) {
                continue;
            }
            let neighbor_index = pixel_index_of(neighbor, width);
            let neighbor_surface = read_curr_surface(neighbor_index);

            if current_valid && neighbor_surface.normal_material.w > 0.0 {
                let neighbor_normal = safe_normalize(neighbor_surface.normal_material.xyz);
                if dot(current_normal, neighbor_normal) < EDGE_NORMAL_COS_THRESHOLD {
                    continue;
                }
                let depth_delta = abs(current_depth - neighbor_surface.normal_material.w) / current_depth;
                if depth_delta > 0.12 {
                    continue;
                }
            }

            let neighbor_color = accumulation[neighbor_index].rgb;
            neighborhood_min = min(neighborhood_min, neighbor_color);
            neighborhood_max = max(neighborhood_max, neighbor_color);
            neighborhood_mean = neighborhood_mean + neighbor_color;
            neighborhood_mean_sq = neighborhood_mean_sq + neighbor_color * neighbor_color;
            neighborhood_count = neighborhood_count + 1.0;
        }
    }

    var clamp_delta = 0.0;
    if neighborhood_count > 0.0 {
        let inv_count = 1.0 / neighborhood_count;
        let mean = neighborhood_mean * inv_count;
        let variance = max(
            neighborhood_mean_sq * inv_count - mean * mean,
            vec3<f32>(1.0e-6),
        );
        let sigma = sqrt(variance);
        let clamp_sigma = max(svgf.params.w, 0.25);
        let lower = max(neighborhood_min, mean - sigma * clamp_sigma);
        let upper = min(neighborhood_max, mean + sigma * clamp_sigma);
        let clamped = clamp(temporal_color, lower, upper);
        clamp_delta = luminance(abs(clamped - temporal_color));
        temporal_color = clamped;
    }

    if !current_valid {
        temporal_variance = temporal_variance * max(svgf.extras.x, 1.0);
    }
    temporal_variance = max(temporal_variance, 1.0e-6);
    write_svgf_target(pixel_index, vec4<f32>(temporal_color, temporal_variance));

    let motion_debug = clamp(motion_pixels * 0.125, 0.0, 1.0);
    debug_data[pixel_index] = vec4<f32>(
        motion_debug,
        history_weight,
        f32(reject_reason),
        clamp_delta,
    );
}

@compute @workgroup_size(8, 8, 1)
fn svgf_atrous_cs(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = tracer.resolution_frame_chunks.x;
    let height = tracer.resolution_frame_chunks.y;
    if gid.x >= width || gid.y >= height {
        return;
    }

    let resolution = vec2<i32>(i32(width), i32(height));
    let pixel = vec2<i32>(i32(gid.x), i32(gid.y));
    let pixel_index = gid.y * width + gid.x;
    let step = i32(max(svgf.resolution_step.z, 1u));

    let center_surface = read_curr_surface(pixel_index);
    let center_value = read_svgf_source(pixel_index);
    let center_luma = luminance(center_value.rgb);
    let center_weight = max(svgf.extras.y, 0.5);

    var weight_sum = center_weight;
    var color_sum = center_value.rgb * center_weight;
    var variance_sum = center_value.a * center_weight;

    var neighborhood_min = center_value.rgb;
    var neighborhood_max = center_value.rgb;
    var neighborhood_mean = center_value.rgb;
    var neighborhood_mean_sq = center_value.rgb * center_value.rgb;
    var neighborhood_count = 1.0;

    for (var oy: i32 = -1; oy <= 1; oy = oy + 1) {
        for (var ox: i32 = -1; ox <= 1; ox = ox + 1) {
            if ox == 0 && oy == 0 {
                continue;
            }

            let neighbor = pixel + vec2<i32>(ox, oy) * step;
            if !in_bounds(neighbor, resolution) {
                continue;
            }

            let neighbor_index = pixel_index_of(neighbor, width);
            let neighbor_value = read_svgf_source(neighbor_index);
            let neighbor_surface = read_curr_surface(neighbor_index);
            var edge_gate_weight = 1.0;
            if center_surface.normal_material.w > 0.0 && neighbor_surface.normal_material.w > 0.0 {
                let center_normal = safe_normalize(center_surface.normal_material.xyz);
                let neighbor_normal = safe_normalize(neighbor_surface.normal_material.xyz);
                let normal_gate = max(EDGE_NORMAL_COS_THRESHOLD, history_normal_reject_cos());
                let normal_cos = dot(center_normal, neighbor_normal);
                if normal_cos < normal_gate {
                    let normal_gap = (normal_gate - normal_cos) / max(normal_gate, 1.0e-6);
                    edge_gate_weight = edge_gate_weight * exp(-normal_gap * 10.0);
                }

                let center_depth = max(center_surface.normal_material.w, 1.0e-4);
                let depth_rel = abs(center_surface.normal_material.w - neighbor_surface.normal_material.w)
                    / center_depth;
                let depth_gate = history_depth_reject_scale() * 1.5;
                if depth_rel > depth_gate {
                    let depth_excess = (depth_rel - depth_gate) / max(depth_gate, 1.0e-6);
                    edge_gate_weight = edge_gate_weight * exp(-depth_excess * 12.0);
                }
                if edge_gate_weight < 0.01 {
                    continue;
                }
            }

            let edge_weight = bilateral_weight(
                center_surface,
                neighbor_surface,
                center_luma,
                luminance(neighbor_value.rgb),
                center_value.a,
            );
            let spatial_weight = select(1.0, 2.0, ox == 0 || oy == 0);
            let sample_weight = spatial_weight * edge_weight * edge_gate_weight;

            weight_sum = weight_sum + sample_weight;
            color_sum = color_sum + neighbor_value.rgb * sample_weight;
            variance_sum = variance_sum + neighbor_value.a * sample_weight;

            neighborhood_min = min(neighborhood_min, neighbor_value.rgb);
            neighborhood_max = max(neighborhood_max, neighbor_value.rgb);
            neighborhood_mean = neighborhood_mean + neighbor_value.rgb;
            neighborhood_mean_sq = neighborhood_mean_sq + neighbor_value.rgb * neighbor_value.rgb;
            neighborhood_count = neighborhood_count + 1.0;
        }
    }

    let filtered = color_sum / max(weight_sum, 1.0e-6);
    let filtered_variance = variance_sum / max(weight_sum, 1.0e-6);

    let inv_count = 1.0 / max(neighborhood_count, 1.0);
    let mean = neighborhood_mean * inv_count;
    let variance = max(neighborhood_mean_sq * inv_count - mean * mean, vec3<f32>(1.0e-6));
    let sigma = sqrt(variance);
    let clamp_sigma = max(svgf.params.w, 0.0);
    let lower = max(neighborhood_min, mean - sigma * clamp_sigma);
    let upper = min(neighborhood_max, mean + sigma * clamp_sigma);
    let clamped = clamp(filtered, lower, upper);
    let clamp_delta = luminance(abs(clamped - filtered));

    write_svgf_target(pixel_index, vec4<f32>(clamped, max(filtered_variance, 1.0e-6)));

    let previous_debug = debug_data[pixel_index];
    debug_data[pixel_index] = vec4<f32>(
        previous_debug.x,
        previous_debug.y,
        previous_debug.z,
        max(previous_debug.w, clamp_delta),
    );
}

@compute @workgroup_size(8, 8, 1)
fn svgf_resolve_cs(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = tracer.resolution_frame_chunks.x;
    let height = tracer.resolution_frame_chunks.y;
    if gid.x >= width || gid.y >= height {
        return;
    }

    let pixel_index = gid.y * width + gid.x;
    let filtered = read_svgf_source(pixel_index);
    let debug = debug_data[pixel_index];
    var color = to_display_referred(filtered.rgb);

    switch tracer.flags.y {
        case OVERLAY_MODE_PROBE: {
            color = overlay_probe_debug(gid.xy, width, color);
        }
        case OVERLAY_MODE_MOTION: {
            color = heatmap(debug.x);
        }
        case OVERLAY_MODE_HISTORY_VALIDITY: {
            let accepted = debug.z == f32(REJECT_NONE) && debug.y > 1.0e-4;
            color = mix(
                vec3<f32>(0.85, 0.2, 0.2),
                vec3<f32>(0.15, 0.82, 0.28),
                select(0.0, 1.0, accepted),
            );
        }
        case OVERLAY_MODE_HISTORY_WEIGHT: {
            color = heatmap(clamp(debug.y, 0.0, 1.0));
        }
        case OVERLAY_MODE_REJECT_REASON: {
            color = reject_reason_color(u32(round(debug.z)));
        }
        case OVERLAY_MODE_CLAMP_DIFF: {
            let clamp_energy = 1.0 - exp(-debug.w * 6.0);
            color = heatmap(clamp(clamp_energy, 0.0, 1.0));
        }
        case OVERLAY_MODE_TEMPORAL_VARIANCE: {
            let variance_view = clamp(log2(filtered.a + 1.0) * 0.5, 0.0, 1.0);
            color = heatmap(variance_view);
        }
        default: {}
    }

    textureStore(output_tex, vec2<i32>(i32(gid.x), i32(gid.y)), vec4<f32>(color, 1.0));
}
