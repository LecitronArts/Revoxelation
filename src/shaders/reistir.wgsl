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

struct EmissiveVoxel {
    position_power: vec4<f32>,
};

struct Reservoir {
    z_i: u32,
    w_sum: f32,
    m_i: f32,
    w_var: f32,
};

struct GiReservoirState {
    head: vec4<u32>,
    accum: vec4<f32>,
    sample: vec4<f32>,
};

struct GiReservoir {
    dir_packed: u32,
    w_sum: f32,
    p_hat: f32,
    m: u32,
    sample_pos: vec3<f32>,
    sample_li: f32,
    sample_normal_packed: u32,
    sample_is_hit: u32,
};

struct SurfaceSample {
    normal_material: vec4<f32>,
};

@group(0) @binding(2)
var<uniform> camera: CameraGpu;
@group(0) @binding(3)
var<uniform> tracer: TracerUniform;
@group(0) @binding(7)
var<storage, read> emissive_voxels: array<EmissiveVoxel>;
@group(0) @binding(8)
var<storage, read_write> reservoir_a: array<Reservoir>;
@group(0) @binding(9)
var<storage, read_write> reservoir_b: array<Reservoir>;
@group(0) @binding(10)
var<uniform> previous_camera: CameraGpu;
@group(0) @binding(11)
var<storage, read_write> surface_history: array<SurfaceSample>;
@group(0) @binding(13)
var<storage, read> emissive_remap: array<u32>;
@group(0) @binding(14)
var<storage, read_write> gi_reservoir_a: array<GiReservoirState>;
@group(0) @binding(15)
var<storage, read_write> gi_reservoir_b: array<GiReservoirState>;

const PI: f32 = 3.14159265359;
const EPSILON: f32 = 1.0e-6;
const INVALID_EMITTER_INDEX: u32 = 0xffffffffu;
const RESERVOIR_INDEX_MASK: u32 = 0x00ffffffu;
const EDGE_NORMAL_COS_THRESHOLD: f32 = 0.9063078; // cos(25deg)

fn hash_u32(value: u32) -> u32 {
    var x = value;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    return x ^ (x >> 16u);
}

fn random_unit_f32(seed: u32) -> f32 {
    return f32(hash_u32(seed)) * (1.0 / 4294967296.0);
}

fn hash_pixel_frame_neighbor(pixel: vec2<i32>, frame: u32, neighbor: vec2<i32>) -> u32 {
    let pixel_hash = hash_u32(
        bitcast<u32>(pixel.x) * 0x9e3779b9u ^
            bitcast<u32>(pixel.y) * 0x85ebca6bu,
    );
    let neighbor_hash = hash_u32(
        bitcast<u32>(neighbor.x) * 0xc2b2ae35u ^
            bitcast<u32>(neighbor.y) * 0x27d4eb2du,
    );
    return hash_u32(pixel_hash ^ (frame * 0x165667b1u) ^ neighbor_hash);
}

fn in_bounds(pixel: vec2<i32>, resolution: vec2<i32>) -> bool {
    return all(pixel >= vec2<i32>(0)) && all(pixel < resolution);
}

fn pixel_index_of(pixel: vec2<i32>, width: u32) -> u32 {
    return u32(pixel.y) * width + u32(pixel.x);
}

fn pixel_coord_from_index(pixel_index: u32, width: u32) -> vec2<u32> {
    return vec2<u32>(pixel_index % width, pixel_index / width);
}

fn reservoir_empty() -> Reservoir {
    return Reservoir(RESERVOIR_INDEX_MASK, 0.0, 0.0, 0.0);
}

fn reservoir_index(z_i: u32) -> u32 {
    return z_i & RESERVOIR_INDEX_MASK;
}

fn encode_weight_hint(weight: f32) -> u32 {
    if weight <= 0.0 {
        return 0u;
    }
    let log2_w = clamp(log2(weight), -20.0, 20.0);
    let normalized = (log2_w + 20.0) * (1.0 / 40.0);
    let quantized = clamp(floor(normalized * 255.0 + 0.5), 0.0, 255.0);
    return u32(quantized);
}

fn pack_reservoir_index_weight(sample_index: u32, selected_weight: f32) -> u32 {
    let clamped_index = min(sample_index, RESERVOIR_INDEX_MASK - 1u);
    return (clamped_index & RESERVOIR_INDEX_MASK) | (encode_weight_hint(selected_weight) << 24u);
}

fn history_read_slot() -> u32 {
    return tracer.flags.w & 1u;
}

fn history_write_slot() -> u32 {
    return (tracer.flags.w >> 1u) & 1u;
}

fn read_prev_reservoir(pixel_index: u32) -> Reservoir {
    if history_read_slot() == 0u {
        return reservoir_a[pixel_index];
    }
    return reservoir_b[pixel_index];
}

