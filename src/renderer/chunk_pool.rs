use std::{collections::HashMap, mem::size_of};

use anyhow::{Result, anyhow};
use ash::vk;
use bytemuck::cast_slice;
use gpu_allocator::{MemoryLocation, vulkan::{Allocation, AllocationScheme}};

use crate::{
    meshing::{PackedMesh, PackedVertex},
    streaming::types::{CHUNK_EDGE, ChunkKey},
};

use super::{Renderer, create_allocated_buffer, destroy_allocated_buffer};

pub const MAX_RENDER_CHUNKS: usize = 881;
pub const MAX_QUADS_PER_CHUNK: usize = 4096;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ChunkDrawMetadata {
    pub aabb_min: [f32; 3],
    pub slot_id: u32,
    pub aabb_max: [f32; 3],
    pub first_index: u32,
    pub vertex_offset: i32,
    pub index_count: u32,
    pub lod_level: u32,
    pub _padding0: u32,
    pub chunk_origin: [f32; 3],
    pub chunk_scale: f32,
}

pub struct SlotUpload {
    pub slot_id: u32,
    pub vertex_offset_bytes: vk::DeviceSize,
    pub index_offset_bytes: vk::DeviceSize,
    pub vertex_bytes: Box<[u8]>,
    pub index_bytes: Box<[u8]>,
    pub metadata: ChunkDrawMetadata,
    pub indirect: vk::DrawIndexedIndirectCommand,
}

pub struct SlotAllocator {
    chunk_to_slot: HashMap<ChunkKey, u32>,
    slot_to_chunk: Vec<Option<ChunkKey>>,
    free_slots: Vec<u32>,
    slot_to_draw_index: Vec<Option<u32>>,
    draw_index_to_slot: Vec<u32>,
    metadata_shadow: Vec<ChunkDrawMetadata>,
    indirect_shadow: Vec<vk::DrawIndexedIndirectCommand>,
}

impl SlotAllocator {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            chunk_to_slot: HashMap::new(),
            slot_to_chunk: vec![None; capacity],
            free_slots: (0..capacity as u32).rev().collect(),
            slot_to_draw_index: vec![None; capacity],
            draw_index_to_slot: Vec::with_capacity(capacity),
            metadata_shadow: vec![ChunkDrawMetadata::default(); capacity],
            indirect_shadow: vec![vk::DrawIndexedIndirectCommand::default(); capacity],
        }
    }

    pub fn prepare_upload(&mut self, key: ChunkKey, mesh: &PackedMesh) -> Result<SlotUpload> {
        let slot_id = match self.chunk_to_slot.get(&key).copied() {
            Some(slot_id) => {
                self.slot_to_draw_index[slot_id as usize]
                    .ok_or_else(|| anyhow!("slot {slot_id} is active but missing a draw index"))?;
                slot_id
            }
            None => {
                let slot_id = self
                    .free_slots
                    .pop()
                    .ok_or_else(|| anyhow!("chunk pool exhausted at {} slots", self.slot_to_chunk.len()))?;
                self.chunk_to_slot.insert(key, slot_id);
                self.slot_to_chunk[slot_id as usize] = Some(key);
                let draw_index = self.draw_index_to_slot.len() as u32;
                self.slot_to_draw_index[slot_id as usize] = Some(draw_index);
                self.draw_index_to_slot.push(slot_id);
                slot_id
            }
        };

        let first_index = slot_id * index_slot_stride_indices() as u32;
        let vertex_offset = (slot_id * vertex_slot_stride_vertices() as u32) as i32;
        let chunk_scale = lod_scale(key.lod_level);
        let chunk_origin = chunk_origin(key, chunk_scale);
        let metadata = ChunkDrawMetadata {
            aabb_min: world_aabb(mesh.aabb_min, chunk_origin, chunk_scale),
            slot_id,
            aabb_max: world_aabb(mesh.aabb_max, chunk_origin, chunk_scale),
            first_index,
            vertex_offset,
            index_count: mesh.indices.len() as u32,
            lod_level: u32::from(key.lod_level),
            _padding0: 0,
            chunk_origin,
            chunk_scale,
        };
        let indirect = vk::DrawIndexedIndirectCommand {
            index_count: mesh.indices.len() as u32,
            instance_count: 1,
            first_index,
            vertex_offset,
            first_instance: slot_id,
        };

        self.metadata_shadow[slot_id as usize] = metadata;
        self.indirect_shadow[slot_id as usize] = indirect;

        let vertex_bytes = cast_slice(mesh.vertices.as_ref()).to_vec().into_boxed_slice();
        let index_bytes = cast_slice(mesh.indices.as_ref()).to_vec().into_boxed_slice();

        Ok(SlotUpload {
            slot_id,
            vertex_offset_bytes: u64::from(slot_id) * vertex_slot_stride_bytes() as u64,
            index_offset_bytes: u64::from(slot_id) * index_slot_stride_bytes() as u64,
            vertex_bytes,
            index_bytes,
            metadata,
            indirect,
        })
    }

    pub fn prepare_remove(&mut self, key: ChunkKey) -> Option<u32> {
        let slot_id = self.chunk_to_slot.remove(&key)?;
        self.slot_to_chunk[slot_id as usize] = None;
        let draw_index = self.slot_to_draw_index[slot_id as usize].take()?;
        let removed_draw_index = draw_index as usize;
        let last_slot = self.draw_index_to_slot.pop()?;
        if removed_draw_index < self.draw_index_to_slot.len() {
            self.draw_index_to_slot[removed_draw_index] = last_slot;
            self.slot_to_draw_index[last_slot as usize] = Some(draw_index);
        }
        self.free_slots.push(slot_id);
        self.metadata_shadow[slot_id as usize] = ChunkDrawMetadata::default();
        self.indirect_shadow[slot_id as usize] = vk::DrawIndexedIndirectCommand::default();
        Some(slot_id)
    }

    pub fn slot_for(&self, key: ChunkKey) -> Option<u32> {
        self.chunk_to_slot.get(&key).copied()
    }

    pub fn metadata_shadow(&self) -> &[ChunkDrawMetadata] {
        &self.metadata_shadow
    }

    pub fn indirect_shadow(&self) -> &[vk::DrawIndexedIndirectCommand] {
        &self.indirect_shadow
    }

    pub fn active_chunk_count(&self) -> u32 {
        self.chunk_to_slot.len() as u32
    }

    pub fn active_draw_count(&self) -> u32 {
        self.draw_index_to_slot.len() as u32
    }

    pub fn draw_slots_shadow(&self) -> &[u32] {
        &self.draw_index_to_slot
    }

    pub fn draw_index_for_slot(&self, slot_id: u32) -> Option<u32> {
        self.slot_to_draw_index.get(slot_id as usize).copied().flatten()
    }
}

