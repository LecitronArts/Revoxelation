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

struct ChunkMeta {
    coord_size: vec4<i32>,
    voxel_offset: u32,
    voxel_count: u32,
    _pad0: u32,
    _pad1: u32,
};

struct ChunkMapEntry {
    key_value: vec4<i32>,
    probe_meta: vec4<u32>,
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
    surface_meta: vec4<u32>,
};

struct Ray {
    origin: vec3<f32>,
    dir: vec3<f32>,
};

struct Hit {
    hit: bool,
    distance: f32,
    position: vec3<f32>,
    normal: vec3<f32>,
    material: u32,
    emissive: u32,
};

@group(0) @binding(0)
var output_tex: texture_storage_2d<__TRACE_STORAGE_FORMAT__, write>;
@group(0) @binding(1)
var<storage, read_write> accumulation: array<vec4<f32>>;
@group(0) @binding(2)
var<uniform> camera: CameraGpu;
@group(0) @binding(3)
var<uniform> tracer: TracerUniform;
@group(0) @binding(4)
var<storage, read> voxels: array<u32>;
@group(0) @binding(5)
var<storage, read> chunk_metas: array<ChunkMeta>;
@group(0) @binding(6)
var<storage, read> chunk_map: array<ChunkMapEntry>;
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
@group(0) @binding(12)
var<storage, read> emissive_cdf: array<f32>;
@group(0) @binding(13)
var<storage, read> emissive_remap: array<u32>;
@group(0) @binding(14)
var<storage, read_write> gi_reservoir_a: array<GiReservoirState>;
@group(0) @binding(15)
var<storage, read_write> gi_reservoir_b: array<GiReservoirState>;
@group(0) @binding(16)
var importance_map: texture_3d<f32>;

const PI: f32 = 3.14159265359;
const EPSILON: f32 = 1e-3;
const CHUNK_SIZE: i32 = 32;
const MAX_DDA_STEPS_LIMIT: u32 = 2048u;
const INVALID_EMITTER_INDEX: u32 = 0xffffffffu;
const RESERVOIR_INDEX_MASK: u32 = 0x00ffffffu;
const OVERLAY_MODE_PROBE: u32 = 1u;

fn hash_u32(value: u32) -> u32 {
    var x = value;
    x = x + 0x7ed55d16u + (x << 12u);
    x = x ^ 0xc761c23cu ^ (x >> 19u);
    x = x + 0x165667b1u + (x << 5u);
    x = x + 0xd3a2646cu ^ (x << 9u);
    x = x + 0xfd7046c5u + (x << 3u);
    x = x ^ 0xb55a4f09u ^ (x >> 16u);
    return x;
}

fn init_seed(pixel: vec2<u32>, frame: u32) -> u32 {
    let packed = pixel.x * 1973u + pixel.y * 9277u + frame * 26699u + 911u;
    return hash_u32(packed);
}

fn rng_next(seed: ptr<function, u32>) -> f32 {
    var s = *seed;
    s ^= s << 13u;
    s ^= s >> 17u;
    s ^= s << 5u;
    *seed = s;
    return f32(s) * (1.0 / 4294967296.0);
}

fn sample_disk_from_u(sample: vec2<f32>) -> vec2<f32> {
    let r = sqrt(sample.x);
    let theta = 2.0 * PI * sample.y;
    return vec2<f32>(r * cos(theta), r * sin(theta));
}

fn permute_sample_index(index: u32, seed: u32) -> u32 {
    return (index * 40503u + (seed | 1u)) & 0x00ffffffu;
}

fn radical_inverse_base2(bits: u32) -> f32 {
    var x = bits;
    x = (x << 16u) | (x >> 16u);
    x = ((x & 0x55555555u) << 1u) | ((x & 0xaaaaaaaau) >> 1u);
    x = ((x & 0x33333333u) << 2u) | ((x & 0xccccccccu) >> 2u);
    x = ((x & 0x0f0f0f0fu) << 4u) | ((x & 0xf0f0f0f0u) >> 4u);
    x = ((x & 0x00ff00ffu) << 8u) | ((x & 0xff00ff00u) >> 8u);
    return f32(x) * 2.3283064365386963e-10;
}

fn radical_inverse_base3(index: u32) -> f32 {
    var n = index;
    var reversed = 0.0;
    var inv_base_pow = 1.0 / 3.0;
    loop {
        if n == 0u {
            break;
        }
        let digit = n % 3u;
        reversed = reversed + f32(digit) * inv_base_pow;
        n = n / 3u;
        inv_base_pow = inv_base_pow * (1.0 / 3.0);
    }
    return reversed;
}

fn permuted_sample_2d(pixel: vec2<u32>, frame: u32, dimension: u32) -> vec2<f32> {
    let pixel_seed = hash_u32(
        pixel.x * 1973u + pixel.y * 9277u + dimension * 26699u + 1013u,
    );
    let sample_index = permute_sample_index(frame + dimension * 8191u, pixel_seed);
    let cp_x = f32(hash_u32(pixel_seed ^ 0x68bc21ebu)) * (1.0 / 4294967296.0);
    let cp_y = f32(hash_u32(pixel_seed ^ 0x02e5be93u)) * (1.0 / 4294967296.0);
    return vec2<f32>(
        fract(radical_inverse_base2(sample_index) + cp_x),
        fract(radical_inverse_base3(sample_index) + cp_y),
    );
}

fn sample_unit_sphere(seed: ptr<function, u32>) -> vec3<f32> {
    let z = rng_next(seed) * 2.0 - 1.0;
    let a = 2.0 * PI * rng_next(seed);
    let r = sqrt(max(0.0, 1.0 - z * z));
    return vec3<f32>(r * cos(a), r * sin(a), z);
}

fn sample_isotropic_hemisphere_from_u(normal: vec3<f32>, sample_u: vec2<f32>) -> vec3<f32> {
    let z = sample_u.x;
    let azimuth = 2.0 * PI * sample_u.y;
    let r = sqrt(max(0.0, 1.0 - z * z));
    let local = vec3<f32>(r * cos(azimuth), r * sin(azimuth), z);
    let helper = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(normal.y) > 0.999);
    let tangent = normalize(cross(helper, normal));
    let bitangent = cross(normal, tangent);
    return normalize(tangent * local.x + bitangent * local.y + normal * local.z);
}

fn sample_isotropic_hemisphere(normal: vec3<f32>, seed: ptr<function, u32>) -> vec3<f32> {
    return sample_isotropic_hemisphere_from_u(
        normalize(normal),
        vec2<f32>(rng_next(seed), rng_next(seed)),
    );
}

fn safe_inverse_dir(dir: vec3<f32>) -> vec3<f32> {
    let ad = abs(dir);
    return vec3<f32>(
        select(1.0e30, 1.0 / dir.x, ad.x > 1.0e-6),
        select(1.0e30, 1.0 / dir.y, ad.y > 1.0e-6),
        select(1.0e30, 1.0 / dir.z, ad.z > 1.0e-6),
    );
}

