//! Phase 3 gap-closure tests.
//!
//! `run_frame` uses `OnceLock` global state, so runtime-oriented tests in this
//! file must reserve unique frame ranges. Phase 3 gap closure reserves
//! `3100..3199`.

use std::{sync::mpsc, time::Duration};

use ash::vk;
use revoxelation::{
    meshing::{
        ChunkNeighborSet, GreedyQuad, MeshDirtyRecord, PackedMesh, PackedVertex,
        build_greedy_mesh, pack_quad, pack_vertex,
    },
    renderer::{
        chunk_pool::SlotAllocator,
        instance::{
            DEBUG_UTILS_EXTENSION_NAME, VALIDATION_LAYER_NAME, resolve_debug_instance_config,
        },
        mesh_pipeline::metadata_descriptor_layout_binding,
        spirv::decode_spirv_words,
    },
    streaming::{
        job_queue::PrioritizedTask,
        job_runner::spawn_chunk_job,
        types::{CHUNK_EDGE, CHUNK_VOXEL_COUNT, ChunkJobOutcome, ChunkKey, ChunkVoxels},
    },
};

fn chunk_key(x: i32, y: i32, z: i32, lod_level: u8) -> ChunkKey {
    ChunkKey::new(x, y, z, lod_level)
}

fn prioritized_task(key: ChunkKey) -> PrioritizedTask {
    PrioritizedTask::new(key, 0, 1.0)
}

fn fresh_pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .expect("rayon test pool should build")
}

fn sample_packed_mesh(index_count: usize, quad_count: u32) -> PackedMesh {
    let vertex_count = (index_count / 6) * 4;
    PackedMesh {
        vertices: vec![PackedVertex([0, 0]); vertex_count].into_boxed_slice(),
        indices: (0..index_count as u32).collect::<Vec<_>>().into_boxed_slice(),
        quad_count,
        aabb_min: [1.0, 2.0, 3.0],
        aabb_max: [4.0, 5.0, 6.0],
    }
}

fn recv_generated_payload(
    rx: &mpsc::Receiver<revoxelation::streaming::types::ChunkJobResult>,
) -> Box<[u8]> {
    let result = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("chunk job should finish within 2s");
    match result.outcome {
        ChunkJobOutcome::Generated(voxels) => voxels.block_ids,
        other => panic!("expected generated chunk payload, got {other:?}"),
    }
}

#[test]
fn mesh_03_job_runner_emits_deterministic_non_empty_chunk_payload() {
    let pool = fresh_pool();
    let key = chunk_key(3, -2, 5, 1);
    let (tx, rx) = mpsc::channel();

    let _first = spawn_chunk_job(&pool, prioritized_task(key), tx.clone());
    let _second = spawn_chunk_job(&pool, prioritized_task(key), tx);

    let first = recv_generated_payload(&rx);
    let second = recv_generated_payload(&rx);

    assert_eq!(first.len(), CHUNK_VOXEL_COUNT);
    assert_eq!(second.len(), CHUNK_VOXEL_COUNT);
    assert_eq!(first, second, "same chunk key should produce the same payload");
    assert!(
        first.iter().any(|block_id| *block_id != 0),
        "generated payload should contain at least one non-air voxel"
    );
}

#[test]
fn mesh_03_dense_draw_list_swap_removes_sparse_slot_holes() {
    let mut allocator = SlotAllocator::with_capacity(4);
    let first = chunk_key(0, 0, 0, 0);
    let second = chunk_key(1, 0, 0, 0);
    let third = chunk_key(2, 0, 0, 0);

    allocator
        .prepare_upload(first, &sample_packed_mesh(6, 1))
        .expect("first upload should succeed");
    allocator
        .prepare_upload(second, &sample_packed_mesh(12, 2))
        .expect("second upload should succeed");
    allocator
        .prepare_upload(third, &sample_packed_mesh(18, 3))
        .expect("third upload should succeed");

    assert_eq!(allocator.active_draw_count(), 3);
    assert_eq!(allocator.draw_slots_shadow(), &[0, 1, 2]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(1), Some(1));
    assert_eq!(allocator.draw_index_for_slot(2), Some(2));

    let third_slot_before_remove = allocator
        .slot_for(third)
        .expect("third chunk should have a stable slot");
    let third_metadata_before_remove = allocator.metadata_shadow()[third_slot_before_remove as usize];

    let removed_slot = allocator
        .prepare_remove(second)
        .expect("removing a live chunk should free its stable slot");
    assert_eq!(removed_slot, 1);

    assert_eq!(allocator.active_chunk_count(), 2);
    assert_eq!(allocator.active_draw_count(), 2);
    assert_eq!(allocator.draw_slots_shadow(), &[0, 2]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(1), None);
    assert_eq!(allocator.draw_index_for_slot(2), Some(1));
    assert_eq!(
        allocator.metadata_shadow()[third_slot_before_remove as usize],
        third_metadata_before_remove,
        "stable-slot storage should remain keyed by slot id even when draw order compacts"
    );
}