pub struct ChunkPool {
    vertex_buffer: vk::Buffer,
    vertex_allocation: Option<Allocation>,
    index_buffer: vk::Buffer,
    index_allocation: Option<Allocation>,
    metadata_buffer: vk::Buffer,
    metadata_allocation: Option<Allocation>,
    indirect_buffer: vk::Buffer,
    indirect_allocation: Option<Allocation>,
    slot_allocator: SlotAllocator,
}

impl ChunkPool {
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let (vertex_buffer, vertex_allocation) = create_allocated_buffer(
            renderer,
            (vertex_slot_stride_bytes() * MAX_RENDER_CHUNKS) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-vertex",
        )?;
        let (index_buffer, index_allocation) = create_allocated_buffer(
            renderer,
            (index_slot_stride_bytes() * MAX_RENDER_CHUNKS) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::INDEX_BUFFER,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-index",
        )?;
        let (metadata_buffer, metadata_allocation) = create_allocated_buffer(
            renderer,
            (size_of::<ChunkDrawMetadata>() * MAX_RENDER_CHUNKS) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::STORAGE_BUFFER,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-metadata",
        )?;
        let (indirect_buffer, indirect_allocation) = create_allocated_buffer(
            renderer,
            (size_of::<vk::DrawIndexedIndirectCommand>() * MAX_RENDER_CHUNKS) as u64,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::INDIRECT_BUFFER,
            MemoryLocation::CpuToGpu,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-indirect",
        )?;