fn ray_aabb(ray: Ray, aabb_min: vec3<f32>, aabb_max: vec3<f32>) -> vec2<f32> {
    let inv = safe_inverse_dir(ray.dir);
    let t0 = (aabb_min - ray.origin) * inv;
    let t1 = (aabb_max - ray.origin) * inv;
    let tmin3 = min(t0, t1);
    let tmax3 = max(t0, t1);
    let tmin = max(max(tmin3.x, tmin3.y), max(tmin3.z, 0.0));
    let tmax = min(tmax3.x, min(tmax3.y, tmax3.z));
    return vec2<f32>(tmin, tmax);
}

fn init_t_max(ray: Ray, voxel: vec3<i32>, step: vec3<i32>, inv_dir: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        (f32(voxel.x + select(0, 1, step.x > 0)) - ray.origin.x) * inv_dir.x,
        (f32(voxel.y + select(0, 1, step.y > 0)) - ray.origin.y) * inv_dir.y,
        (f32(voxel.z + select(0, 1, step.z > 0)) - ray.origin.z) * inv_dir.z,
    );
}

fn hash_chunk_coord(coord: vec3<i32>) -> u32 {
    var h = u32(coord.x) * 0x9e3779b9u;
    h = h ^ (u32(coord.y) * 0x85ebca6bu);
    h = h ^ (u32(coord.z) * 0xc2b2ae35u);
    h = h ^ (h >> 16u);
    h = h * 0x7feb352du;
    h = h ^ (h >> 15u);
    h = h * 0x846ca68bu;
    return h ^ (h >> 16u);
}

fn chunk_coord_from_voxel(world_voxel: vec3<i32>) -> vec3<i32> {
    return vec3<i32>(
        world_voxel.x >> 5u,
        world_voxel.y >> 5u,
        world_voxel.z >> 5u,
    );
}

fn lookup_chunk_index(chunk_coord: vec3<i32>) -> i32 {
    let map_size = tracer.chunk_map_info.x;
    if map_size == 0u {
        return -1;
    }

    let map_mask = tracer.chunk_map_info.y;
    let max_probe = max(tracer.chunk_map_info.z, 1u);
    var slot = hash_chunk_coord(chunk_coord) & map_mask;
    var probe: u32 = 0u;

    loop {
        if probe >= max_probe {
            return -1;
        }

        let entry = chunk_map[slot];
        if entry.key_value.w == 0 {
            return -1;
        }

        let resident_probe = entry.probe_meta.y;
        if resident_probe < probe {
            return -1;
        }

        if all(entry.key_value.xyz == chunk_coord) {
            return entry.key_value.w - 1;
        }

        probe = probe + 1u;
        slot = (slot + 1u) & map_mask;
    }

    return -1;
}

fn sample_voxel(
    world_voxel: vec3<i32>,
    cached_chunk_coord: ptr<function, vec3<i32>>,
    cached_chunk_index: ptr<function, i32>,
    cache_valid: ptr<function, u32>,
) -> u32 {
    let chunk_coord = chunk_coord_from_voxel(world_voxel);

    var chunk_index: i32;
    if *cache_valid != 0u && all(*cached_chunk_coord == chunk_coord) {
        chunk_index = *cached_chunk_index;
    } else {
        chunk_index = lookup_chunk_index(chunk_coord);
        *cached_chunk_coord = chunk_coord;
        *cached_chunk_index = chunk_index;
        *cache_valid = 1u;
    }

    if chunk_index < 0 {
        return 0u;
    }

    let chunk_meta = chunk_metas[u32(chunk_index)];
    let local = world_voxel & vec3<i32>(31);
    let linear = (u32(local.x) & 31u) | ((u32(local.y) & 31u) << 5u) | ((u32(local.z) & 31u) << 10u);
    if linear >= chunk_meta.voxel_count {
        return 0u;
    }

    return voxels[chunk_meta.voxel_offset + linear];
}

fn material_albedo(material: u32) -> vec3<f32> {
    switch material {
        case 1u: {
            return vec3<f32>(0.45, 0.39, 0.32);
        }
        case 2u: {
            return vec3<f32>(0.25, 0.55, 0.22);
        }
        case 3u: {
            return vec3<f32>(0.32, 0.34, 0.38);
        }
        default: {
            let palette = vec3<f32>(
                f32((material * 31u) & 255u) / 255.0,
                f32((material * 67u) & 255u) / 255.0,
                f32((material * 97u) & 255u) / 255.0,
            );
            return mix(vec3<f32>(0.2), vec3<f32>(0.9), palette);
        }
    }
}

fn emission_radiance(emissive: u32) -> vec3<f32> {
    let energy = f32(emissive) / 255.0;
    return vec3<f32>(1.0, 0.95, 0.75) * energy * 18.0;
}

fn environment_radiance(dir: vec3<f32>) -> vec3<f32> {
    let up = clamp(dir.y * 0.5 + 0.5, 0.0, 1.0);
    let sky = mix(
        vec3<f32>(0.06, 0.10, 0.18),
        vec3<f32>(0.40, 0.62, 0.95),
        up,
    );
    let sun_dir = normalize(tracer.sun_dir.xyz);
    let sun_disc = pow(max(dot(dir, sun_dir), 0.0), 1200.0) * tracer.integrator.y;
    return sky * tracer.integrator.w + vec3<f32>(sun_disc);
}

fn miss_hit(distance: f32) -> Hit {
    return Hit(false, distance, vec3<f32>(0.0), vec3<f32>(0.0), 0u, 0u);
}

