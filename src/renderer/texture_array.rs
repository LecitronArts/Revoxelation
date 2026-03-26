//! 2D texture array for block textures.
//!
//! Creates a VkImage (2D array, 16×16 RGBA8, 256 max layers) and populates
//! the first ~10 layers with procedurally generated pixel-art textures.
//! Registered at bindless binding 9 as a COMBINED_IMAGE_SAMPLER.

use anyhow::{Context, Result, anyhow};
use ash::vk;
use gpu_allocator::vulkan::{Allocation, AllocationCreateDesc, AllocationScheme};
use gpu_allocator::MemoryLocation;

use super::Renderer;
use super::helpers::{allocator_mut, submit_one_shot_commands};

/// Texture resolution per layer (16×16 pixels).
const TEX_SIZE: u32 = 16;
/// Maximum number of layers in the texture array.
const MAX_LAYERS: u32 = 256;
/// Number of initial procedural texture layers.
const INITIAL_LAYERS: u32 = 11; // layer 0 placeholder + 10 block textures

/// A 2D texture array holding all block textures.
pub struct TextureArray {
    pub image: vk::Image,
    pub allocation: Option<Allocation>,
    pub view: vk::ImageView,
    pub sampler: vk::Sampler,
}

impl TextureArray {
    /// Create the texture array, generate procedural textures, upload, and register
    /// at bindless binding 9.
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let device = renderer.device_ctx.device.clone();