        Ok(Self {
            vertex_buffer,
            vertex_allocation: Some(vertex_allocation),
            index_buffer,
            index_allocation: Some(index_allocation),
            metadata_buffer,
            metadata_allocation: Some(metadata_allocation),
            indirect_buffer,
            indirect_allocation: Some(indirect_allocation),
            slot_allocator: SlotAllocator::with_capacity(MAX_RENDER_CHUNKS),
        })
    }

    pub fn prepare_upload(&mut self, key: ChunkKey, mesh: &PackedMesh) -> Result<SlotUpload> {
        self.slot_allocator.prepare_upload(key, mesh)
    }

    pub fn prepare_remove(&mut self, key: ChunkKey) -> Option<u32> {
        self.slot_allocator.prepare_remove(key)
    }

    pub fn active_chunk_count(&self) -> u32 {
        self.slot_allocator.active_chunk_count()
    }

    pub fn active_draw_count(&self) -> u32 {
        self.slot_allocator.active_draw_count()
    }

    pub fn indirect_buffer(&self) -> vk::Buffer {
        self.indirect_buffer
    }

    pub fn metadata_buffer(&self) -> vk::Buffer {
        self.metadata_buffer
    }

    pub fn vertex_buffer(&self) -> vk::Buffer {
        self.vertex_buffer
    }

    pub fn index_buffer(&self) -> vk::Buffer {
        self.index_buffer
    }

    pub fn slot_allocator(&self) -> &SlotAllocator {
        &self.slot_allocator
    }

    pub fn apply_upload(&mut self, upload: SlotUpload) -> Result<()> {
        write_allocation_bytes(
            self.vertex_allocation.as_mut(),
            upload.vertex_offset_bytes as usize,
            &upload.vertex_bytes,
        )?;
        write_allocation_bytes(
            self.index_allocation.as_mut(),
            upload.index_offset_bytes as usize,
            &upload.index_bytes,
        )?;

        let metadata = [upload.metadata];
        let metadata_bytes = cast_slice(&metadata);
        write_allocation_bytes(
            self.metadata_allocation.as_mut(),
            upload.slot_id as usize * size_of::<ChunkDrawMetadata>(),
            metadata_bytes,
        )?;

        write_allocation_bytes(
            self.indirect_allocation.as_mut(),
            upload.slot_id as usize * size_of::<vk::DrawIndexedIndirectCommand>(),
            struct_as_bytes(&upload.indirect),
        )?;

        Ok(())
    }

    pub fn clear_slot(&mut self, slot_id: u32) -> Result<()> {
        write_allocation_bytes(
            self.vertex_allocation.as_mut(),
            slot_id as usize * vertex_slot_stride_bytes(),
            &vec![0_u8; vertex_slot_stride_bytes()],
        )?;
        write_allocation_bytes(
            self.index_allocation.as_mut(),
            slot_id as usize * index_slot_stride_bytes(),
            &vec![0_u8; index_slot_stride_bytes()],
        )?;
        write_allocation_bytes(
            self.metadata_allocation.as_mut(),
            slot_id as usize * size_of::<ChunkDrawMetadata>(),
            cast_slice(&[ChunkDrawMetadata::default()]),
        )?;
        write_allocation_bytes(
            self.indirect_allocation.as_mut(),
            slot_id as usize * size_of::<vk::DrawIndexedIndirectCommand>(),
            struct_as_bytes(&vk::DrawIndexedIndirectCommand::default()),
        )?;
        Ok(())
    }

    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        if let Some(allocation) = self.indirect_allocation.take() {
            destroy_allocated_buffer(renderer, self.indirect_buffer, allocation)?;
        }
        if let Some(allocation) = self.metadata_allocation.take() {
            destroy_allocated_buffer(renderer, self.metadata_buffer, allocation)?;
        }
        if let Some(allocation) = self.index_allocation.take() {
            destroy_allocated_buffer(renderer, self.index_buffer, allocation)?;
        }
        if let Some(allocation) = self.vertex_allocation.take() {
            destroy_allocated_buffer(renderer, self.vertex_buffer, allocation)?;
        }
        Ok(())
    }
}

fn vertex_slot_stride_vertices() -> usize {
    MAX_QUADS_PER_CHUNK * 4
}

fn index_slot_stride_indices() -> usize {
    MAX_QUADS_PER_CHUNK * 6
}

fn vertex_slot_stride_bytes() -> usize {
    vertex_slot_stride_vertices() * size_of::<PackedVertex>()
}

fn index_slot_stride_bytes() -> usize {
    index_slot_stride_indices() * size_of::<u32>()
}

fn lod_scale(lod_level: u8) -> f32 {
    (1_u32 << lod_level) as f32
}

fn chunk_origin(key: ChunkKey, chunk_scale: f32) -> [f32; 3] {
    let chunk_world_edge = CHUNK_EDGE as f32 * chunk_scale;
    [
        key.x as f32 * chunk_world_edge,
        key.y as f32 * chunk_world_edge,
        key.z as f32 * chunk_world_edge,
    ]
}

fn world_aabb(local: [f32; 3], origin: [f32; 3], chunk_scale: f32) -> [f32; 3] {
    [
        origin[0] + local[0] * chunk_scale,
        origin[1] + local[1] * chunk_scale,
        origin[2] + local[2] * chunk_scale,
    ]
}

fn write_allocation_bytes(
    allocation: Option<&mut Allocation>,
    offset: usize,
    bytes: &[u8],
) -> Result<()> {
    let allocation = allocation.ok_or_else(|| anyhow!("missing chunk pool allocation"))?;
    let mapped = allocation
        .mapped_slice_mut()
        .ok_or_else(|| anyhow!("chunk pool allocation is not CPU-visible"))?;
    let end = offset + bytes.len();
    if end > mapped.len() {
        return Err(anyhow!("chunk pool write exceeds allocation bounds"));
    }
    mapped[offset..end].copy_from_slice(bytes);
    Ok(())
}

fn struct_as_bytes<T>(value: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((value as *const T).cast::<u8>(), size_of::<T>()) }
}