fn trace_dda(ray: Ray, max_steps: u32) -> Hit {
    let world_min = vec3<f32>(tracer.world_min.xyz);
    let world_max = vec3<f32>(tracer.world_max.xyz);
    let range = ray_aabb(ray, world_min, world_max);
    if range.x > range.y {
        return miss_hit(range.y);
    }

    var t = max(range.x, 0.0);
    var start_pos = ray.origin + ray.dir * (t + EPSILON);
    var voxel = vec3<i32>(floor(start_pos));

    let step = vec3<i32>(
        select(-1, 1, ray.dir.x >= 0.0),
        select(-1, 1, ray.dir.y >= 0.0),
        select(-1, 1, ray.dir.z >= 0.0),
    );

    let inv_dir = safe_inverse_dir(ray.dir);
    var t_max = init_t_max(ray, voxel, step, inv_dir);
    let t_delta = abs(inv_dir);
    var cached_chunk_coord = vec3<i32>(0);
    var cached_chunk_index = -1;
    var cache_valid = 0u;

    var normal = vec3<f32>(0.0);
    for (var i: u32 = 0u; i < max_steps; i = i + 1u) {
        let packed = sample_voxel(
            voxel,
            &cached_chunk_coord,
            &cached_chunk_index,
            &cache_valid,
        );
        let material = packed & 0xffu;
        let emissive = (packed >> 8u) & 0xffu;
        if material != 0u || emissive != 0u {
            var hit_normal = normal;
            if all(hit_normal == vec3<f32>(0.0)) {
                hit_normal = -normalize(ray.dir);
            }
            let distance = max(t, 0.0);
            return Hit(
                true,
                distance,
                ray.origin + ray.dir * distance,
                hit_normal,
                material,
                emissive,
            );
        }

        if cached_chunk_index < 0 {
            let chunk_min = vec3<f32>(cached_chunk_coord * CHUNK_SIZE);
            let chunk_max = chunk_min + vec3<f32>(f32(CHUNK_SIZE));
            let chunk_range = ray_aabb(ray, chunk_min, chunk_max);
            let skip_t = chunk_range.y + EPSILON;
            if skip_t > t + EPSILON {
                t = skip_t;
                if t > range.y + EPSILON {
                    break;
                }
                start_pos = ray.origin + ray.dir * t;
                voxel = vec3<i32>(floor(start_pos));
                t_max = init_t_max(ray, voxel, step, inv_dir);
                normal = vec3<f32>(0.0);
                cache_valid = 0u;
                continue;
            }
        }

        let choose_x = t_max.x <= t_max.y && t_max.x <= t_max.z;
        let choose_y = (!choose_x) && t_max.y <= t_max.z;
        let mx = select(0, 1, choose_x);
        let my = select(0, 1, choose_y);
        let mz = 1 - mx - my;
        let axis_mask = vec3<i32>(mx, my, mz);

        normal = -vec3<f32>(
            f32(step.x * axis_mask.x),
            f32(step.y * axis_mask.y),
            f32(step.z * axis_mask.z),
        );
        t = t_max.x * f32(mx) + t_max.y * f32(my) + t_max.z * f32(mz);
        voxel = voxel + step * axis_mask;
        t_max = t_max + t_delta * vec3<f32>(f32(mx), f32(my), f32(mz));

        if t > range.y + EPSILON {
            break;
        }
    }

    return miss_hit(range.y);
}

fn is_occluded(ray: Ray, max_distance: f32) -> bool {
    let shadow = trace_dda(ray, 256u);
    return shadow.hit && shadow.distance < max_distance;
}

fn luminance(color: vec3<f32>) -> f32 {
    return dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn reservoir_empty() -> Reservoir {
    return Reservoir(RESERVOIR_INDEX_MASK, 0.0, 0.0, 0.0);
}

fn gi_reservoir_empty() -> GiReservoir {
    return GiReservoir(0u, 0.0, 0.0, 0u, vec3<f32>(0.0), 0.0, 0u, 0u);
}

fn gi_reservoir_state_empty() -> GiReservoirState {
    return GiReservoirState(vec4<u32>(0u), vec4<f32>(0.0), vec4<f32>(0.0));
}

fn encode_octahedron(dir: vec3<f32>) -> vec2<f32> {
    let inv_l1 = 1.0 / max(abs(dir.x) + abs(dir.y) + abs(dir.z), 1.0e-6);
    var p = dir.xy * inv_l1;
    if dir.z < 0.0 {
        p = (1.0 - abs(p.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), p >= vec2<f32>(0.0));
    }
    return p * 0.5 + 0.5;
}

fn decode_octahedron(encoded: vec2<f32>) -> vec3<f32> {
    var f = encoded * 2.0 - 1.0;
    var n = vec3<f32>(f.x, f.y, 1.0 - abs(f.x) - abs(f.y));
    if n.z < 0.0 {
        let projected =
            (1.0 - abs(n.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), n.xy >= vec2<f32>(0.0));
        n = vec3<f32>(projected.x, projected.y, n.z);
    }
    return normalize(n);
}

fn pack_octahedron(dir: vec3<f32>) -> u32 {
    let encoded = clamp(encode_octahedron(dir), vec2<f32>(0.0), vec2<f32>(1.0));
    let x = u32(encoded.x * 65535.0 + 0.5);
    let y = u32(encoded.y * 65535.0 + 0.5);
    return (y << 16u) | x;
}

fn unpack_octahedron(packed: u32) -> vec3<f32> {
    let x = f32(packed & 0xffffu) * (1.0 / 65535.0);
    let y = f32((packed >> 16u) & 0xffffu) * (1.0 / 65535.0);
    return decode_octahedron(vec2<f32>(x, y));
}

fn encode_weight_hint(weight: f32) -> u32 {
    if weight <= 0.0 {
        return 0u;
    }
    let log2_w = clamp(log2(weight), -20.0, 20.0);
    let normalized = (log2_w + 20.0) * (1.0 / 40.0);
    return u32(clamp(round(normalized * 255.0), 0.0, 255.0));
}

fn decode_weight_hint(encoded: u32) -> f32 {
    let normalized = f32(encoded & 0xffu) * (1.0 / 255.0);
    let log2_w = normalized * 40.0 - 20.0;
    return pow(2.0, log2_w);
}

fn pack_reservoir_index_weight(sample_index: u32, selected_weight: f32) -> u32 {
    let clamped_index = min(sample_index, RESERVOIR_INDEX_MASK - 1u);
    return clamped_index | (encode_weight_hint(selected_weight) << 24u);
}

fn reservoir_index(z_i: u32) -> u32 {
    return z_i & RESERVOIR_INDEX_MASK;
}

fn reservoir_weight_hint(z_i: u32) -> f32 {
    return decode_weight_hint((z_i >> 24u) & 0xffu);
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

fn remap_previous_emitter(index: u32) -> u32 {
    if tracer.emissive_info.z == 0u || index >= tracer.emissive_info.z {
        return INVALID_EMITTER_INDEX;
    }
    let mapped = emissive_remap[index];
    if mapped == INVALID_EMITTER_INDEX || mapped >= tracer.emissive_info.x {
        return INVALID_EMITTER_INDEX;
    }
    return mapped;
}

fn surface_empty() -> SurfaceSample {
    return SurfaceSample(vec4<f32>(0.0, 0.0, 0.0, -1.0), vec4<u32>(0u));
}

fn read_prev_reservoir(pixel_index: u32) -> Reservoir {
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        return reservoir_a[pixel_index];
    }
    return reservoir_b[pixel_index];
}

fn write_curr_reservoir(pixel_index: u32, value: Reservoir) {
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        reservoir_b[pixel_index] = value;
    } else {
        reservoir_a[pixel_index] = value;
    }
}

fn read_curr_reservoir(pixel_index: u32) -> Reservoir {
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        return reservoir_b[pixel_index];
    }
    return reservoir_a[pixel_index];
}

fn write_curr_reservoir_di(pixel_index: u32, value: Reservoir) {
    write_curr_reservoir(pixel_index, value);
}

fn write_curr_reservoir_gi(pixel_index: u32, value: GiReservoir) {
    write_curr_gi_reservoir(pixel_index, value);
}

fn read_prev_gi_reservoir(pixel_index: u32) -> GiReservoir {
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        return gi_from_state(gi_reservoir_a[pixel_index]);
    }
    return gi_from_state(gi_reservoir_b[pixel_index]);
}

fn write_curr_gi_reservoir(pixel_index: u32, value: GiReservoir) {
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        gi_reservoir_b[pixel_index] = gi_to_state(value);
    } else {
        gi_reservoir_a[pixel_index] = gi_to_state(value);
    }
}