#[test]
fn mesh_03_dense_draw_list_reuse_keeps_other_draw_indices_stable() {
    let mut allocator = SlotAllocator::with_capacity(4);
    let first = chunk_key(0, 0, 0, 0);
    let second = chunk_key(1, 0, 0, 0);
    let third = chunk_key(2, 0, 0, 0);
    let reused = chunk_key(9, 0, 0, 0);

    allocator
        .prepare_upload(first, &sample_packed_mesh(6, 1))
        .expect("first upload should succeed");
    allocator
        .prepare_upload(second, &sample_packed_mesh(12, 2))
        .expect("second upload should succeed");
    allocator
        .prepare_upload(third, &sample_packed_mesh(18, 3))
        .expect("third upload should succeed");

    allocator
        .prepare_remove(second)
        .expect("removing a middle slot should succeed");
    assert_eq!(allocator.draw_slots_shadow(), &[0, 2]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(2), Some(1));

    let reused_upload = allocator
        .prepare_upload(reused, &sample_packed_mesh(24, 4))
        .expect("reused upload should succeed");

    assert_eq!(reused_upload.slot_id, 1, "the freed stable slot should be reused");
    assert_eq!(allocator.active_chunk_count(), 3);
    assert_eq!(allocator.active_draw_count(), 3);
    assert_eq!(allocator.draw_slots_shadow(), &[0, 2, 1]);
    assert_eq!(allocator.draw_index_for_slot(0), Some(0));
    assert_eq!(allocator.draw_index_for_slot(2), Some(1));
    assert_eq!(allocator.draw_index_for_slot(1), Some(2));
}

#[test]
fn mesh_03_chunk_metadata_world_origin_matches_chunk_key() {
    let mut allocator = SlotAllocator::with_capacity(2);
    let key = chunk_key(-2, 1, 3, 2);
    let mesh = sample_packed_mesh(12, 2);

    let upload = allocator
        .prepare_upload(key, &mesh)
        .expect("upload should compute chunk metadata");

    let lod_scale = (1_u32 << key.lod_level) as f32;
    let chunk_world_edge = CHUNK_EDGE as f32 * lod_scale;
    let expected_origin = [
        key.x as f32 * chunk_world_edge,
        key.y as f32 * chunk_world_edge,
        key.z as f32 * chunk_world_edge,
    ];

    assert_eq!(upload.metadata.chunk_origin, expected_origin);
    assert_eq!(
        upload.metadata.aabb_min,
        [
            expected_origin[0] + mesh.aabb_min[0] * lod_scale,
            expected_origin[1] + mesh.aabb_min[1] * lod_scale,
            expected_origin[2] + mesh.aabb_min[2] * lod_scale,
        ]
    );
    assert_eq!(
        upload.metadata.aabb_max,
        [
            expected_origin[0] + mesh.aabb_max[0] * lod_scale,
            expected_origin[1] + mesh.aabb_max[1] * lod_scale,
            expected_origin[2] + mesh.aabb_max[2] * lod_scale,
        ]
    );
    assert_eq!(
        allocator.metadata_shadow()[upload.slot_id as usize].chunk_origin,
        expected_origin
    );
}

#[test]
fn mesh_03_mesh_pipeline_binds_metadata_storage_buffer() {
    let binding = metadata_descriptor_layout_binding();

    assert_eq!(binding.binding, 0);
    assert_eq!(binding.descriptor_count, 1);
    assert_eq!(binding.descriptor_type, vk::DescriptorType::STORAGE_BUFFER);
    assert_eq!(binding.stage_flags, vk::ShaderStageFlags::VERTEX);
}

