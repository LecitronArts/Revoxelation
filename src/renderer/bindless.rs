//! Unified bindless descriptor set 0 shared by all pipelines (cull + mesh).
//!
//! BindlessTable manages a single descriptor set with Vulkan 1.2
//! UPDATE_AFTER_BIND and PARTIALLY_BOUND flags, eliminating per-pipeline
//! descriptor infrastructure.

use anyhow::{Context, Result};
use ash::vk;

/// Number of bindings in the global bindless descriptor set 0.
///
/// Extended from 16 to 25 in Phase 7 for lighting/PBR bindings (LGHT-01).
const BINDING_COUNT: usize = 25;

// ---------------------------------------------------------------------------
// Named binding ID constants (REFAC-05, extended LGHT-01)
// ---------------------------------------------------------------------------
// Replace all magic-number binding indices with these named constants.

/// Binding 0: scene/metadata SSBO (GpuChunkInstance[] + indirect templates + draw slots + dense indirect).
pub const BINDING_SCENE: u32 = 0;
/// Binding 1: indirect templates (legacy, shares scene_buffer region).
pub const BINDING_INDIRECT_TEMPLATES: u32 = 1;
/// Binding 2: draw slots (legacy, shares scene_buffer region).
pub const BINDING_DRAW_SLOTS: u32 = 2;
/// Binding 3: dense indirect output (legacy, shares scene_buffer region).
pub const BINDING_DENSE_INDIRECT: u32 = 3;
/// Binding 4: frustum planes SSBO.
pub const BINDING_FRUSTUM_PLANES: u32 = 4;
/// Binding 5: draw count SSBO.
pub const BINDING_DRAW_COUNT: u32 = 5;
/// Binding 6: Hi-Z config SSBO.
pub const BINDING_HIZ_CONFIG: u32 = 6;
/// Binding 7: Hi-Z pyramid combined image sampler.
pub const BINDING_HIZ_PYRAMID: u32 = 7;
/// Binding 8: material SSBO.
pub const BINDING_MATERIAL: u32 = 8;
/// Binding 9: texture array combined image sampler.
pub const BINDING_TEXTURE_ARRAY: u32 = 9;
/// Binding 10: meshlet metadata SSBO (GpuMeshlet[]).
pub const BINDING_MESHLET_META: u32 = 10;
/// Binding 11: meshlet vertex SSBO (PackedVertex[]).
pub const BINDING_MESHLET_VERTEX: u32 = 11;
/// Binding 12: meshlet triangle index SSBO (u32[]).
pub const BINDING_MESHLET_TRI: u32 = 12;
/// Binding 13: visible meshlet SSBO (u32[], cull output).
pub const BINDING_VISIBLE_MESHLET: u32 = 13;
/// Binding 14: meshlet indirect SSBO (DrawIndexedIndirectCommand[]).
pub const BINDING_MESHLET_INDIRECT: u32 = 14;
/// Binding 15: meshlet count SSBO (u32, visible meshlet count).
pub const BINDING_MESHLET_COUNT: u32 = 15;
/// Binding 16: CSM shadow maps combined image sampler (LGHT-02, reserved).
pub const BINDING_CSM_SHADOW_MAPS: u32 = 16;
/// Binding 17: SSAO texture combined image sampler (LGHT-03, reserved).
pub const BINDING_SSAO_TEXTURE: u32 = 17;
/// Binding 18: lighting parameters SSBO (LGHT-01).
pub const BINDING_LIGHTING_UBO: u32 = 18;
/// Binding 19: metallic-roughness texture array combined image sampler (LGHT-01).
pub const BINDING_MR_TEXTURE_ARRAY: u32 = 19;
/// Binding 20: normal map texture array combined image sampler (LGHT-01).
pub const BINDING_NORMAL_TEXTURE_ARRAY: u32 = 20;
/// Binding 21: emissive texture array combined image sampler (LGHT-01).
pub const BINDING_EMISSIVE_TEXTURE_ARRAY: u32 = 21;
/// Binding 22: point light SSBO (LGHT-01).
pub const BINDING_POINT_LIGHT_SSBO: u32 = 22;
/// Binding 23: sky/atmosphere parameters SSBO (LGHT-05, reserved).
pub const BINDING_SKY_PARAMS: u32 = 23;
/// Binding 24: SSAO blur intermediate storage image (LGHT-03, reserved).
#[allow(dead_code)]
pub const BINDING_SSAO_BLUR_INTERMEDIATE: u32 = 24;