fn read_curr_gi_reservoir(pixel_index: u32) -> GiReservoir {
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        return gi_from_state(gi_reservoir_b[pixel_index]);
    }
    return gi_from_state(gi_reservoir_a[pixel_index]);
}

fn importance_at_world_pos(world_pos: vec3<f32>) -> f32 {
    let dims = vec3<u32>(
        max(tracer.importance_info.x, 1u),
        max(tracer.importance_info.y, 1u),
        max(tracer.importance_info.z, 1u),
    );
    let dims_i = vec3<i32>(i32(dims.x), i32(dims.y), i32(dims.z));
    let span_i = vec3<i32>(tracer.world_max.xyz - tracer.world_min.xyz);
    let span = vec3<f32>(
        max(f32(span_i.x), 1.0),
        max(f32(span_i.y), 1.0),
        max(f32(span_i.z), 1.0),
    );
    let rel = clamp(
        (world_pos - vec3<f32>(tracer.world_min.xyz)) / span,
        vec3<f32>(0.0),
        vec3<f32>(0.999999),
    );
    let coord_u = vec3<u32>(rel * vec3<f32>(dims));
    let coord = vec3<i32>(i32(coord_u.x), i32(coord_u.y), i32(coord_u.z));
    let clamped = clamp(coord, vec3<i32>(0), dims_i - vec3<i32>(1));
    return textureLoad(importance_map, clamped, 0).x;
}

fn sample_emissive_from_cdf(u: f32) -> u32 {
    let emitter_count = tracer.emissive_info.x;
    if emitter_count == 0u {
        return 0u;
    }
    let cdf_count = max(tracer.emissive_info.y, 1u);
    let last = min(emitter_count, cdf_count) - 1u;
    var low: u32 = 0u;
    var high: u32 = last;
    let cdf_target = clamp(u, 0.0, 0.99999994);
    loop {
        if low >= high {
            break;
        }
        let mid = (low + high) >> 1u;
        if emissive_cdf[mid] < cdf_target {
            low = mid + 1u;
        } else {
            high = mid;
        }
    }
    return min(low, last);
}

fn emissive_proposal_pdf(index: u32) -> f32 {
    let cdf_count = max(tracer.emissive_info.y, 1u);
    let emitter_count = tracer.emissive_info.x;
    if emitter_count == 0u || cdf_count == 0u {
        return 1.0;
    }
    let idx = min(index, min(emitter_count, cdf_count) - 1u);
    let curr = emissive_cdf[idx];
    let prev = select(0.0, emissive_cdf[idx - 1u], idx > 0u);
    return max(curr - prev, 1.0e-6);
}

fn resolve_reservoir_emitter_index(reservoir: Reservoir) -> u32 {
    let idx = reservoir_index(reservoir.z_i);
    if idx >= tracer.emissive_info.x {
        return INVALID_EMITTER_INDEX;
    }
    return idx;
}

fn selected_reservoir_weight(reservoir: Reservoir) -> f32 {
    return reservoir_weight_hint(reservoir.z_i);
}

fn dissolve_invalid_reservoir(reservoir: ptr<function, Reservoir>) {
    if (*reservoir).m_i < 0.0 || (*reservoir).w_sum < 0.0 || (*reservoir).w_var < 0.0 {
        *reservoir = reservoir_empty();
    }
}

fn reset_curr_gi(pixel_index: u32) {
    write_curr_gi_reservoir(pixel_index, gi_reservoir_empty());
}

fn reset_curr_di(pixel_index: u32) {
    write_curr_reservoir(pixel_index, reservoir_empty());
}

fn read_prev_surface(pixel_index: u32) -> SurfaceSample {
    let pixel_count = tracer.resolution_frame_chunks.x * tracer.resolution_frame_chunks.y;
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        return surface_history[pixel_index];
    }
    return surface_history[pixel_index + pixel_count];
}

fn write_curr_surface(pixel_index: u32, value: SurfaceSample) {
    let pixel_count = tracer.resolution_frame_chunks.x * tracer.resolution_frame_chunks.y;
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        surface_history[pixel_index + pixel_count] = value;
    } else {
        surface_history[pixel_index] = value;
    }
}

fn read_curr_surface(pixel_index: u32) -> SurfaceSample {
    let pixel_count = tracer.resolution_frame_chunks.x * tracer.resolution_frame_chunks.y;
    let even_frame = (tracer.resolution_frame_chunks.z & 1u) == 0u;
    if even_frame {
        return surface_history[pixel_index + pixel_count];
    }
    return surface_history[pixel_index];
}

fn encode_surface_face(hit_normal: vec3<f32>) -> u32 {
    let normal = normalize(hit_normal);
    let abs_normal = abs(normal);
    if abs_normal.x >= abs_normal.y && abs_normal.x >= abs_normal.z {
        return select(0u, 1u, normal.x >= 0.0);
    }
    if abs_normal.y >= abs_normal.z {
        return select(2u, 3u, normal.y >= 0.0);
    }
    return select(4u, 5u, normal.z >= 0.0);
}

fn hash_surface_identity(
    voxel_coord: vec3<i32>,
    chunk_coord: vec3<i32>,
    material_tag: u32,
    face: u32,
) -> u32 {
    let voxel_hash = hash_u32(u32(voxel_coord.x) * 73856093u)
        ^ hash_u32(u32(voxel_coord.y) * 19349663u)
        ^ hash_u32(u32(voxel_coord.z) * 83492791u);
    var id = hash_u32(material_tag ^ (face * 0x9e3779b9u));
    id = hash_u32(id ^ voxel_hash);
    id = hash_u32(id ^ hash_chunk_coord(chunk_coord));
    return id;
}

fn build_surface_sample(
    hit_position: vec3<f32>,
    hit_normal: vec3<f32>,
    material: u32,
    emissive: u32,
) -> SurfaceSample {
    let normal = normalize(hit_normal);
    let depth = dot(
        hit_position - camera.position_lens.xyz,
        camera.forward_fov.xyz,
    );
    if depth <= 0.0 {
        return surface_empty();
    }
    let voxel_coord = vec3<i32>(floor(hit_position - normal * 0.5));
    let chunk_coord = chunk_coord_from_voxel(voxel_coord);
    let material_tag = (material & 0xffu) | ((emissive & 0xffu) << 8u);
    let face = encode_surface_face(normal);
    let surface_id = hash_surface_identity(voxel_coord, chunk_coord, material_tag, face);
    return SurfaceSample(
        vec4<f32>(normal, depth),
        vec4<u32>(surface_id, material_tag, face, 0u),
    );
}

fn surface_reuse_compatible(
    hit_position: vec3<f32>,
    hit_normal: vec3<f32>,
    sample: SurfaceSample,
) -> bool {
    if sample.normal_material.w < 0.0 {
        return false;
    }

    let normal_delta = dot(normalize(hit_normal), normalize(sample.normal_material.xyz));
    if normal_delta < 0.85 {
        return false;
    }

    let expected_prev_depth = dot(
        hit_position - previous_camera.position_lens.xyz,
        previous_camera.forward_fov.xyz,
    );
    if expected_prev_depth <= 0.0 {
        return false;
    }

    let depth_delta = abs(sample.normal_material.w - expected_prev_depth);
    let depth_eps = max(0.2, expected_prev_depth * 0.04);
    return depth_delta <= depth_eps;
}

