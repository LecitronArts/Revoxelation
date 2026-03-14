use bytemuck::cast_slice;
use wgpu::util::DeviceExt;

use super::payload_builder::GpuWorldPayload;

#[derive(Debug, Clone, PartialEq)]
pub struct WorldUploadMetadata {
    pub chunk_count: u32,
    pub chunk_map_size: u32,
    pub chunk_map_mask: u32,
    pub chunk_map_max_probe: u32,
    pub chunk_map_avg_probe: f32,
    pub chunk_map_max_probe_observed: u32,
    pub chunk_map_load_factor: f32,
    pub chunk_map_dropped_entries: u32,
    pub emissive_count: u32,
    pub emissive_cdf_count: u32,
    pub emissive_remap_count: u32,
    pub emissive_signatures: Vec<u32>,
    pub importance_map_dims: [u32; 3],
    pub world_min: [i32; 3],
    pub world_max: [i32; 3],
}

pub struct WorldUploadPlan {
    metadata: WorldUploadMetadata,
    payload: GpuWorldPayload,
    remap: Vec<u32>,
}

pub struct UploadedWorldResources {
    pub voxel_buffer: wgpu::Buffer,
    pub chunk_meta_buffer: wgpu::Buffer,
    pub chunk_map_buffer: wgpu::Buffer,
    pub emissive_voxel_buffer: wgpu::Buffer,
    pub emissive_cdf_buffer: wgpu::Buffer,
    pub emissive_remap_buffer: wgpu::Buffer,
    pub importance_map_texture: wgpu::Texture,
    pub importance_map_view: wgpu::TextureView,
}

pub struct ExecutedWorldUpload {
    pub metadata: WorldUploadMetadata,
    pub resources: UploadedWorldResources,
}

#[allow(clippy::too_many_arguments)]
pub fn apply_world_upload_metadata(
    metadata: WorldUploadMetadata,
    chunk_count: &mut u32,
    chunk_map_size: &mut u32,
    chunk_map_mask: &mut u32,
    chunk_map_max_probe: &mut u32,
    chunk_map_avg_probe: &mut f32,
    chunk_map_max_probe_observed: &mut u32,
    chunk_map_load_factor: &mut f32,
    emissive_count: &mut u32,
    emissive_cdf_count: &mut u32,
    emissive_remap_count: &mut u32,
    importance_map_dims: &mut [u32; 3],
    emissive_signatures: &mut Vec<u32>,
    world_min: &mut [i32; 3],
    world_max: &mut [i32; 3],
) -> u32 {
    let WorldUploadMetadata {
        chunk_count: next_chunk_count,
        chunk_map_size: next_chunk_map_size,
        chunk_map_mask: next_chunk_map_mask,
        chunk_map_max_probe: next_chunk_map_max_probe,
        chunk_map_avg_probe: next_chunk_map_avg_probe,
        chunk_map_max_probe_observed: next_chunk_map_max_probe_observed,
        chunk_map_load_factor: next_chunk_map_load_factor,
        chunk_map_dropped_entries,
        emissive_count: next_emissive_count,
        emissive_cdf_count: next_emissive_cdf_count,
        emissive_remap_count: next_emissive_remap_count,
        emissive_signatures: next_emissive_signatures,
        importance_map_dims: next_importance_map_dims,
        world_min: next_world_min,
        world_max: next_world_max,
    } = metadata;

    *chunk_count = next_chunk_count;
    *chunk_map_size = next_chunk_map_size;
    *chunk_map_mask = next_chunk_map_mask;
    *chunk_map_max_probe = next_chunk_map_max_probe;
    *chunk_map_avg_probe = next_chunk_map_avg_probe;
    *chunk_map_max_probe_observed = next_chunk_map_max_probe_observed;
    *chunk_map_load_factor = next_chunk_map_load_factor;
    *emissive_count = next_emissive_count;
    *emissive_cdf_count = next_emissive_cdf_count;
    *emissive_remap_count = next_emissive_remap_count;
    *importance_map_dims = next_importance_map_dims;
    *emissive_signatures = next_emissive_signatures;
    *world_min = next_world_min;
    *world_max = next_world_max;
    chunk_map_dropped_entries
}