fn read_curr_reservoir(pixel_index: u32) -> Reservoir {
    if history_write_slot() == 0u {
        return reservoir_a[pixel_index];
    }
    return reservoir_b[pixel_index];
}

fn write_curr_reservoir(pixel_index: u32, value: Reservoir) {
    if history_write_slot() == 0u {
        reservoir_a[pixel_index] = value;
    } else {
        reservoir_b[pixel_index] = value;
    }
}

fn gi_reservoir_empty() -> GiReservoir {
    return GiReservoir(0u, 0.0, 0.0, 0u, vec3<f32>(0.0), 0.0, 0u, 0u);
}

fn gi_from_state(reservoir: GiReservoirState) -> GiReservoir {
    return GiReservoir(
        reservoir.head.x,
        reservoir.accum.x,
        reservoir.accum.y,
        reservoir.head.w,
        reservoir.sample.xyz,
        reservoir.sample.w,
        reservoir.head.y,
        reservoir.head.z,
    );
}

fn gi_to_state(gi: GiReservoir) -> GiReservoirState {
    return GiReservoirState(
        vec4<u32>(gi.dir_packed, gi.sample_normal_packed, gi.sample_is_hit, gi.m),
        vec4<f32>(gi.w_sum, gi.p_hat, 0.0, 0.0),
        vec4<f32>(gi.sample_pos, gi.sample_li),
    );
}

fn read_prev_gi_reservoir(pixel_index: u32) -> GiReservoir {
    if history_read_slot() == 0u {
        return gi_from_state(gi_reservoir_a[pixel_index]);
    }
    return gi_from_state(gi_reservoir_b[pixel_index]);
}

fn read_curr_gi_reservoir(pixel_index: u32) -> GiReservoir {
    if history_write_slot() == 0u {
        return gi_from_state(gi_reservoir_a[pixel_index]);
    }
    return gi_from_state(gi_reservoir_b[pixel_index]);
}

fn write_curr_gi_reservoir(pixel_index: u32, value: GiReservoir) {
    if history_write_slot() == 0u {
        gi_reservoir_a[pixel_index] = gi_to_state(value);
    } else {
        gi_reservoir_b[pixel_index] = gi_to_state(value);
    }
}

fn read_curr_surface(pixel_index: u32) -> SurfaceSample {
    let pixel_count = tracer.resolution_frame_chunks.x * tracer.resolution_frame_chunks.y;
    if history_write_slot() == 0u {
        return surface_history[pixel_index];
    }
    return surface_history[pixel_index + pixel_count];
}

fn read_prev_surface(pixel_index: u32) -> SurfaceSample {
    let pixel_count = tracer.resolution_frame_chunks.x * tracer.resolution_frame_chunks.y;
    if history_read_slot() == 0u {
        return surface_history[pixel_index];
    }
    return surface_history[pixel_index + pixel_count];
}

fn remap_previous_emitter(previous_index: u32) -> u32 {
    let remap_count = tracer.emissive_info.z;
    if remap_count == 0u {
        return INVALID_EMITTER_INDEX;
    }
    if previous_index >= remap_count {
        return INVALID_EMITTER_INDEX;
    }
    return emissive_remap[previous_index];
}

fn reconstruct_camera_space_position(pixel: vec2<u32>, depth: f32, cam: CameraGpu) -> vec3<f32> {
    let width = max(f32(cam.resolution_frame.x), 1.0);
    let height = max(f32(cam.resolution_frame.y), 1.0);
    let uv = (vec2<f32>(vec2<i32>(pixel)) + vec2<f32>(0.5)) / vec2<f32>(width, height);
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let tan_half_fov = tan(0.5 * cam.forward_fov.w);
    let x = ndc.x * depth * tan_half_fov * cam.right_aspect.w;
    let y = ndc.y * depth * tan_half_fov;
    return cam.position_lens.xyz + cam.forward_fov.xyz * depth + cam.right_aspect.xyz * x + cam.up_focus.xyz * y;
}

fn edge_compatible(curr: SurfaceSample, neigh: SurfaceSample) -> bool {
    if curr.normal_material.w <= 0.0 || neigh.normal_material.w <= 0.0 {
        return false;
    }
    let curr_normal = normalize(curr.normal_material.xyz);
    let neigh_normal = normalize(neigh.normal_material.xyz);
    if dot(curr_normal, neigh_normal) < EDGE_NORMAL_COS_THRESHOLD {
        return false;
    }

    let curr_depth = max(curr.normal_material.w, 1.0e-4);
    let neigh_depth = neigh.normal_material.w;
    let depth_delta = abs(curr_depth - neigh_depth);
    let depth_limit = curr_depth * 0.10;
    return depth_delta <= depth_limit;
}