fn spatial_similarity_jacobian(
    hit_position: vec3<f32>,
    hit_normal: vec3<f32>,
    prev_surface: SurfaceSample,
) -> f32 {
    if prev_surface.normal_material.w < 0.0 {
        return 0.0;
    }

    let prev_normal = normalize(prev_surface.normal_material.xyz);
    let curr_normal = normalize(hit_normal);
    let normal_similarity = dot(curr_normal, prev_normal);
    if normal_similarity < 0.9 {
        return 0.0;
    }

    let expected_prev_depth = dot(
        hit_position - previous_camera.position_lens.xyz,
        previous_camera.forward_fov.xyz,
    );
    if expected_prev_depth <= 0.0 {
        return 0.0;
    }

    let observed_prev_depth = prev_surface.normal_material.w;
    let depth_delta = abs(observed_prev_depth - expected_prev_depth);
    let depth_threshold = max(0.06, expected_prev_depth * 0.015);
    if depth_delta > depth_threshold {
        return 0.0;
    }

    let depth_ratio = observed_prev_depth / max(expected_prev_depth, 1.0e-4);
    let depth_jacobian = clamp(depth_ratio * depth_ratio, 0.25, 4.0);
    return normal_similarity * depth_jacobian;
}

fn gi_reservoir_update(
    reservoir: ptr<function, GiReservoir>,
    direction: vec3<f32>,
    sample_pos: vec3<f32>,
    sample_li: f32,
    sample_normal_packed: u32,
    sample_is_hit: u32,
    weight: f32,
    p_hat: f32,
    m_add: u32,
    seed: ptr<function, u32>,
) {
    if weight <= 0.0 || p_hat <= 0.0 || m_add == 0u {
        return;
    }

    let new_w_sum = (*reservoir).w_sum + weight;
    let pick = rng_next(seed) < (weight / max(new_w_sum, 1.0e-6));
    (*reservoir).w_sum = new_w_sum;
    (*reservoir).m = (*reservoir).m + m_add;
    if pick {
        (*reservoir).dir_packed = pack_octahedron(direction);
        (*reservoir).sample_pos = sample_pos;
        (*reservoir).sample_li = sample_li;
        (*reservoir).sample_normal_packed = sample_normal_packed;
        (*reservoir).sample_is_hit = sample_is_hit;
        (*reservoir).p_hat = p_hat;
    }
}