/// Manages the global descriptor set 0 shared by all pipelines.
///
/// Layout (D-01, extended D-08, extended LGHT-01):
/// - binding  0: STORAGE_BUFFER (scene/metadata SSBO) — COMPUTE | VERTEX
/// - binding  1: STORAGE_BUFFER (indirect templates) — COMPUTE
/// - binding  2: STORAGE_BUFFER (draw slots) — COMPUTE
/// - binding  3: STORAGE_BUFFER (dense indirect output) — COMPUTE
/// - binding  4: STORAGE_BUFFER (frustum planes) — COMPUTE
/// - binding  5: STORAGE_BUFFER (draw count) — COMPUTE
/// - binding  6: STORAGE_BUFFER (Hi-Z config) — COMPUTE
/// - binding  7: COMBINED_IMAGE_SAMPLER (Hi-Z pyramid) — COMPUTE
/// - binding  8: STORAGE_BUFFER (material SSBO) — FRAGMENT
/// - binding  9: COMBINED_IMAGE_SAMPLER (texture array) — FRAGMENT
/// - binding 10: STORAGE_BUFFER (meshlet_meta) — COMPUTE
/// - binding 11: STORAGE_BUFFER (meshlet_vertex) — COMPUTE | VERTEX
/// - binding 12: STORAGE_BUFFER (meshlet_tri) — COMPUTE
/// - binding 13: STORAGE_BUFFER (visible_meshlet) — COMPUTE
/// - binding 14: STORAGE_BUFFER (meshlet_indirect) — COMPUTE
/// - binding 15: STORAGE_BUFFER (meshlet_count) — COMPUTE
/// - binding 16: COMBINED_IMAGE_SAMPLER (CSM shadow maps) — FRAGMENT
/// - binding 17: COMBINED_IMAGE_SAMPLER (SSAO texture) — FRAGMENT
/// - binding 18: STORAGE_BUFFER (lighting params) — VERTEX | FRAGMENT
/// - binding 19: COMBINED_IMAGE_SAMPLER (MR texture array) — FRAGMENT
/// - binding 20: COMBINED_IMAGE_SAMPLER (normal map texture array) — FRAGMENT
/// - binding 21: COMBINED_IMAGE_SAMPLER (emissive texture array) — FRAGMENT
/// - binding 22: STORAGE_BUFFER (point light SSBO) — FRAGMENT
/// - binding 23: STORAGE_BUFFER (sky params) — VERTEX | FRAGMENT
/// - binding 24: STORAGE_IMAGE (SSAO blur intermediate) — COMPUTE
///
/// All bindings have PARTIALLY_BOUND | UPDATE_AFTER_BIND flags (D-02).
pub struct BindlessTable {
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
}

impl BindlessTable {
    /// Create the BindlessTable with all 25 bindings, UPDATE_AFTER_BIND pool and layout.
    ///
    /// When `mesh_shader_supported` is true, TASK_EXT and MESH_EXT
    /// are added to stage flags for bindings accessible from task/mesh shaders (HIGH-07).
    pub fn new(device: &ash::Device, mesh_shader_supported: bool) -> Result<Self> {
        // Extra stage flags for mesh shader pipeline (HIGH-07).
        let mesh_extra = if mesh_shader_supported {
            vk::ShaderStageFlags::TASK_EXT | vk::ShaderStageFlags::MESH_EXT
        } else {
            vk::ShaderStageFlags::empty()
        };

        // Define all 25 bindings.
        let bindings = [
            // binding 0: scene/metadata SSBO — COMPUTE | VERTEX (+ TASK/MESH)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_SCENE)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | vk::ShaderStageFlags::VERTEX | mesh_extra),
            // binding 1: indirect templates — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_INDIRECT_TEMPLATES)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 2: draw slots — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_DRAW_SLOTS)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 3: dense indirect output — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_DENSE_INDIRECT)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 4: frustum planes — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_FRUSTUM_PLANES)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 5: draw count — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_DRAW_COUNT)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 6: Hi-Z config — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_HIZ_CONFIG)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 7: Hi-Z pyramid combined image sampler — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_HIZ_PYRAMID)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 8: material SSBO — FRAGMENT (+ MESH for mesh shader material access)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_MATERIAL)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT | mesh_extra),
            // binding 9: texture array — FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_TEXTURE_ARRAY)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 10: meshlet_meta SSBO — COMPUTE | VERTEX (+ TASK/MESH)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_MESHLET_META)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | vk::ShaderStageFlags::VERTEX | mesh_extra),
            // binding 11: meshlet_vertex SSBO — COMPUTE | VERTEX (+ TASK/MESH)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_MESHLET_VERTEX)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | vk::ShaderStageFlags::VERTEX | mesh_extra),
            // binding 12: meshlet_tri SSBO — COMPUTE (+ TASK/MESH)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_MESHLET_TRI)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | mesh_extra),
            // binding 13: visible_meshlet SSBO — COMPUTE | VERTEX (+ TASK/MESH)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_VISIBLE_MESHLET)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | vk::ShaderStageFlags::VERTEX | mesh_extra),
            // binding 14: meshlet_indirect SSBO — COMPUTE (+ TASK/MESH)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_MESHLET_INDIRECT)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | mesh_extra),
            // binding 15: meshlet_count SSBO — COMPUTE (+ TASK/MESH)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_MESHLET_COUNT)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | mesh_extra),
            // binding 16: CSM shadow maps — FRAGMENT (reserved for LGHT-02)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_CSM_SHADOW_MAPS)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 17: SSAO texture — FRAGMENT (reserved for LGHT-03)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_SSAO_TEXTURE)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 18: lighting params SSBO — VERTEX | FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_LIGHTING_UBO)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            // binding 19: MR texture array — FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_MR_TEXTURE_ARRAY)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 20: normal map texture array — FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_NORMAL_TEXTURE_ARRAY)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 21: emissive texture array — FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_EMISSIVE_TEXTURE_ARRAY)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 22: point light SSBO — FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_POINT_LIGHT_SSBO)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 23: sky params SSBO — VERTEX | FRAGMENT (reserved for LGHT-05)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_SKY_PARAMS)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT),
            // binding 24: SSAO blur intermediate — COMPUTE (reserved for LGHT-03)
            vk::DescriptorSetLayoutBinding::default()
                .binding(BINDING_SSAO_BLUR_INTERMEDIATE)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
        ];

        // All bindings get PARTIALLY_BOUND | UPDATE_AFTER_BIND flags (D-02).
        let binding_flags = [vk::DescriptorBindingFlags::PARTIALLY_BOUND
            | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND; BINDING_COUNT];

        let mut binding_flags_info =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default()
                .binding_flags(&binding_flags);

        // Layout created with UPDATE_AFTER_BIND_BIT.
        let descriptor_set_layout = unsafe {
            device
                .create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default()
                        .bindings(&bindings)
                        .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
                        .push_next(&mut binding_flags_info),
                    None,
                )
                .context("failed to create bindless descriptor set layout")?
        };