        // --- Create VkImage (2D array, RGBA8, 256 layers) ---
        let image = unsafe {
            device
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(vk::Format::R8G8B8A8_UNORM)
                        .extent(vk::Extent3D {
                            width: TEX_SIZE,
                            height: TEX_SIZE,
                            depth: 1,
                        })
                        .mip_levels(1)
                        .array_layers(MAX_LAYERS)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(
                            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
                        )
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED),
                    None,
                )
                .context("failed to create texture array image")?
        };

        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let allocation = allocator_mut(renderer)
            .allocate(&AllocationCreateDesc {
                name: "texture-array",
                requirements,
                location: MemoryLocation::GpuOnly,
                linear: false,
                allocation_scheme: AllocationScheme::DedicatedImage(image),
            })
            .map_err(|e| anyhow!("failed to allocate texture array memory: {e}"))?;

        unsafe {
            device
                .bind_image_memory(image, allocation.memory(), allocation.offset())
                .context("failed to bind texture array image memory")?;
        }

        // --- Generate procedural textures ---
        let textures = generate_all_textures();

        // --- Create staging buffer for upload ---
        let pixel_bytes_per_layer = (TEX_SIZE * TEX_SIZE * 4) as usize;
        let total_staging_size = pixel_bytes_per_layer * INITIAL_LAYERS as usize;
        let (staging_buffer, staging_alloc) = super::helpers::create_allocated_buffer(
            renderer,
            total_staging_size as u64,
            vk::BufferUsageFlags::TRANSFER_SRC,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "texture-array-staging",
        )?;

        // Write pixel data to staging buffer
        if let Some(mapped) = staging_alloc.mapped_ptr() {
            let ptr = mapped.as_ptr() as *mut u8;
            for (i, tex_data) in textures.iter().enumerate() {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        tex_data.as_ptr(),
                        ptr.add(i * pixel_bytes_per_layer),
                        pixel_bytes_per_layer,
                    );
                }
            }
        }

        // --- Upload via one-shot commands ---
        let device_clone = device.clone();
        submit_one_shot_commands(renderer, |device, cmd| {
            // Transition all layers to TRANSFER_DST
            let barrier = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_access_mask(vk::AccessFlags::empty())
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .image(image)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(MAX_LAYERS),
                );
            unsafe {
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
            }

            // Copy each layer from staging buffer to image
            let mut regions = Vec::with_capacity(INITIAL_LAYERS as usize);
            for layer in 0..INITIAL_LAYERS {
                regions.push(
                    vk::BufferImageCopy::default()
                        .buffer_offset((layer as u64) * pixel_bytes_per_layer as u64)
                        .buffer_row_length(0)
                        .buffer_image_height(0)
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .mip_level(0)
                                .base_array_layer(layer)
                                .layer_count(1),
                        )
                        .image_offset(vk::Offset3D { x: 0, y: 0, z: 0 })
                        .image_extent(vk::Extent3D {
                            width: TEX_SIZE,
                            height: TEX_SIZE,
                            depth: 1,
                        }),
                );
            }
            unsafe {
                device.cmd_copy_buffer_to_image(
                    cmd,
                    staging_buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &regions,
                );
            }

            // Transition all layers to SHADER_READ_ONLY_OPTIMAL
            let barrier2 = vk::ImageMemoryBarrier::default()
                .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::SHADER_READ)
                .image(image)
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(MAX_LAYERS),
                );
            unsafe {
                device.cmd_pipeline_barrier(
                    cmd,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier2],
                );
            }

            Ok(())
        })?;

        // Clean up staging buffer
        super::helpers::destroy_allocated_buffer(renderer, staging_buffer, staging_alloc)?;

        // --- Create VkImageView (2D_ARRAY) ---
        let view = unsafe {
            device_clone
                .create_image_view(
                    &vk::ImageViewCreateInfo::default()
                        .image(image)
                        .view_type(vk::ImageViewType::TYPE_2D_ARRAY)
                        .format(vk::Format::R8G8B8A8_UNORM)
                        .components(vk::ComponentMapping::default())
                        .subresource_range(
                            vk::ImageSubresourceRange::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .base_mip_level(0)
                                .level_count(1)
                                .base_array_layer(0)
                                .layer_count(MAX_LAYERS),
                        ),
                    None,
                )
                .context("failed to create texture array image view")?
        };

        // --- Create VkSampler (NEAREST filter — pixelated voxel look) ---
        let sampler = unsafe {
            device_clone
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .mag_filter(vk::Filter::NEAREST)
                        .min_filter(vk::Filter::NEAREST)
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::REPEAT)
                        .address_mode_v(vk::SamplerAddressMode::REPEAT)
                        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                        .max_lod(0.0),
                    None,
                )
                .context("failed to create texture array sampler")?
        };

        // --- Register at bindless binding 9 ---
        if let Some(bindless) = renderer.bindless.as_ref() {
            bindless.register_image(
                &renderer.device_ctx.device,
                9,
                view,
                sampler,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        }

        Ok(Self {
            image,
            allocation: Some(allocation),
            view,
            sampler,
        })
    }

    /// Clean up all GPU resources.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        let device = &renderer.device_ctx.device;
        unsafe {
            device.destroy_sampler(self.sampler, None);
            device.destroy_image_view(self.view, None);
        }
        if let Some(alloc) = self.allocation.take() {
            super::helpers::destroy_allocated_image(renderer, self.image, alloc)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Procedural texture generation (16×16 RGBA8 per layer)
// ---------------------------------------------------------------------------

type TexPixels = Vec<u8>;

/// Generate all initial texture layers, returning one Vec<u8> per layer.
fn generate_all_textures() -> Vec<TexPixels> {
    vec![
        gen_placeholder(),   // layer 0: unused placeholder (black)
        gen_dirt(),          // layer 1: dirt
        gen_grass_top(),     // layer 2: grass top
        gen_grass_side(),    // layer 3: grass side
        gen_stone(),         // layer 4: stone
        gen_sand(),          // layer 5: sand
        gen_log_bark(),      // layer 6: log bark
        gen_log_end(),       // layer 7: log end
        gen_planks(),        // layer 8: planks
        gen_leaves(),        // layer 9: leaves
        gen_water(),         // layer 10: water
    ]
}

fn new_tex() -> TexPixels {
    vec![0u8; (TEX_SIZE * TEX_SIZE * 4) as usize]
}

fn set_pixel(tex: &mut TexPixels, x: u32, y: u32, r: u8, g: u8, b: u8, a: u8) {
    let idx = ((y * TEX_SIZE + x) * 4) as usize;
    tex[idx] = r;
    tex[idx + 1] = g;
    tex[idx + 2] = b;
    tex[idx + 3] = a;
}

/// Simple hash-based noise for procedural variation.
fn noise(x: u32, y: u32, seed: u32) -> u8 {
    let mut h = x.wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(seed.wrapping_mul(2147483647));
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    (h & 0xFF) as u8
}

fn gen_placeholder() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            set_pixel(&mut tex, x, y, 0, 0, 0, 255);
        }
    }
    tex
}