fn pixel_coord_from_index(pixel_index: u32, width: u32) -> vec2<u32> {
    return vec2<u32>(pixel_index % width, pixel_index / width);
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

fn gi_reconnection_jacobian(
    prev_shading: vec3<f32>,
    curr_shading: vec3<f32>,
    sample_pos: vec3<f32>,
    sample_normal: vec3<f32>,
) -> f32 {
    let to_prev = prev_shading - sample_pos;
    let to_curr = curr_shading - sample_pos;
    let dist_prev_sq = max(dot(to_prev, to_prev), 1.0e-6);
    let dist_curr_sq = max(dot(to_curr, to_curr), 1.0e-6);
    let g_prev = abs(dot(sample_normal, normalize(to_prev))) / dist_prev_sq;
    let g_curr = abs(dot(sample_normal, normalize(to_curr))) / dist_curr_sq;
    if g_prev <= 1.0e-6 || g_curr <= 0.0 {
        return 0.0;
    }
    let jacobian_min = clamp(min(tracer.tuning_c.z, tracer.tuning_c.w), 0.01, 1.0);
    let jacobian_max = max(max(tracer.tuning_c.z, tracer.tuning_c.w), jacobian_min + 1.0e-3);
    return clamp(g_curr / g_prev, jacobian_min, jacobian_max);
}

fn estimate_secondary_luminance(secondary_hit: Hit, max_steps: u32) -> f32 {
    var li = emission_radiance(secondary_hit.emissive);
    let sun_dir = normalize(tracer.sun_dir.xyz);
    let n_dot_s = max(dot(secondary_hit.normal, sun_dir), 0.0);
    if n_dot_s > 0.0 {
        let shadow_ray = Ray(secondary_hit.position + secondary_hit.normal * (EPSILON * 6.0), sun_dir);
        if !is_occluded(shadow_ray, 512.0) {
            li = li + vec3<f32>(tracer.integrator.y) * n_dot_s * 0.35;
        }
    }

    let escape_dir = normalize(secondary_hit.normal);
    let escape_ray = Ray(secondary_hit.position + secondary_hit.normal * (EPSILON * 6.0), escape_dir);
    let escape_hit = trace_dda(escape_ray, max_steps);
    if escape_hit.hit {
        li = li + emission_radiance(escape_hit.emissive) * 0.35;
    } else {
        li = li + environment_radiance(escape_dir) * 0.35;
    }
    li = li + environment_radiance(secondary_hit.normal) * 0.2;
    return max(luminance(li), 1.0e-5);
}

fn attempt_prev_gi_reuse(
    reservoir: ptr<function, GiReservoir>,
    prev_pixel_index: u32,
    hit_position: vec3<f32>,
    hit_normal: vec3<f32>,
    reuse_scale: f32,
    seed: ptr<function, u32>,
) {
    if reuse_scale <= 0.0 {
        return;
    }

    let prev_surface = read_prev_surface(prev_pixel_index);
    let spatial_jacobian = spatial_similarity_jacobian(hit_position, hit_normal, prev_surface);
    if spatial_jacobian <= 0.0 {
        return;
    }

    let prev = read_prev_gi_reservoir(prev_pixel_index);
    if prev.m == 0u || prev.p_hat <= 0.0 {
        return;
    }

    let prev_li = max(prev.sample_li, 1.0e-5);
    let prev_direction = unpack_octahedron(prev.dir_packed);
    var direction = prev_direction;
    var jacobian = spatial_jacobian;
    if prev.sample_is_hit != 0u {
        let sample_pos = prev.sample_pos;
        let sample_normal = unpack_octahedron(prev.sample_normal_packed);
        let to_sample = sample_pos - hit_position;
        let dist_sq = dot(to_sample, to_sample);
        if dist_sq <= 1.0e-5 {
            return;
        }
        direction = normalize(to_sample);
        if dot(sample_normal, -direction) <= 0.02 {
            return;
        }
        let n_dot_l = max(dot(hit_normal, direction), 0.0);
        if n_dot_l <= 0.0 {
            return;
        }
        let dist = sqrt(dist_sq);
        let vis_ray = Ray(hit_position + hit_normal * (EPSILON * 6.0), direction);
        if is_occluded(vis_ray, dist - EPSILON * 10.0) {
            return;
        }
        let prev_surface_depth = prev_surface.normal_material.w;
        let prev_pixel = pixel_coord_from_index(prev_pixel_index, tracer.resolution_frame_chunks.x);
        let prev_shading = reconstruct_camera_space_position(prev_pixel, prev_surface_depth, previous_camera);
        let reprojection_error = length(prev_shading - hit_position);
        let reprojection_limit = max(0.08, prev_surface_depth * 0.02);
        if reprojection_error > reprojection_limit {
            return;
        }
        jacobian = jacobian * gi_reconnection_jacobian(
            prev_shading,
            hit_position,
            sample_pos,
            sample_normal,
        );
        if jacobian <= 0.0 {
            return;
        }
    }

    let direction_gate = clamp(tracer.tuning_b.w, -0.25, 0.99);
    if dot(direction, prev_direction) < direction_gate {
        return;
    }

    let p_hat = max(dot(hit_normal, direction), 0.0) * prev_li;
    if p_hat <= 0.0 {
        return;
    }

    let reuse_m_cap = max(1u, u32(max(tracer.tuning_c.x, 1.0)));
    let reuse_weight_cap = max(tracer.tuning_c.y, 1.0);
    var temporal_boost = max(tracer.tuning_b.x, 0.0) * reuse_scale;
    if tracer.debug_map_stats.w > 0.5 {
        temporal_boost = temporal_boost * 0.35;
    }
    if temporal_boost <= 0.0 {
        return;
    }

    let effective_prev_m = min(prev.w_sum / max(prev.p_hat, 1.0e-6), f32(reuse_m_cap));
    var reused_weight = effective_prev_m * p_hat * temporal_boost * jacobian;
    reused_weight = min(reused_weight, p_hat * reuse_weight_cap);
    if reused_weight <= 0.0 {
        return;
    }
    let reused_m = min(max(1u, prev.m), reuse_m_cap);
    gi_reservoir_update(
        reservoir,
        direction,
        prev.sample_pos,
        prev.sample_li,
        prev.sample_normal_packed,
        prev.sample_is_hit,
        reused_weight,
        p_hat,
        reused_m,
        seed,
    );
}

fn restir_gi_select_direction(
    pixel_index: u32,
    hit_position: vec3<f32>,
    hit_normal: vec3<f32>,
    seed: ptr<function, u32>,
    out_reservoir: ptr<function, GiReservoir>,
) -> vec3<f32> {
    if tracer.flags.z == 0u {
        *out_reservoir = gi_reservoir_empty();
        return sample_isotropic_hemisphere(hit_normal, seed);
    }

    var reservoir = gi_reservoir_empty();
    let frame = tracer.resolution_frame_chunks.z;
    let pixel = pixel_coord_from_index(pixel_index, tracer.resolution_frame_chunks.x);
    let candidate_u = permuted_sample_2d(pixel, frame, 2u);
    let candidate = sample_isotropic_hemisphere_from_u(hit_normal, candidate_u);
    let secondary_ray = Ray(hit_position + hit_normal * (EPSILON * 6.0), candidate);
    let secondary_hit = trace_dda(secondary_ray, clamp(u32(tracer.tuning_b.z), 32u, MAX_DDA_STEPS_LIMIT));
    var sample_pos = vec3<f32>(0.0);
    var sample_normal_packed = 0u;
    var sample_is_hit = 0u;
    var candidate_li = max(luminance(environment_radiance(candidate)), 1.0e-5);
    if secondary_hit.hit {
        sample_pos = secondary_hit.position;
        sample_normal_packed = pack_octahedron(normalize(secondary_hit.normal));
        sample_is_hit = 1u;
        candidate_li = estimate_secondary_luminance(secondary_hit, clamp(u32(tracer.tuning_b.z), 32u, MAX_DDA_STEPS_LIMIT));
    }

    let candidate_p_hat = max(dot(hit_normal, candidate), 0.0) * candidate_li;
    if candidate_p_hat > 0.0 {
        let pdf = 1.0 / (2.0 * PI);
        let weight = candidate_p_hat / pdf;
        gi_reservoir_update(
            &reservoir,
            candidate,
            sample_pos,
            candidate_li,
            sample_normal_packed,
            sample_is_hit,
            weight,
            candidate_p_hat,
            1u,
            seed,
        );
    }

    *out_reservoir = reservoir;
    if reservoir.m == 0u {
        return candidate;
    }
    if reservoir.p_hat <= 0.0 {
        return candidate;
    }

    let selected = unpack_octahedron(reservoir.dir_packed);
    if dot(selected, hit_normal) <= 0.0 {
        return candidate;
    }
    // Energy-conserving ReSTIR normalization, consumed by the dedicated ReSTIR pass.
    let _normalization =
        reservoir.w_sum / max(f32(max(1u, reservoir.m)) * reservoir.p_hat, 1.0e-6);
    return selected;
}

fn in_bounds(pixel: vec2<i32>, resolution: vec2<i32>) -> bool {
    return all(pixel >= vec2<i32>(0)) && all(pixel < resolution);
}

fn pixel_index_of(pixel: vec2<i32>, width: u32) -> u32 {
    return u32(pixel.y) * width + u32(pixel.x);
}

fn reproject_previous_pixel(world_pos: vec3<f32>, resolution: vec2<i32>) -> vec2<i32> {
    let to_point = world_pos - previous_camera.position_lens.xyz;
    let z = dot(to_point, previous_camera.forward_fov.xyz);
    if z <= previous_camera.clip_depth.x {
        return vec2<i32>(-1);
    }

    let tan_half_fov = tan(0.5 * previous_camera.forward_fov.w);
    let x = dot(to_point, previous_camera.right_aspect.xyz);
    let y = dot(to_point, previous_camera.up_focus.xyz);
    let ndc = vec2<f32>(
        x / (z * tan_half_fov * previous_camera.right_aspect.w),
        y / (z * tan_half_fov),
    );
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec2<i32>(-1);
    }

    let previous_width = max(f32(previous_camera.resolution_frame.x), 1.0);
    let previous_height = max(f32(previous_camera.resolution_frame.y), 1.0);
    let px = i32(floor(uv.x * previous_width));
    let py = i32(floor(uv.y * previous_height));
    let pixel = vec2<i32>(px, py);
    if !in_bounds(pixel, resolution) {
        return vec2<i32>(-1);
    }
    return pixel;
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

    let color = vec3<f32>(1.0, 0.95, 0.75) * emitter.w * (n_dot_l / dist_sq);
    return max(luminance(color), 0.0);
}

fn evaluate_emitter_visible(hit_position: vec3<f32>, hit_normal: vec3<f32>, emitter_index: u32) -> vec3<f32> {
    let emitter = emissive_voxels[emitter_index].position_power;
    let to_light = emitter.xyz - hit_position;
    let dist_sq = max(dot(to_light, to_light), 1.0e-4);
    let inv_dist = inverseSqrt(dist_sq);
    let light_dir = to_light * inv_dist;
    let n_dot_l = max(dot(hit_normal, light_dir), 0.0);
    if n_dot_l <= 0.0 {
        return vec3<f32>(0.0);
    }

    let light_distance = sqrt(dist_sq);
    let shadow_origin = hit_position + hit_normal * (EPSILON * 4.0);
    let shadow_ray = Ray(shadow_origin, light_dir);
    if is_occluded(shadow_ray, light_distance - EPSILON * 8.0) {
        return vec3<f32>(0.0);
    }

    return vec3<f32>(1.0, 0.95, 0.75) * emitter.w * (n_dot_l / dist_sq);
}

