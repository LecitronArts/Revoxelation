//! Unified bindless descriptor set 0 shared by all pipelines (cull + mesh).
//!
//! BindlessTable manages a single descriptor set with Vulkan 1.2
//! UPDATE_AFTER_BIND and PARTIALLY_BOUND flags, eliminating per-pipeline
//! descriptor infrastructure.

use anyhow::{Context, Result};
use ash::vk;

/// Number of bindings in the global bindless descriptor set 0.
const BINDING_COUNT: usize = 10;

/// Manages the global descriptor set 0 shared by all pipelines.
///
/// Layout (D-01):
/// - binding 0: STORAGE_BUFFER (scene/metadata SSBO) — COMPUTE | VERTEX
/// - binding 1: STORAGE_BUFFER (indirect templates) — COMPUTE
/// - binding 2: STORAGE_BUFFER (draw slots) — COMPUTE
/// - binding 3: STORAGE_BUFFER (dense indirect output) — COMPUTE
/// - binding 4: STORAGE_BUFFER (frustum planes) — COMPUTE
/// - binding 5: STORAGE_BUFFER (draw count) — COMPUTE
/// - binding 6: STORAGE_BUFFER (Hi-Z config) — COMPUTE
/// - binding 7: COMBINED_IMAGE_SAMPLER (Hi-Z pyramid) — COMPUTE
/// - binding 8: STORAGE_BUFFER (material SSBO, reserved for Plan 04) — FRAGMENT
/// - binding 9: COMBINED_IMAGE_SAMPLER (texture array, reserved for Plan 04) — FRAGMENT
///
/// All bindings have PARTIALLY_BOUND | UPDATE_AFTER_BIND flags (D-02).
/// Bindings 8-9 can remain unbound until Plan 04 fills them.
pub struct BindlessTable {
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    pub descriptor_set: vk::DescriptorSet,
}

impl BindlessTable {
    /// Create the BindlessTable with all 10 bindings, UPDATE_AFTER_BIND pool and layout.
    pub fn new(device: &ash::Device) -> Result<Self> {
        // Define all 10 bindings.
        let bindings = [
            // binding 0: scene/metadata SSBO — COMPUTE | VERTEX
            vk::DescriptorSetLayoutBinding::default()
                .binding(0)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE | vk::ShaderStageFlags::VERTEX),
            // binding 1: indirect templates — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(1)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 2: draw slots — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(2)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 3: dense indirect output — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(3)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 4: frustum planes — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(4)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 5: draw count — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(5)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 6: Hi-Z config — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(6)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 7: Hi-Z pyramid combined image sampler — COMPUTE
            vk::DescriptorSetLayoutBinding::default()
                .binding(7)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::COMPUTE),
            // binding 8: material SSBO (reserved for Plan 04) — FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(8)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            // binding 9: texture array (reserved for Plan 04) — FRAGMENT
            vk::DescriptorSetLayoutBinding::default()
                .binding(9)
                .descriptor_count(1)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .stage_flags(vk::ShaderStageFlags::FRAGMENT),
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
        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(8), // bindings 0-6, 8
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(2), // bindings 7, 9
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