fn gen_dirt() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 1) as i16;
            let r = (139 + (n - 128) / 8).clamp(0, 255) as u8;
            let g = (90 + (n - 128) / 10).clamp(0, 255) as u8;
            let b = (43 + (n - 128) / 12).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, 255);
        }
    }
    tex
}

fn gen_grass_top() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 2) as i16;
            let r = (60 + (n - 128) / 10).clamp(0, 255) as u8;
            let g = (170 + (n - 128) / 6).clamp(0, 255) as u8;
            let b = (40 + (n - 128) / 12).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, 255);
        }
    }
    tex
}

fn gen_grass_side() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            if y < 4 {
                // Top strip — green
                let n = noise(x, y, 3) as i16;
                let r = (70 + (n - 128) / 10).clamp(0, 255) as u8;
                let g = (160 + (n - 128) / 6).clamp(0, 255) as u8;
                let b = (45 + (n - 128) / 12).clamp(0, 255) as u8;
                set_pixel(&mut tex, x, y, r, g, b, 255);
            } else {
                // Bottom — dirt
                let n = noise(x, y, 1) as i16;
                let r = (139 + (n - 128) / 8).clamp(0, 255) as u8;
                let g = (90 + (n - 128) / 10).clamp(0, 255) as u8;
                let b = (43 + (n - 128) / 12).clamp(0, 255) as u8;
                set_pixel(&mut tex, x, y, r, g, b, 255);
            }
        }
    }
    tex
}

fn gen_stone() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 4) as i16;
            let base = (128 + (n - 128) / 6).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, base, base, base, 255);
        }
    }
    tex
}

fn gen_sand() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 5) as i16;
            let r = (220 + (n - 128) / 8).clamp(0, 255) as u8;
            let g = (200 + (n - 128) / 8).clamp(0, 255) as u8;
            let b = (140 + (n - 128) / 10).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, 255);
        }
    }
    tex
}

fn gen_log_bark() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 6) as i16;
            // Vertical stripe pattern for bark
            let stripe = if (x % 4) < 2 { 15i16 } else { -15i16 };
            let r = (100 + stripe + (n - 128) / 10).clamp(0, 255) as u8;
            let g = (70 + stripe + (n - 128) / 12).clamp(0, 255) as u8;
            let b = (35 + (n - 128) / 14).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, 255);
        }
    }
    tex
}

fn gen_log_end() -> TexPixels {
    let mut tex = new_tex();
    let cx = TEX_SIZE as f32 / 2.0;
    let cy = TEX_SIZE as f32 / 2.0;
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let ring = ((dist * 1.5) as u32) % 2;
            let n = noise(x, y, 7) as i16;
            let base = if ring == 0 { 120i16 } else { 90i16 };
            let r = (base + (n - 128) / 10).clamp(0, 255) as u8;
            let g = ((base - 20) + (n - 128) / 12).clamp(0, 255) as u8;
            let b = ((base - 50) + (n - 128) / 14).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, 255);
        }
    }
    tex
}

fn gen_planks() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 8) as i16;
            // Horizontal plank lines
            let line = if (y % 4) == 0 { -25i16 } else { 0i16 };
            let r = (180 + line + (n - 128) / 10).clamp(0, 255) as u8;
            let g = (145 + line + (n - 128) / 10).clamp(0, 255) as u8;
            let b = (90 + line + (n - 128) / 12).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, 255);
        }
    }
    tex
}

fn gen_leaves() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 9);
            // Sparse transparency
            let alpha = if n < 40 { 0u8 } else { 255u8 };
            let n16 = n as i16;
            let r = (30 + (n16 - 128) / 10).clamp(0, 255) as u8;
            let g = (140 + (n16 - 128) / 6).clamp(0, 255) as u8;
            let b = (30 + (n16 - 128) / 12).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, alpha);
        }
    }
    tex
}

fn gen_water() -> TexPixels {
    let mut tex = new_tex();
    for y in 0..TEX_SIZE {
        for x in 0..TEX_SIZE {
            let n = noise(x, y, 10) as i16;
            // Wave-like pattern
            let wave = (((x + y) % 8) as i16 - 4).abs() * 5;
            let r = (30 + (n - 128) / 12).clamp(0, 255) as u8;
            let g = (80 + wave + (n - 128) / 10).clamp(0, 255) as u8;
            let b = (200 + (n - 128) / 6).clamp(0, 255) as u8;
            set_pixel(&mut tex, x, y, r, g, b, 180);
        }
    }
    tex
}