#[test]
fn mesh_03_vertex_shader_uses_metadata_for_world_placement() {
    let shader =
        std::fs::read_to_string("shaders/chunk_mesh.vert").expect("chunk mesh shader should exist");

    assert!(
        shader.contains("gl_InstanceIndex"),
        "vertex shader should index chunk metadata per draw instance"
    );
    assert!(
        shader.contains("chunk_origin"),
        "vertex shader should read chunk world origin from metadata"
    );
    assert!(
        shader.contains("world_position"),
        "vertex shader should place vertices in world space before projection"
    );
    assert!(
        !shader.contains("vec3 centered = (local - vec3(32.0, 32.0, 32.0)) / vec3(32.0, 32.0, 96.0);"),
        "vertex shader should no longer treat every chunk as local-only centered geometry"
    );
}

#[test]
fn mesh_03_cull_shader_consumes_metadata_and_dense_draw_slots() {
    let shader =
        std::fs::read_to_string("shaders/chunk_cull.comp").expect("chunk cull shader should exist");
    let pipeline_source = std::fs::read_to_string("src/renderer/cull_pipeline.rs")
        .expect("chunk cull pipeline source should exist");

    assert!(
        shader.contains("ChunkDrawMetadata"),
        "compute shader should declare the metadata layout it consumes"
    );
    assert!(
        shader.contains("draw_slots"),
        "compute shader should read the dense draw-slot list"
    );
    assert!(
        shader.contains("indirect_templates"),
        "compute shader should read stable indirect templates"
    );
    assert!(
        shader.contains("dense_indirect"),
        "compute shader should write dense indirect commands"
    );
    assert!(
        shader.contains("gl_GlobalInvocationID.x"),
        "compute shader should address dense draw indices"
    );
    assert!(
        shader.contains("instanceCount"),
        "compute shader should control per-command visibility through instanceCount"
    );
    assert!(
        pipeline_source.contains("DescriptorSetLayout"),
        "cull pipeline should define a descriptor-backed compute layout"
    );
    assert!(
        pipeline_source.contains("STORAGE_BUFFER"),
        "cull pipeline should bind storage buffers for compute"
    );
}

#[test]
fn mesh_03_submit_frame_uses_dense_indirect_draw_count() {
    let renderer_source =
        std::fs::read_to_string("src/renderer/mod.rs").expect("renderer module should exist");
    let mesh_source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("chunk mesh pipeline source should exist");

    assert!(
        renderer_source.contains("active_draw_count()"),
        "submit_frame should use the dense draw count rather than sparse active slots"
    );
    assert!(
        renderer_source.contains("dense_indirect_buffer()"),
        "submit_frame should barrier the dense indirect output buffer"
    );
    assert!(
        mesh_source.contains("dense_indirect_buffer()"),
        "graphics draw should read commands from the dense indirect buffer"
    );
    assert!(
        !renderer_source.contains("let draw_count = chunk_pool.active_chunk_count();"),
        "submit_frame should not derive indirect draw count from sparse stable slot ownership"
    );
}

#[test]
fn mesh_03_sparse_slot_remove_keeps_dense_indirect_order_valid() {
    let mut allocator = SlotAllocator::with_capacity(4);
    let first = chunk_key(0, 0, 0, 0);
    let second = chunk_key(1, 0, 0, 0);
    let third = chunk_key(2, 0, 0, 0);

    allocator
        .prepare_upload(first, &sample_packed_mesh(6, 1))
        .expect("first upload should succeed");
    allocator
        .prepare_upload(second, &sample_packed_mesh(12, 2))
        .expect("second upload should succeed");
    allocator
        .prepare_upload(third, &sample_packed_mesh(18, 3))
        .expect("third upload should succeed");

    allocator
        .prepare_remove(second)
        .expect("removing a middle stable slot should succeed");

    let first_slot = allocator
        .slot_for(first)
        .expect("first chunk should retain its stable slot");
    let third_slot = allocator
        .slot_for(third)
        .expect("third chunk should retain its stable slot");
    assert_eq!(
        allocator.draw_slots_shadow(),
        &[first_slot, third_slot],
        "dense draw order should stay compact after removing a middle stable slot"
    );

    let chunk_pool_source = std::fs::read_to_string("src/renderer/chunk_pool.rs")
        .expect("chunk pool source should exist");
    assert!(
        chunk_pool_source.contains("draw_slot_buffer"),
        "chunk pool should maintain a GPU-visible dense draw-slot buffer"
    );
    assert!(
        chunk_pool_source.contains("dense_indirect_buffer"),
        "chunk pool should maintain a GPU-visible dense indirect command buffer"
    );
}