pub fn prepare_world_upload(payload: GpuWorldPayload, remap: Vec<u32>) -> WorldUploadPlan {
    let metadata = WorldUploadMetadata {
        chunk_count: payload.chunk_count().max(1),
        chunk_map_size: payload.chunk_map_size.max(1),
        chunk_map_mask: payload.chunk_map_mask,
        chunk_map_max_probe: payload.chunk_map_max_probe.max(1),
        chunk_map_avg_probe: payload.chunk_map_avg_probe,
        chunk_map_max_probe_observed: payload.chunk_map_max_probe_observed,
        chunk_map_load_factor: payload.chunk_map_load_factor,
        chunk_map_dropped_entries: payload.chunk_map_dropped_entries,
        emissive_count: payload.emissive_count,
        emissive_cdf_count: payload.emissive_cdf.len().max(1) as u32,
        emissive_remap_count: remap.len().max(1) as u32,
        emissive_signatures: payload.emissive_signatures.clone(),
        importance_map_dims: payload.importance_map_dims,
        world_min: payload.world_min,
        world_max: payload.world_max,
    };
    WorldUploadPlan {
        metadata,
        payload,
        remap,
    }
}

pub fn execute_world_upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    plan: WorldUploadPlan,
) -> ExecutedWorldUpload {
    let WorldUploadPlan {
        metadata,
        payload,
        remap,
    } = plan;

    let voxel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("voxel-buffer"),
        contents: cast_slice(&payload.voxel_words),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let chunk_meta_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chunk-meta-buffer"),
        contents: cast_slice(&payload.chunk_meta),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let chunk_map_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("chunk-map-buffer"),
        contents: cast_slice(&payload.chunk_map),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let emissive_voxel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("emissive-voxel-buffer"),
        contents: cast_slice(&payload.emissive_voxels),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let emissive_cdf_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("emissive-cdf-buffer"),
        contents: cast_slice(&payload.emissive_cdf),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let emissive_remap_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("emissive-remap-buffer"),
        contents: cast_slice(&remap),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let (importance_map_texture, importance_map_view) = create_importance_map_texture(
        device,
        queue,
        payload.importance_map_dims,
        &payload.importance_map_texels,
    );

    ExecutedWorldUpload {
        metadata,
        resources: UploadedWorldResources {
            voxel_buffer,
            chunk_meta_buffer,
            chunk_map_buffer,
            emissive_voxel_buffer,
            emissive_cdf_buffer,
            emissive_remap_buffer,
            importance_map_texture,
            importance_map_view,
        },
    }
}