fn jacobian_compensation(
    curr_pos: vec3<f32>,
    curr_normal: vec3<f32>,
    neigh_pos: vec3<f32>,
    neigh_normal: vec3<f32>,
    light_pos: vec3<f32>,
) -> f32 {
    let to_light_curr = light_pos - curr_pos;
    let to_light_neigh = light_pos - neigh_pos;
    let dist_curr_sq = max(dot(to_light_curr, to_light_curr), 1.0e-6);
    let dist_neigh_sq = max(dot(to_light_neigh, to_light_neigh), 1.0e-6);

    let dir_curr = normalize(to_light_curr);
    let dir_neigh = normalize(to_light_neigh);
    let cos_curr = max(dot(curr_normal, dir_curr), 0.0);
    let cos_neigh = max(dot(neigh_normal, dir_neigh), 0.0);
    if cos_curr <= 0.0 || cos_neigh <= 1.0e-6 {
        return 0.0;
    }

    let distance_ratio = dist_neigh_sq / dist_curr_sq;
    let cosine_ratio = cos_curr / cos_neigh;
    let jacobian = distance_ratio * cosine_ratio;

    let jacobian_min = clamp(min(tracer.tuning_c.z, tracer.tuning_c.w), 0.01, 1.0);
    let jacobian_max = max(max(tracer.tuning_c.z, tracer.tuning_c.w), jacobian_min + 1.0e-3);
    return clamp(jacobian, jacobian_min, jacobian_max);
}

fn emitter_unshadowed_target(hit_position: vec3<f32>, hit_normal: vec3<f32>, emitter_index: u32) -> f32 {
    let emitter = emissive_voxels[emitter_index].position_power;
    let to_light = emitter.xyz - hit_position;
    let dist_sq = max(dot(to_light, to_light), 1.0e-4);
    let light_dir = to_light * inverseSqrt(dist_sq);
    let n_dot_l = max(dot(hit_normal, light_dir), 0.0);
    if n_dot_l <= 0.0 {
        return 0.0;
    }
    let intensity = emitter.w;
    let attenuation = n_dot_l / dist_sq;
    return max(intensity * attenuation, 0.0);
}

fn reservoir_update(
    reservoir: ptr<function, Reservoir>,
    sample_index: u32,
    sample_weight: f32,
    p_hat: f32,
    m_add: f32,
    random_tag: u32,
) {
    if sample_weight <= 0.0 || p_hat <= 0.0 || m_add <= 0.0 {
        return;
    }
    let new_w_sum = (*reservoir).w_sum + sample_weight;
    let threshold = sample_weight / max(new_w_sum, 1.0e-6);
    let random_seed = hash_u32(
        random_tag ^
            bitcast<u32>(sample_weight) ^
            (bitcast<u32>(p_hat) * 0x9e3779b9u) ^
            bitcast<u32>((*reservoir).m_i),
    );
    let pick = random_unit_f32(random_seed) < threshold;
    (*reservoir).w_sum = new_w_sum;
    (*reservoir).m_i = (*reservoir).m_i + m_add;
    if pick {
        (*reservoir).z_i = pack_reservoir_index_weight(sample_index, sample_weight);
        (*reservoir).w_var = 1.0 / max(p_hat, 1.0e-6);
    }
}

fn gi_reservoir_update(
    reservoir: ptr<function, GiReservoir>,
    sample: GiReservoir,
    sample_weight: f32,
    p_hat: f32,
    m_add: u32,
    random_tag: u32,
) {
    if sample_weight <= 0.0 || p_hat <= 0.0 || m_add == 0u {
        return;
    }
    let new_w_sum = (*reservoir).w_sum + sample_weight;
    let threshold = sample_weight / max(new_w_sum, 1.0e-6);
    let random_seed = hash_u32(
        random_tag ^
            bitcast<u32>(sample_weight) ^
            (bitcast<u32>(p_hat) * 0x85ebca6bu) ^
            ((*reservoir).m * 0x27d4eb2du),
    );
    let pick = random_unit_f32(random_seed) < threshold;
    (*reservoir).w_sum = new_w_sum;
    (*reservoir).m = (*reservoir).m + m_add;
    if pick {
        (*reservoir).dir_packed = sample.dir_packed;
        (*reservoir).sample_pos = sample.sample_pos;
        (*reservoir).sample_li = sample.sample_li;
        (*reservoir).sample_normal_packed = sample.sample_normal_packed;
        (*reservoir).sample_is_hit = sample.sample_is_hit;
        (*reservoir).p_hat = p_hat;
    }
}