#[test]
fn mesh_01_missing_validation_layer_disables_optional_debug_bootstrap() {
    let config = resolve_debug_instance_config(&[], &[DEBUG_UTILS_EXTENSION_NAME.to_string()]);

    assert!(
        !config.validation_layer_enabled,
        "missing validation layer should disable validation bootstrap"
    );
    assert!(
        !config.debug_utils_enabled,
        "debug utils should also be disabled when validation is unavailable"
    );
}

#[test]
fn mesh_01_missing_debug_utils_extension_disables_debug_messenger_only() {
    let config = resolve_debug_instance_config(&[VALIDATION_LAYER_NAME.to_string()], &[]);

    assert!(
        config.validation_layer_enabled,
        "validation should stay enabled when the layer is available"
    );
    assert!(
        !config.debug_utils_enabled,
        "debug messenger support should disable cleanly when debug utils is absent"
    );
}

#[test]
fn mesh_01_renderer_new_uses_instance_debug_config_contract() {
    let renderer_source =
        std::fs::read_to_string("src/renderer/mod.rs").expect("renderer module should exist");

    assert!(
        renderer_source.contains("let bootstrap = instance::create_instance(&entry, display_handle)?;"),
        "Renderer::new should receive the instance bootstrap contract"
    );
    assert!(
        renderer_source.contains("bootstrap.debug.debug_utils_enabled"),
        "Renderer::new should gate debug utils loader creation from the bootstrap debug config"
    );
    assert!(
        renderer_source.contains(
            "bootstrap.debug.validation_layer_enabled && bootstrap.debug.debug_utils_enabled"
        ),
        "Renderer::new should require both validation and debug utils before creating a debug messenger"
    );
    assert!(
        !renderer_source.contains("let debug_messenger = Some(instance::setup_debug_messenger(&entry, &instance)?);"),
        "Renderer::new should no longer create the debug messenger unconditionally in debug builds"
    );
}

#[test]
fn mesh_01_spirv_word_decoder_accepts_unaligned_byte_input() {
    let bytes = [
        0xFF, 0x03, 0x02, 0x01, 0x00, 0x0D, 0x0C, 0x0B, 0x0A,
    ];

    let words = decode_spirv_words(&bytes[1..]).expect("unaligned SPIR-V bytes should decode");

    assert_eq!(words, vec![0x0001_0203, 0x0A0B_0C0D]);
}

#[test]
fn mesh_01_spirv_word_decoder_rejects_non_word_aligned_length() {
    let err = decode_spirv_words(&[0x01, 0x02, 0x03]).expect_err(
        "byte lengths that are not divisible by 4 should return an explicit error",
    );

    assert_eq!(err.to_string(), "SPIR-V byte length must be a multiple of 4");
}

#[test]
fn mesh_01_pipeline_sources_stop_using_bytemuck_cast_slice_for_shader_modules() {
    let mesh_source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("chunk mesh pipeline source should exist");
    let cull_source = std::fs::read_to_string("src/renderer/cull_pipeline.rs")
        .expect("chunk cull pipeline source should exist");

    assert!(
        !mesh_source.contains("bytemuck::cast_slice(bytes)"),
        "graphics shader-module creation should stop depending on input alignment"
    );
    assert!(
        !cull_source.contains("bytemuck::cast_slice(bytes)"),
        "compute shader-module creation should stop depending on input alignment"
    );
}

#[test]
fn mesh_01_chunk_mesh_pipeline_uses_alignment_safe_spirv_decoder() {
    let mesh_source = std::fs::read_to_string("src/renderer/mesh_pipeline.rs")
        .expect("chunk mesh pipeline source should exist");

    assert!(
        mesh_source.contains("decode_spirv_words(bytes)?"),
        "graphics shader-module creation should decode SPIR-V bytes into owned words first"
    );
    assert!(
        mesh_source.contains("ShaderModuleCreateInfo::default().code(&code)"),
        "graphics shader-module creation should pass aligned words into Vulkan"
    );
}