fn reservoir_update(
    reservoir: ptr<function, Reservoir>,
    sample_index: u32,
    weight: f32,
    p_hat: f32,
    m_add: f32,
    seed: ptr<function, u32>,
) {
    if weight <= 0.0 || p_hat <= 0.0 || m_add <= 0.0 {
        return;
    }

    let new_w_sum = (*reservoir).w_sum + weight;
    let pick = rng_next(seed) < (weight / max(new_w_sum, 1.0e-6));
    (*reservoir).w_sum = new_w_sum;
    (*reservoir).m_i = (*reservoir).m_i + m_add;

    if pick {
        (*reservoir).z_i = pack_reservoir_index_weight(sample_index, weight);
        (*reservoir).w_var = 1.0 / max(p_hat, 1.0e-6);
    }
    dissolve_invalid_reservoir(reservoir);
}

fn attempt_prev_reuse(
    reservoir: ptr<function, Reservoir>,
    prev_pixel_index: u32,
    hit_position: vec3<f32>,
    hit_normal: vec3<f32>,
    emitter_count: u32,
    seed: ptr<function, u32>,
) {
    let prev_surface = read_prev_surface(prev_pixel_index);
    let jacobian = spatial_similarity_jacobian(hit_position, hit_normal, prev_surface);
    if jacobian <= 0.0 {
        return;
    }

    let prev = read_prev_reservoir(prev_pixel_index);
    if prev.m_i <= 0.0 || prev.w_sum <= 0.0 || prev.w_var <= 0.0 {
        return;
    }

    let prev_sample_index = reservoir_index(prev.z_i);
    if prev_sample_index >= emitter_count {
        return;
    }
    let mapped_index = remap_previous_emitter(prev_sample_index);
    if mapped_index == INVALID_EMITTER_INDEX || mapped_index >= emitter_count {
        return;
    }

    let p_hat_prev = emitter_unshadowed_target(hit_position, hit_normal, mapped_index);
    if p_hat_prev <= 0.0 {
        return;
    }

    let reuse_m_cap = max(tracer.tuning_c.x, 1.0);
    let temporal_boost = max(tracer.tuning_b.x, 0.0);
    let history_m = min(prev.m_i, reuse_m_cap);
    let talbot_mis = 1.0 / (1.0 + history_m * 0.5);
    let reuse_weight_cap = max(tracer.tuning_c.y, 1.0);
    var reused_weight = prev.w_sum * p_hat_prev * prev.w_var * temporal_boost * jacobian * talbot_mis;
    reused_weight = min(reused_weight, p_hat_prev * reuse_weight_cap);
    if reused_weight <= 0.0 {
        return;
    }

    let reused_m = clamp(prev.m_i, 1.0, reuse_m_cap);
    reservoir_update(
        reservoir,
        mapped_index,
        reused_weight,
        p_hat_prev,
        reused_m,
        seed,
    );
}

fn restir_emissive_direct(
    pixel_index: u32,
    hit_position: vec3<f32>,
    hit_normal: vec3<f32>,
    seed: ptr<function, u32>,
) -> vec3<f32> {
    if tracer.flags.x == 0u {
        write_curr_reservoir_di(pixel_index, reservoir_empty());
        return sample_emissive_nee(hit_position, hit_normal, seed);
    }

    let emitter_count = tracer.emissive_info.x;
    if emitter_count == 0u {
        write_curr_reservoir_di(pixel_index, reservoir_empty());
        return vec3<f32>(0.0);
    }

    // Trace pass only performs initial candidate generation.
    var reservoir = reservoir_empty();
    let local_importance = importance_at_world_pos(hit_position);
    let proposal_count = select(1u, 2u, local_importance > 0.55);
    for (var proposal_iter: u32 = 0u; proposal_iter < 2u; proposal_iter = proposal_iter + 1u) {
        if proposal_iter >= proposal_count {
            break;
        }
        let u = fract(rng_next(seed) + local_importance * 0.37 + f32(proposal_iter) * 0.5);
        let proposal = sample_emissive_from_cdf(u);
        let p_hat = emitter_unshadowed_target(hit_position, hit_normal, proposal);
        if p_hat <= 0.0 {
            continue;
        }
        let pdf = emissive_proposal_pdf(proposal);
        let weight = p_hat / max(pdf, 1.0e-6);
        reservoir_update(&reservoir, proposal, weight, p_hat, 1.0, seed);
    }

    write_curr_reservoir_di(pixel_index, reservoir);
    if reservoir.m_i <= 0.0 || reservoir.w_var <= 0.0 || reservoir.w_sum <= 0.0 {
        return vec3<f32>(0.0);
    }

    let selected_index = resolve_reservoir_emitter_index(reservoir);
    if selected_index == INVALID_EMITTER_INDEX {
        return vec3<f32>(0.0);
    }
    let selected_radiance = evaluate_emitter_visible(hit_position, hit_normal, selected_index);
    if all(selected_radiance == vec3<f32>(0.0)) {
        return vec3<f32>(0.0);
    }

    let p_hat_selected = max(1.0 / max(reservoir.w_var, 1.0e-6), 1.0e-6);
    let normalization = reservoir.w_sum / max(reservoir.m_i * p_hat_selected, 1.0e-6);
    return selected_radiance * max(normalization, 0.0);
}

fn sample_emissive_nee(hit_position: vec3<f32>, hit_normal: vec3<f32>, seed: ptr<function, u32>) -> vec3<f32> {
    let emitter_count = tracer.emissive_info.x;
    if emitter_count == 0u {
        return vec3<f32>(0.0);
    }

    let guided_u = fract(rng_next(seed) + importance_at_world_pos(hit_position) * 0.37);
    let pick = sample_emissive_from_cdf(guided_u);
    let emitter = emissive_voxels[pick].position_power;
    let to_light = emitter.xyz - hit_position;
    let dist_sq = max(dot(to_light, to_light), 1.0e-4);
    let inv_dist = inverseSqrt(dist_sq);
    let light_dir = to_light * inv_dist;
    let n_dot_l = max(dot(hit_normal, light_dir), 0.0);
    if n_dot_l <= 0.0 {
        return vec3<f32>(0.0);
    }

    let light_distance = sqrt(dist_sq);
    let shadow_origin = hit_position + hit_normal * (EPSILON * 4.0);
    let shadow_ray = Ray(shadow_origin, light_dir);
    if is_occluded(shadow_ray, light_distance - EPSILON * 8.0) {
        return vec3<f32>(0.0);
    }

    let emission = vec3<f32>(1.0, 0.95, 0.75) * emitter.w;
    let pdf = emissive_proposal_pdf(pick);
    let geometric = n_dot_l / dist_sq;
    return emission * geometric / max(pdf, 1.0e-6);
}