fn spatial_reuse(
    reservoir: ptr<function, Reservoir>,
    pixel: vec2<i32>,
    curr_surface: SurfaceSample,
    curr_pos: vec3<f32>,
) {
    let width = tracer.resolution_frame_chunks.x;
    let height = tracer.resolution_frame_chunks.y;
    let resolution = vec2<i32>(i32(width), i32(height));
    let radius = clamp(i32(tracer.tuning_b.y), 0, 2);
    if radius == 0 {
        return;
    }

    let curr_normal = normalize(curr_surface.normal_material.xyz);
    let emitter_count = tracer.emissive_info.x;
    let temporal_boost = max(tracer.tuning_b.x, 0.0);
    let reuse_weight_cap = max(tracer.tuning_c.y, 1.0);
    let reuse_m_cap = max(tracer.tuning_c.x, 1.0);

    for (var oy: i32 = -2; oy <= 2; oy = oy + 1) {
        for (var ox: i32 = -2; ox <= 2; ox = ox + 1) {
            if abs(ox) > radius || abs(oy) > radius || (ox == 0 && oy == 0) {
                continue;
            }

            let neighbor = pixel + vec2<i32>(ox, oy);
            if !in_bounds(neighbor, resolution) {
                continue;
            }

            let neighbor_index = pixel_index_of(neighbor, width);
            let prev_surface = read_prev_surface(neighbor_index);
            if !edge_compatible(curr_surface, prev_surface) {
                continue;
            }

            let prev_reservoir = read_prev_reservoir(neighbor_index);
            if prev_reservoir.m_i <= 0.0 || prev_reservoir.w_sum <= 0.0 || prev_reservoir.w_var <= 0.0 {
                continue;
            }

            let prev_sample_index = reservoir_index(prev_reservoir.z_i);
            if prev_sample_index >= tracer.emissive_info.z {
                continue;
            }
            let mapped = remap_previous_emitter(prev_sample_index);
            if mapped == INVALID_EMITTER_INDEX || mapped >= emitter_count {
                continue;
            }

            let neigh_depth = prev_surface.normal_material.w;
            let neigh_pos = reconstruct_camera_space_position(vec2<u32>(neighbor), neigh_depth, previous_camera);
            let neigh_normal = normalize(prev_surface.normal_material.xyz);
            let light_pos = emissive_voxels[mapped].position_power.xyz;
            let jacobian = jacobian_compensation(curr_pos, curr_normal, neigh_pos, neigh_normal, light_pos);
            if jacobian <= 0.0 {
                continue;
            }

            let p_hat = emitter_unshadowed_target(curr_pos, curr_normal, mapped);
            if p_hat <= 0.0 {
                continue;
            }

            let prev_p_hat = max(1.0 / max(prev_reservoir.w_var, 1.0e-6), 1.0e-6);
            let prev_unbiased = prev_reservoir.w_sum / max(prev_reservoir.m_i * prev_p_hat, 1.0e-6);
            let talbot_mis = 1.0 / (1.0 + prev_reservoir.m_i * 0.5);
            var reused_weight = prev_unbiased * p_hat * temporal_boost * jacobian * talbot_mis;
            reused_weight = min(reused_weight, p_hat * reuse_weight_cap);
            if reused_weight <= 0.0 {
                continue;
            }

            let reused_m = clamp(prev_reservoir.m_i, 1.0, reuse_m_cap);
            let random_tag = hash_pixel_frame_neighbor(
                pixel,
                tracer.resolution_frame_chunks.z,
                neighbor,
            );
            reservoir_update(
                reservoir,
                mapped,
                reused_weight,
                p_hat,
                reused_m,
                random_tag,
            );
        }
    }
}

@compute @workgroup_size(8, 8, 1)
fn restir_spatial_cs(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = tracer.resolution_frame_chunks.x;
    let height = tracer.resolution_frame_chunks.y;
    if gid.x >= width || gid.y >= height {
        return;
    }

    let pixel_index = gid.y * width + gid.x;
    let curr_surface = read_curr_surface(pixel_index);
    var reused = read_curr_reservoir(pixel_index);
    if tracer.flags.x == 0u || curr_surface.normal_material.w <= 0.0 {
        write_curr_reservoir(pixel_index, reused);
        return;
    }
    if tracer.resolution_frame_chunks.z == 0u {
        write_curr_reservoir(pixel_index, reused);
        return;
    }

    let curr_pos = reconstruct_camera_space_position(gid.xy, curr_surface.normal_material.w, camera);
    if reused.w_sum <= 0.0 || reused.m_i <= 0.0 || reused.w_var <= 0.0 {
        reused = reservoir_empty();
    }
    spatial_reuse(&reused, vec2<i32>(vec2<u32>(gid.xy)), curr_surface, curr_pos);
    write_curr_reservoir(pixel_index, reused);
}