pub fn create_importance_map_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    dims: [u32; 3],
    texels: &[f32],
) -> (wgpu::Texture, wgpu::TextureView) {
    let extent = wgpu::Extent3d {
        width: dims[0].max(1),
        height: dims[1].max(1),
        depth_or_array_layers: dims[2].max(1),
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("light-importance-map"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let row_bytes = extent.width * std::mem::size_of::<f32>() as u32;
    let aligned_row_bytes =
        row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let rows_per_image = extent.height;
    let slice_bytes = aligned_row_bytes * rows_per_image;
    let total_bytes = slice_bytes * extent.depth_or_array_layers;
    let mut staging = vec![0u8; total_bytes as usize];

    for z in 0..extent.depth_or_array_layers {
        for y in 0..extent.height {
            let src_start = ((z * extent.height + y) * extent.width) as usize;
            let src_end = src_start + extent.width as usize;
            let dst_start = (z * slice_bytes + y * aligned_row_bytes) as usize;
            let dst_end = dst_start + row_bytes as usize;
            let src = bytemuck::cast_slice::<f32, u8>(&texels[src_start..src_end]);
            staging[dst_start..dst_end].copy_from_slice(src);
        }
    }

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &staging,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(aligned_row_bytes),
            rows_per_image: Some(rows_per_image),
        },
        extent,
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::protocol::ChunkMetaGpu;

    #[test]
    fn prepare_world_upload_derives_metadata_from_payload() {
        let payload = GpuWorldPayload {
            chunk_map_size: 256,
            chunk_map_mask: 255,
            chunk_map_max_probe: 9,
            chunk_map_avg_probe: 1.75,
            chunk_map_max_probe_observed: 8,
            chunk_map_load_factor: 0.62,
            chunk_map_dropped_entries: 3,
            emissive_count: 12,
            emissive_cdf: vec![0.5, 1.0],
            emissive_signatures: vec![11, 22, 33],
            importance_map_dims: [4, 5, 6],
            world_min: [-8, -4, -2],
            world_max: [8, 4, 2],
            chunk_meta: vec![ChunkMetaGpu::empty(), ChunkMetaGpu::empty()],
            ..GpuWorldPayload::default()
        };
        let remap = vec![7, 8, 9];

        let plan = prepare_world_upload(payload, remap);
        let metadata = &plan.metadata;
        assert_eq!(metadata.chunk_count, 2);
        assert_eq!(metadata.chunk_map_size, 256);
        assert_eq!(metadata.chunk_map_mask, 255);
        assert_eq!(metadata.chunk_map_max_probe, 9);
        assert_eq!(metadata.chunk_map_avg_probe, 1.75);
        assert_eq!(metadata.chunk_map_max_probe_observed, 8);
        assert_eq!(metadata.chunk_map_load_factor, 0.62);
        assert_eq!(metadata.chunk_map_dropped_entries, 3);
        assert_eq!(metadata.emissive_count, 12);
        assert_eq!(metadata.emissive_cdf_count, 2);
        assert_eq!(metadata.emissive_remap_count, 3);
        assert_eq!(metadata.emissive_signatures, vec![11, 22, 33]);
        assert_eq!(metadata.importance_map_dims, [4, 5, 6]);
        assert_eq!(metadata.world_min, [-8, -4, -2]);
        assert_eq!(metadata.world_max, [8, 4, 2]);
    }

    #[test]
    fn prepare_world_upload_clamps_minimum_counts() {
        let payload = GpuWorldPayload::default();
        let remap = Vec::new();

        let plan = prepare_world_upload(payload, remap);
        let metadata = &plan.metadata;
        assert_eq!(metadata.chunk_count, 1);
        assert_eq!(metadata.chunk_map_size, 1);
        assert_eq!(metadata.chunk_map_max_probe, 1);
        assert_eq!(metadata.emissive_cdf_count, 1);
        assert_eq!(metadata.emissive_remap_count, 1);
    }

    #[test]
    fn prepare_world_upload_preserves_signatures_and_bounds() {
        let payload = GpuWorldPayload {
            emissive_signatures: vec![101, 202],
            world_min: [-64, -32, -16],
            world_max: [64, 32, 16],
            ..GpuWorldPayload::default()
        };

        let plan = prepare_world_upload(payload, vec![0]);
        let metadata = &plan.metadata;
        assert_eq!(metadata.emissive_signatures, vec![101, 202]);
        assert_eq!(metadata.world_min, [-64, -32, -16]);
        assert_eq!(metadata.world_max, [64, 32, 16]);
    }

    #[test]
    fn apply_world_upload_metadata_updates_renderer_fields() {
        let metadata = WorldUploadMetadata {
            chunk_count: 12,
            chunk_map_size: 128,
            chunk_map_mask: 127,
            chunk_map_max_probe: 5,
            chunk_map_avg_probe: 2.5,
            chunk_map_max_probe_observed: 4,
            chunk_map_load_factor: 0.77,
            chunk_map_dropped_entries: 3,
            emissive_count: 9,
            emissive_cdf_count: 10,
            emissive_remap_count: 11,
            emissive_signatures: vec![7, 8, 9],
            importance_map_dims: [3, 4, 5],
            world_min: [-1, -2, -3],
            world_max: [1, 2, 3],
        };
        let mut chunk_count = 0;
        let mut chunk_map_size = 1;
        let mut chunk_map_mask = 0;
        let mut chunk_map_max_probe = 1;
        let mut chunk_map_avg_probe = 0.0;
        let mut chunk_map_max_probe_observed = 0;
        let mut chunk_map_load_factor = 0.0;
        let mut emissive_count = 0;
        let mut emissive_cdf_count = 1;
        let mut emissive_remap_count = 1;
        let mut importance_map_dims = [1, 1, 1];
        let mut emissive_signatures = vec![42];
        let mut world_min = [0, 0, 0];
        let mut world_max = [0, 0, 0];

        let dropped_entries = apply_world_upload_metadata(
            metadata,
            &mut chunk_count,
            &mut chunk_map_size,
            &mut chunk_map_mask,
            &mut chunk_map_max_probe,
            &mut chunk_map_avg_probe,
            &mut chunk_map_max_probe_observed,
            &mut chunk_map_load_factor,
            &mut emissive_count,
            &mut emissive_cdf_count,
            &mut emissive_remap_count,
            &mut importance_map_dims,
            &mut emissive_signatures,
            &mut world_min,
            &mut world_max,
        );

        assert_eq!(chunk_count, 12);
        assert_eq!(chunk_map_size, 128);
        assert_eq!(chunk_map_mask, 127);
        assert_eq!(chunk_map_max_probe, 5);
        assert_eq!(chunk_map_avg_probe, 2.5);
        assert_eq!(chunk_map_max_probe_observed, 4);
        assert_eq!(chunk_map_load_factor, 0.77);
        assert_eq!(emissive_count, 9);
        assert_eq!(emissive_cdf_count, 10);
        assert_eq!(emissive_remap_count, 11);
        assert_eq!(importance_map_dims, [3, 4, 5]);
        assert_eq!(emissive_signatures, vec![7, 8, 9]);
        assert_eq!(world_min, [-1, -2, -3]);
        assert_eq!(world_max, [1, 2, 3]);
        assert_eq!(dropped_entries, 3);
    }

    #[test]
    fn apply_world_upload_metadata_replaces_previous_signatures() {
        let metadata = WorldUploadMetadata {
            chunk_count: 1,
            chunk_map_size: 1,
            chunk_map_mask: 0,
            chunk_map_max_probe: 1,
            chunk_map_avg_probe: 0.0,
            chunk_map_max_probe_observed: 0,
            chunk_map_load_factor: 0.0,
            chunk_map_dropped_entries: 0,
            emissive_count: 0,
            emissive_cdf_count: 1,
            emissive_remap_count: 1,
            emissive_signatures: vec![100, 200],
            importance_map_dims: [1, 1, 1],
            world_min: [0, 0, 0],
            world_max: [1, 1, 1],
        };

        let mut chunk_count = 0;
        let mut chunk_map_size = 0;
        let mut chunk_map_mask = 0;
        let mut chunk_map_max_probe = 0;
        let mut chunk_map_avg_probe = 0.0;
        let mut chunk_map_max_probe_observed = 0;
        let mut chunk_map_load_factor = 0.0;
        let mut emissive_count = 0;
        let mut emissive_cdf_count = 0;
        let mut emissive_remap_count = 0;
        let mut importance_map_dims = [0, 0, 0];
        let mut emissive_signatures = vec![1, 2, 3, 4];
        let mut world_min = [0, 0, 0];
        let mut world_max = [0, 0, 0];

        let _ = apply_world_upload_metadata(
            metadata,
            &mut chunk_count,
            &mut chunk_map_size,
            &mut chunk_map_mask,
            &mut chunk_map_max_probe,
            &mut chunk_map_avg_probe,
            &mut chunk_map_max_probe_observed,
            &mut chunk_map_load_factor,
            &mut emissive_count,
            &mut emissive_cdf_count,
            &mut emissive_remap_count,
            &mut importance_map_dims,
            &mut emissive_signatures,
            &mut world_min,
            &mut world_max,
        );

        assert_eq!(emissive_signatures, vec![100, 200]);
    }
}