fn generate_primary_ray(pixel: vec2<u32>) -> Ray {
    let frame = tracer.resolution_frame_chunks.z;
    let resolution = vec2<f32>(
        f32(tracer.resolution_frame_chunks.x),
        f32(tracer.resolution_frame_chunks.y),
    );
    let jitter = permuted_sample_2d(pixel, frame, 0u) - vec2<f32>(0.5);
    let uv = (vec2<f32>(vec2<i32>(pixel)) + jitter) / resolution;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);

    let tan_half_fov = tan(0.5 * camera.forward_fov.w);
    let fov_corrected = vec2<f32>(ndc.x * camera.right_aspect.w, ndc.y);
    var direction = normalize(
        camera.forward_fov.xyz
            + camera.right_aspect.xyz * fov_corrected.x * tan_half_fov
            + camera.up_focus.xyz * fov_corrected.y * tan_half_fov,
    );

    let depth_adapt = 1.0 + camera.clip_depth.z * length(ndc);
    let focal_distance = max(camera.up_focus.w * depth_adapt, camera.clip_depth.x);
    let focal_point = camera.position_lens.xyz + direction * focal_distance;

    let lens_sample = sample_disk_from_u(permuted_sample_2d(pixel, frame, 1u)) * camera.position_lens.w;
    let lens_offset = camera.right_aspect.xyz * lens_sample.x + camera.up_focus.xyz * lens_sample.y;
    let ray_origin = camera.position_lens.xyz + lens_offset;
    direction = normalize(focal_point - ray_origin);

    return Ray(ray_origin, direction);
}

fn integrate_path(pixel: vec2<u32>, pixel_index: u32, seed: ptr<function, u32>) -> vec3<f32> {
    var ray = generate_primary_ray(pixel);
    var throughput = vec3<f32>(1.0);
    var radiance = vec3<f32>(0.0);

    let max_bounces = max(1u, u32(tracer.integrator.x));
    let max_dda_steps = clamp(u32(tracer.tuning_b.z), 32u, MAX_DDA_STEPS_LIMIT);
    let rr_start = max(1u, u32(tracer.tuning_a.y));
    let rr_min = clamp(tracer.tuning_a.z, 0.01, 0.99);
    let rr_max = clamp(tracer.tuning_a.w, rr_min, 0.995);
    let sun_dir = normalize(tracer.sun_dir.xyz);

    for (var bounce: u32 = 0u; bounce < max_bounces; bounce = bounce + 1u) {
        let hit = trace_dda(ray, max_dda_steps);
        if !hit.hit {
            radiance = radiance + throughput * environment_radiance(ray.dir);
            break;
        }

        let albedo = material_albedo(hit.material);
        let emission = emission_radiance(hit.emissive);
        radiance = radiance + throughput * emission;

        let n_dot_l = max(dot(hit.normal, sun_dir), 0.0);
        if n_dot_l > 0.0 {
            let shadow_ray = Ray(hit.position + hit.normal * (EPSILON * 4.0), sun_dir);
            if !is_occluded(shadow_ray, 512.0) {
                let direct = vec3<f32>(tracer.integrator.y) * n_dot_l;
                radiance = radiance + throughput * albedo * direct;
            }
        }

        if tracer.emissive_info.x > 0u {
            if bounce == 0u {
                let emissive_direct = restir_emissive_direct(
                    pixel_index,
                    hit.position,
                    hit.normal,
                    seed,
                );
                radiance = radiance + throughput * albedo * emissive_direct;
            } else {
                let emissive_direct = sample_emissive_nee(hit.position, hit.normal, seed);
                radiance = radiance + throughput * albedo * emissive_direct;
            }
        }

        var next_dir = sample_isotropic_hemisphere(hit.normal, seed);
        if bounce == 0u {
            var gi_reservoir = gi_reservoir_empty();
            next_dir = restir_gi_select_direction(
                pixel_index,
                hit.position,
                hit.normal,
                seed,
                &gi_reservoir,
            );
            write_curr_surface(
                pixel_index,
                build_surface_sample(hit.position, hit.normal, hit.material, hit.emissive),
            );
            write_curr_reservoir_gi(pixel_index, gi_reservoir);
        }

        throughput = throughput * albedo;

        if bounce >= rr_start {
            let rr = clamp(max(max(throughput.x, throughput.y), throughput.z), rr_min, rr_max);
            if rng_next(seed) > rr {
                break;
            }
            throughput = throughput / rr;
        }

        ray = Ray(hit.position + hit.normal * (EPSILON * 6.0), next_dir);
    }

    return radiance;
}

fn aces_tonemap(color: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((color * (a * color + b)) / (color * (c * color + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn to_display_referred(radiance: vec3<f32>) -> vec3<f32> {
    let exposure = max(tracer.integrator.z, 1.0e-4);
    let white_balance = vec3<f32>(1.03, 0.97, 1.02);
    let cd_per_m2 = radiance * exposure * 0.31830988618 * white_balance;
    let mapped = aces_tonemap(cd_per_m2);
    return pow(mapped, vec3<f32>(1.0 / 2.2));
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
        return mix(dim, vec3<f32>(0.20, 0.90, 0.35), select(0.0, 1.0, x_norm <= avg_norm));
    }
    if pixel.y == 1u {
        return mix(dim, vec3<f32>(0.95, 0.56, 0.18), select(0.0, 1.0, x_norm <= observed_max_norm));
    }
    if pixel.y == 2u {
        return mix(dim, vec3<f32>(0.25, 0.62, 0.98), select(0.0, 1.0, x_norm <= load_norm));
    }

    let observed = select(0.0, 1.0, x_norm <= observed_max_norm);
    let budget = select(0.0, 1.0, x_norm <= budget_norm);
    return vec3<f32>(budget, observed, observed);
}

@compute @workgroup_size(8, 8, 1)
fn main_cs(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = tracer.resolution_frame_chunks.x;
    let height = tracer.resolution_frame_chunks.y;
    if gid.x >= width || gid.y >= height {
        return;
    }

    let pixel_index = gid.y * width + gid.x;
    reset_curr_di(pixel_index);
    reset_curr_gi(pixel_index);
    write_curr_surface(pixel_index, surface_empty());

    var seed = init_seed(gid.xy, tracer.resolution_frame_chunks.z);
    let sample = integrate_path(gid.xy, pixel_index, &seed);

    let reset = tracer.resolution_frame_chunks.z == 0u;
    let max_history = clamp(tracer.tuning_a.x, 1.0, 256.0);
    let previous = accumulation[pixel_index];
    let previous_weight = select(min(previous.w, max_history), 0.0, reset);
    let current_weight = previous_weight + 1.0;
    let accumulated = (previous.rgb * previous_weight + sample) / current_weight;
    accumulation[pixel_index] = vec4<f32>(accumulated, current_weight);

    var color = to_display_referred(accumulated);
    if tracer.flags.y == OVERLAY_MODE_PROBE {
        color = overlay_probe_debug(gid.xy, width, color);
    }
    textureStore(output_tex, vec2<i32>(vec2<u32>(gid.xy)), vec4<f32>(color, 1.0));
}