        // Pool created with UPDATE_AFTER_BIND_BIT flag (D-02).
        // 17 STORAGE_BUFFER descriptors: bindings 0-6, 8, 10-15, 18, 22, 23
        //  6 COMBINED_IMAGE_SAMPLER descriptors: bindings 7, 9, 16, 17, 19, 20, 21
        //  1 STORAGE_IMAGE descriptor: binding 24
        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(17), // bindings 0-6, 8, 10-15, 18, 22, 23
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(7), // bindings 7, 9, 16, 17, 19, 20, 21
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(1), // binding 24
        ];
        let descriptor_pool = unsafe {
            device
                .create_descriptor_pool(
                    &vk::DescriptorPoolCreateInfo::default()
                        .pool_sizes(&pool_sizes)
                        .max_sets(1)
                        .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND),
                    None,
                )
                .context("failed to create bindless descriptor pool")?
        };

        // Allocate the single descriptor set.
        let set_layouts = [descriptor_set_layout];
        let descriptor_set = unsafe {
            device
                .allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(descriptor_pool)
                        .set_layouts(&set_layouts),
                )
                .context("failed to allocate bindless descriptor set")?
                .into_iter()
                .next()
                .context("bindless descriptor set allocation returned empty")?
        };

        Ok(Self {
            descriptor_pool,
            descriptor_set_layout,
            descriptor_set,
        })
    }

    /// Register all 6 meshlet SSBOs from MeshletPool at bindings 10-15.
    pub fn register_meshlet_buffers(
        &self,
        device: &ash::Device,
        meshlet_pool: &super::chunk_pool::MeshletPool,
    ) {
        self.register_buffer(device, BINDING_MESHLET_META, meshlet_pool.meshlet_meta_buffer, vk::WHOLE_SIZE);
        self.register_buffer(device, BINDING_MESHLET_VERTEX, meshlet_pool.meshlet_vertex_buffer, vk::WHOLE_SIZE);
        self.register_buffer(device, BINDING_MESHLET_TRI, meshlet_pool.meshlet_tri_buffer, vk::WHOLE_SIZE);
        self.register_buffer(device, BINDING_VISIBLE_MESHLET, meshlet_pool.visible_meshlet_buffer, vk::WHOLE_SIZE);
        self.register_buffer(device, BINDING_MESHLET_INDIRECT, meshlet_pool.meshlet_indirect_buffer, vk::WHOLE_SIZE);
        self.register_buffer(device, BINDING_MESHLET_COUNT, meshlet_pool.meshlet_count_buffer, vk::WHOLE_SIZE);
    }

    /// Write a STORAGE_BUFFER descriptor to the given binding (D-03).
    pub fn register_buffer(
        &self,
        device: &ash::Device,
        binding: u32,
        buffer: vk::Buffer,
        range: vk::DeviceSize,
    ) {
        let buffer_info = [vk::DescriptorBufferInfo::default()
            .buffer(buffer)
            .offset(0)
            .range(range)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(self.descriptor_set)
            .dst_binding(binding)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&buffer_info)];
        unsafe {
            device.update_descriptor_sets(&writes, &[]);
        }
    }

    /// Write a COMBINED_IMAGE_SAMPLER descriptor to the given binding (D-03).
    pub fn register_image(
        &self,
        device: &ash::Device,
        binding: u32,
        view: vk::ImageView,
        sampler: vk::Sampler,
        layout: vk::ImageLayout,
    ) {
        let image_info = [vk::DescriptorImageInfo::default()
            .image_view(view)
            .sampler(sampler)
            .image_layout(layout)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(self.descriptor_set)
            .dst_binding(binding)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe {
            device.update_descriptor_sets(&writes, &[]);
        }
    }

    /// Clean up descriptor pool and layout.
    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.descriptor_set_layout, None);
        }
    }
}