#[test]
fn mesh_01_chunk_cull_pipeline_uses_alignment_safe_spirv_decoder() {
    let cull_source = std::fs::read_to_string("src/renderer/cull_pipeline.rs")
        .expect("chunk cull pipeline source should exist");

    assert!(
        cull_source.contains("decode_spirv_words(bytes)?"),
        "compute shader-module creation should decode SPIR-V bytes into owned words first"
    );
    assert!(
        cull_source.contains("ShaderModuleCreateInfo::default().code(&code)"),
        "compute shader-module creation should pass aligned words into Vulkan"
    );
}

#[test]
fn mesh_01_pack_vertex_uses_7_bit_coordinate_encoding() {
    let packed = pack_vertex([1, 2, 3], 4, 513, [5, 6]);
    assert_eq!(packed.0[0], 1 | (2 << 7) | (3 << 14) | (4 << 21));
    assert_eq!(packed.0[1], 513 | (5 << 16) | (6 << 24));
}

#[test]
fn mesh_01_pack_quad_produces_non_degenerate_quads() {
    let quad = GreedyQuad {
        axis: 0,
        positive_face: true,
        origin: [5, 10, 20],
        size: [3, 4],
        block_id: 7,
        is_skirt: false,
    };
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    pack_quad(&quad, &mut vertices, &mut indices);
    assert_eq!(vertices.len(), 4);
    assert_eq!(indices.len(), 6);
    // At least two vertices must have distinct word0 (distinct positions)
    let distinct_positions: std::collections::HashSet<u32> =
        vertices.iter().map(|v| v.0[0]).collect();
    assert!(
        distinct_positions.len() >= 2,
        "pack_quad must produce non-degenerate quads with distinct positions, got {distinct_positions:?}"
    );
    // Decode positions and verify span
    let decode = |word0: u32| -> [u32; 3] {
        [word0 & 0x7F, (word0 >> 7) & 0x7F, (word0 >> 14) & 0x7F]
    };
    let positions: Vec<[u32; 3]> = vertices.iter().map(|v| decode(v.0[0])).collect();
    // For axis=0, u_axis=Y(1), v_axis=Z(2); size=[3,4]
    let y_range = positions.iter().map(|p| p[1]).max().unwrap()
                - positions.iter().map(|p| p[1]).min().unwrap();
    let z_range = positions.iter().map(|p| p[2]).max().unwrap()
                - positions.iter().map(|p| p[2]).min().unwrap();
    assert_eq!(y_range, 3, "Y span should match size[0]=3");
    assert_eq!(z_range, 4, "Z span should match size[1]=4");
}

#[test]
fn mesh_01_greedy_mesh_single_block_has_nonzero_position_spread() {
    let mut block_ids = vec![0u8; CHUNK_VOXEL_COUNT];
    let index = ChunkVoxels::linear_index(1, 1, 1);
    block_ids[index] = 2;
    let chunk =
        ChunkVoxels::new(block_ids.into_boxed_slice()).expect("single block chunk must be valid");

    let mesh = build_greedy_mesh(
        &chunk,
        &ChunkNeighborSet::default(),
        &MeshDirtyRecord {
            causes: Vec::new(),
            source_revision: 1,
            finer_neighbor_face_mask: 0,
        },
    );

    assert!(
        !mesh.vertices.is_empty(),
        "single-block chunk should produce at least one vertex"
    );

    let decode = |word0: u32| -> [u32; 3] {
        [word0 & 0x7F, (word0 >> 7) & 0x7F, (word0 >> 14) & 0x7F]
    };
    let positions: Vec<[u32; 3]> = mesh.vertices.iter().map(|v| decode(v.0[0])).collect();

    for axis in 0..3 {
        let min = positions.iter().map(|p| p[axis]).min().unwrap();
        let max = positions.iter().map(|p| p[axis]).max().unwrap();
        assert!(
            max - min >= 1,
            "axis {axis}: all vertex positions identical ({min}); vertices are degenerate"
        );
    }
}
