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
use super::staging_ring::StagingRing;

pub const INITIAL_CAPACITY: usize = 1024;

/// Growth threshold factor: grow when active > capacity * GROW_THRESHOLD.
const GROW_THRESHOLD: f64 = 0.9;
pub const MAX_QUADS_PER_CHUNK: usize = 4096;

/// Per-chunk GPU instance data in the unified scene_buffer (48 bytes, #[repr(C)]).
///
/// Replaces ChunkDrawMetadata. Used by the vertex shader via `gl_InstanceIndex`
/// (= firstInstance = slot_id) and by the cull compute shader.
/// Stored in region 0 of scene_buffer.
///
/// Layout (D-02):
///   aabb_min:      [f32; 3]  — 12 bytes
///   material_id:   u32       —  4 bytes (used in Plan 04 for material lookup)
///   aabb_max:      [f32; 3]  — 12 bytes
///   lod_level:     u32       —  4 bytes
///   chunk_origin:  [f32; 3]  — 12 bytes
///   chunk_scale:   f32       —  4 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuChunkInstance {
    pub aabb_min: [f32; 3],
    pub material_id: u32,
    pub aabb_max: [f32; 3],
    pub lod_level: u32,
    pub chunk_origin: [f32; 3],
    pub chunk_scale: f32,
}

/// Legacy metadata struct — retained during migration. Will be removed in a future plan.
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

/// Calculate byte offsets for the 4 regions of the unified scene_buffer.
///
/// Layout (D-01):
///   Region 0: GpuChunkInstance\[capacity\]           (48 bytes each)
///   Region 1: DrawIndexedIndirectCommand\[capacity\]  (20 bytes each)
///   Region 2: u32\[capacity\]                         (4 bytes each)
///   Region 3: DrawIndexedIndirectCommand\[capacity\]  (20 bytes each)
///
/// All region boundaries are 16-byte aligned.
///
/// Returns `(instance_offset, indirect_template_offset, draw_slot_offset, dense_indirect_offset, total_size)`.
pub fn scene_buffer_region_offsets(capacity: usize) -> (u64, u64, u64, u64, u64) {
    fn align_up(offset: u64, alignment: u64) -> u64 {
        (offset + alignment - 1) & !(alignment - 1)
    }

    const INSTANCE_STRIDE: usize = size_of::<GpuChunkInstance>(); // 48
    const INDIRECT_STRIDE: usize = 20; // DrawIndexedIndirectCommand: 5 * u32
    const SLOT_STRIDE: usize = size_of::<u32>(); // 4

    let instance_offset = 0u64;
    let instance_size = (capacity * INSTANCE_STRIDE) as u64;

    let indirect_template_offset = align_up(instance_offset + instance_size, 16);
    let indirect_template_size = (capacity * INDIRECT_STRIDE) as u64;

    let draw_slot_offset = align_up(indirect_template_offset + indirect_template_size, 16);
    let draw_slot_size = (capacity * SLOT_STRIDE) as u64;

    let dense_indirect_offset = align_up(draw_slot_offset + draw_slot_size, 16);
    let dense_indirect_size = (capacity * INDIRECT_STRIDE) as u64;

    let total_size = dense_indirect_offset + dense_indirect_size;

    (
        instance_offset,
        indirect_template_offset,
        draw_slot_offset,
        dense_indirect_offset,
        total_size,
    )
}

pub struct SlotUpload {
    pub slot_id: u32,
    pub draw_slot_write: Option<DrawSlotWrite>,
    pub dense_indirect_write: DenseIndirectWrite,
    pub vertex_offset_bytes: vk::DeviceSize,
    pub index_offset_bytes: vk::DeviceSize,
    pub vertex_bytes: Box<[u8]>,
    pub index_bytes: Box<[u8]>,
    pub instance: GpuChunkInstance,
    pub indirect: vk::DrawIndexedIndirectCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawSlotWrite {
    pub draw_index: u32,
    pub slot_id: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct DenseIndirectWrite {
    pub draw_index: u32,
    pub command: vk::DrawIndexedIndirectCommand,
}

pub struct SlotRemove {
    pub slot_id: u32,
    pub draw_slot_writes: Vec<DrawSlotWrite>,
    pub dense_indirect_writes: Vec<DenseIndirectWrite>,
}

pub struct SlotAllocator {
    chunk_to_slot: HashMap<ChunkKey, u32>,
    slot_to_chunk: Vec<Option<ChunkKey>>,
    free_slots: Vec<u32>,
    slot_to_draw_index: Vec<Option<u32>>,
    draw_index_to_slot: Vec<u32>,
    instance_shadow: Vec<GpuChunkInstance>,
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
            instance_shadow: vec![GpuChunkInstance::default(); capacity],
            indirect_shadow: vec![vk::DrawIndexedIndirectCommand::default(); capacity],
        }
    }

    pub fn prepare_upload(&mut self, key: ChunkKey, mesh: &PackedMesh) -> Result<SlotUpload> {
        let (slot_id, draw_index, draw_slot_write) = match self.chunk_to_slot.get(&key).copied() {
            Some(slot_id) => {
                let draw_index = self.slot_to_draw_index[slot_id as usize]
                    .ok_or_else(|| anyhow!("slot {slot_id} is active but missing a draw index"))?;
                (slot_id, draw_index, None)
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
                (
                    slot_id,
                    draw_index,
                    Some(DrawSlotWrite {
                        draw_index,
                        slot_id,
                    }),
                )
            }
        };

        let first_index = slot_id * index_slot_stride_indices() as u32;
        let vertex_offset = (slot_id * vertex_slot_stride_vertices() as u32) as i32;
        let chunk_scale = lod_scale(key.lod_level);
        let chunk_origin = chunk_origin(key, chunk_scale);

        let instance = GpuChunkInstance {
            aabb_min: world_aabb(mesh.aabb_min, chunk_origin, chunk_scale),
            material_id: 0, // filled by material system (Plan 04)
            aabb_max: world_aabb(mesh.aabb_max, chunk_origin, chunk_scale),
            lod_level: u32::from(key.lod_level),
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

        self.instance_shadow[slot_id as usize] = instance;
        self.indirect_shadow[slot_id as usize] = indirect;

        let vertex_bytes = cast_slice(mesh.vertices.as_ref()).to_vec().into_boxed_slice();
        let index_bytes = cast_slice(mesh.indices.as_ref()).to_vec().into_boxed_slice();

        Ok(SlotUpload {
            slot_id,
            draw_slot_write,
            dense_indirect_write: DenseIndirectWrite {
                draw_index,
                command: indirect,
            },
            vertex_offset_bytes: u64::from(slot_id) * vertex_slot_stride_bytes() as u64,
            index_offset_bytes: u64::from(slot_id) * index_slot_stride_bytes() as u64,
            vertex_bytes,
            index_bytes,
            instance,
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
        self.instance_shadow[slot_id as usize] = GpuChunkInstance::default();
        self.indirect_shadow[slot_id as usize] = vk::DrawIndexedIndirectCommand::default();
        Some(slot_id)
    }

    pub fn slot_for(&self, key: ChunkKey) -> Option<u32> {
        self.chunk_to_slot.get(&key).copied()
    }

    pub fn instance_shadow(&self) -> &[GpuChunkInstance] {
        &self.instance_shadow
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

    /// Current capacity of this allocator.
    pub fn capacity(&self) -> usize {
        self.slot_to_chunk.len()
    }

    /// Grow the allocator to `new_capacity`, extending all internal vectors
    /// and adding new free slots from `old_capacity..new_capacity` (D-08).
    pub fn grow(&mut self, new_capacity: usize) {
        let old_capacity = self.slot_to_chunk.len();
        assert!(new_capacity > old_capacity, "grow: new_capacity must exceed old");
        self.slot_to_chunk.resize(new_capacity, None);
        self.slot_to_draw_index.resize(new_capacity, None);
        self.instance_shadow.resize(new_capacity, GpuChunkInstance::default());
        self.indirect_shadow.resize(new_capacity, vk::DrawIndexedIndirectCommand::default());
        // Add new free slots in reverse order so that the lowest new slot is popped first.
        for slot in (old_capacity as u32..new_capacity as u32).rev() {
            self.free_slots.push(slot);
        }
    }
}

/// Unified chunk pool with 3 GPU buffers: vertex, index, scene_buffer.
///
/// The scene_buffer is a single SSBO containing 4 contiguous regions (D-01):
///   Region 0: GpuChunkInstance\[capacity\]
///   Region 1: DrawIndexedIndirectCommand\[capacity\] (indirect templates)
///   Region 2: u32\[capacity\] (draw slot mapping)
///   Region 3: DrawIndexedIndirectCommand\[capacity\] (dense indirect output)
pub struct ChunkPool {
    vertex_buffer: vk::Buffer,
    vertex_allocation: Option<Allocation>,
    index_buffer: vk::Buffer,
    index_allocation: Option<Allocation>,
    /// Unified scene buffer containing all per-chunk metadata and indirect data (D-05).
    scene_buffer: vk::Buffer,
    scene_allocation: Option<Allocation>,
    /// Pool capacity (number of chunk slots).
    capacity: usize,
    dense_indirect_shadow: Vec<vk::DrawIndexedIndirectCommand>,
    slot_allocator: SlotAllocator,
}

impl ChunkPool {
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let capacity = INITIAL_CAPACITY;

        let (vertex_buffer, vertex_allocation) = create_allocated_buffer(
            renderer,
            (vertex_slot_stride_bytes() * capacity) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-vertex",
        )?;
        let (index_buffer, index_allocation) = create_allocated_buffer(
            renderer,
            (index_slot_stride_bytes() * capacity) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::INDEX_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-index",
        )?;

        // Unified scene_buffer — TRANSFER_DST | STORAGE_BUFFER | INDIRECT_BUFFER (D-05)
        let (_, _, _, _, total_scene_size) = scene_buffer_region_offsets(capacity);
        let (scene_buffer, scene_allocation) = create_allocated_buffer(
            renderer,
            total_scene_size,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::INDIRECT_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-scene",
        )?;

        Ok(Self {
            vertex_buffer,
            vertex_allocation: Some(vertex_allocation),
            index_buffer,
            index_allocation: Some(index_allocation),
            scene_buffer,
            scene_allocation: Some(scene_allocation),
            capacity,
            dense_indirect_shadow: vec![vk::DrawIndexedIndirectCommand::default(); capacity],
            slot_allocator: SlotAllocator::with_capacity(capacity),
        })
    }

    pub fn prepare_upload(&mut self, key: ChunkKey, mesh: &PackedMesh) -> Result<SlotUpload> {
        self.slot_allocator.prepare_upload(key, mesh)
    }

    pub fn prepare_remove(&mut self, key: ChunkKey) -> Option<SlotRemove> {
        let slot_id = self.slot_allocator.slot_for(key)?;
        let draw_index = self.slot_allocator.draw_index_for_slot(slot_id)?;
        let last_draw_index = self.slot_allocator.active_draw_count().checked_sub(1)?;
        let moved_slot = if draw_index != last_draw_index {
            self.slot_allocator
                .draw_slots_shadow()
                .get(last_draw_index as usize)
                .copied()
        } else {
            None
        };

        self.slot_allocator.prepare_remove(key)?;

        let mut draw_slot_writes = Vec::with_capacity(if moved_slot.is_some() { 2 } else { 1 });
        let mut dense_indirect_writes =
            Vec::with_capacity(if moved_slot.is_some() { 2 } else { 1 });
        if let Some(moved_slot) = moved_slot {
            draw_slot_writes.push(DrawSlotWrite {
                draw_index,
                slot_id: moved_slot,
            });
            dense_indirect_writes.push(DenseIndirectWrite {
                draw_index,
                command: self.slot_allocator.indirect_shadow()[moved_slot as usize],
            });
        }
        draw_slot_writes.push(DrawSlotWrite {
            draw_index: last_draw_index,
            slot_id: 0,
        });
        dense_indirect_writes.push(DenseIndirectWrite {
            draw_index: last_draw_index,
            command: vk::DrawIndexedIndirectCommand::default(),
        });

        Some(SlotRemove {
            slot_id,
            draw_slot_writes,
            dense_indirect_writes,
        })
    }

    pub fn active_chunk_count(&self) -> u32 {
        self.slot_allocator.active_chunk_count()
    }

    pub fn active_draw_count(&self) -> u32 {
        self.slot_allocator.active_draw_count()
    }

    // ---- Buffer accessors ----

    pub fn vertex_buffer(&self) -> vk::Buffer {
        self.vertex_buffer
    }

    pub fn index_buffer(&self) -> vk::Buffer {
        self.index_buffer
    }

    /// Return the unified scene_buffer handle (D-01).
    pub fn scene_buffer(&self) -> vk::Buffer {
        self.scene_buffer
    }

    /// Return the pool capacity (number of chunk slots).
    pub fn scene_buffer_capacity(&self) -> usize {
        self.capacity
    }

    /// Byte offset of the dense indirect region within scene_buffer.
    pub fn dense_indirect_region_offset(&self) -> vk::DeviceSize {
        let (_, _, _, dense_off, _) = scene_buffer_region_offsets(self.capacity);
        dense_off
    }

    /// Compatibility accessor: returns scene_buffer (dense indirect region lives within it).
    pub fn dense_indirect_buffer(&self) -> vk::Buffer {
        self.scene_buffer
    }

    /// Compatibility accessor: returns scene_buffer (metadata/instance region lives within it).
    pub fn metadata_buffer(&self) -> vk::Buffer {
        self.scene_buffer
    }

    /// Compatibility accessor: returns scene_buffer (indirect template region lives within it).
    pub fn indirect_template_buffer(&self) -> vk::Buffer {
        self.scene_buffer
    }

    /// Compatibility accessor: returns scene_buffer (draw slot region lives within it).
    pub fn draw_slot_buffer(&self) -> vk::Buffer {
        self.scene_buffer
    }

    pub fn slot_allocator(&self) -> &SlotAllocator {
        &self.slot_allocator
    }

    /// Current capacity (number of chunk slots).
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns true when active chunks exceed 90% of capacity (D-02).
    pub fn needs_grow(&self) -> bool {
        let active = self.slot_allocator.active_chunk_count() as f64;
        let threshold = self.capacity as f64 * GROW_THRESHOLD;
        active > threshold
    }

    /// Grow capacity by 2× — allocate new buffers, copy old data, destroy old,
    /// update bindless descriptor bindings, and grow the slot allocator (D-04, D-05).
    ///
    /// Must be called between frames (after fence wait), never mid-command-buffer.
    pub fn grow_capacity(
        &mut self,
        renderer: &mut Renderer,
        bindless: &super::bindless::BindlessTable,
    ) -> Result<()> {
        let old_capacity = self.capacity;
        let new_capacity = old_capacity * 2;
        log::info!("ChunkPool: growing capacity {old_capacity} → {new_capacity}");

        // Allocate new vertex buffer
        let (new_vertex_buffer, new_vertex_allocation) = create_allocated_buffer(
            renderer,
            (vertex_slot_stride_bytes() * new_capacity) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-vertex",
        )?;

        // Allocate new index buffer
        let (new_index_buffer, new_index_allocation) = create_allocated_buffer(
            renderer,
            (index_slot_stride_bytes() * new_capacity) as u64,
            vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::INDEX_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-index",
        )?;

        // Allocate new scene buffer
        let (_, _, _, _, new_total_scene_size) = scene_buffer_region_offsets(new_capacity);
        let (new_scene_buffer, new_scene_allocation) = create_allocated_buffer(
            renderer,
            new_total_scene_size,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::INDIRECT_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-scene",
        )?;

        // Copy old data to new buffers via one-shot command buffer with fence wait.
        let old_vertex = self.vertex_buffer;
        let old_index = self.index_buffer;
        let old_scene = self.scene_buffer;
        let old_vertex_size = (vertex_slot_stride_bytes() * old_capacity) as u64;
        let old_index_size = (index_slot_stride_bytes() * old_capacity) as u64;
        let (_, _, _, _, old_total_scene_size) = scene_buffer_region_offsets(old_capacity);

        super::helpers::submit_one_shot_commands(renderer, |device, cmd| {
            let vertex_copy = vk::BufferCopy::default().size(old_vertex_size);
            let index_copy = vk::BufferCopy::default().size(old_index_size);
            let scene_copy = vk::BufferCopy::default().size(old_total_scene_size);
            unsafe {
                device.cmd_copy_buffer(cmd, old_vertex, new_vertex_buffer, &[vertex_copy]);
                device.cmd_copy_buffer(cmd, old_index, new_index_buffer, &[index_copy]);
                device.cmd_copy_buffer(cmd, old_scene, new_scene_buffer, &[scene_copy]);
            }
            Ok(())
        })?;

        // Destroy old buffers
        if let Some(allocation) = self.scene_allocation.take() {
            destroy_allocated_buffer(renderer, self.scene_buffer, allocation)?;
        }
        if let Some(allocation) = self.index_allocation.take() {
            destroy_allocated_buffer(renderer, self.index_buffer, allocation)?;
        }
        if let Some(allocation) = self.vertex_allocation.take() {
            destroy_allocated_buffer(renderer, self.vertex_buffer, allocation)?;
        }

        // Install new buffers
        self.vertex_buffer = new_vertex_buffer;
        self.vertex_allocation = Some(new_vertex_allocation);
        self.index_buffer = new_index_buffer;
        self.index_allocation = Some(new_index_allocation);
        self.scene_buffer = new_scene_buffer;
        self.scene_allocation = Some(new_scene_allocation);
        self.capacity = new_capacity;
        self.dense_indirect_shadow.resize(new_capacity, vk::DrawIndexedIndirectCommand::default());

        // Grow slot allocator
        self.slot_allocator.grow(new_capacity);

        // Update BindlessTable binding 0 to point to the new scene_buffer (D-05)
        bindless.register_buffer(&renderer.device_ctx.device, 0, self.scene_buffer, vk::WHOLE_SIZE);

        log::info!("ChunkPool: growth complete, new capacity = {new_capacity}");
        Ok(())
    }

    /// Record vkCmdCopyBuffer commands for an upload, writing data through the staging ring.
    ///
    /// Writes to the unified scene_buffer at the correct region offsets (D-06).
    pub fn record_upload(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        upload: SlotUpload,
    ) -> Result<()> {
        let SlotUpload {
            slot_id,
            draw_slot_write,
            dense_indirect_write,
            vertex_offset_bytes,
            index_offset_bytes,
            vertex_bytes,
            index_bytes,
            instance,
            indirect,
        } = upload;

        let (inst_off, indirect_off, slot_off, dense_off, _) =
            scene_buffer_region_offsets(self.capacity);

        // Copy vertex data via staging
        if !vertex_bytes.is_empty() {
            let mut alloc = staging_ring.allocate(vertex_bytes.len() as u64, 16)?;
            alloc.write_bytes(&vertex_bytes);
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(vertex_offset_bytes)
                .size(vertex_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.vertex_buffer, &[region]);
            }
        }

        // Copy index data via staging
        if !index_bytes.is_empty() {
            let mut alloc = staging_ring.allocate(index_bytes.len() as u64, 4)?;
            alloc.write_bytes(&index_bytes);
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(index_offset_bytes)
                .size(index_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.index_buffer, &[region]);
            }
        }

        // Copy GpuChunkInstance to scene_buffer instance region (region 0)
        {
            let instance_arr = [instance];
            let instance_bytes = cast_slice(&instance_arr);
            let mut alloc = staging_ring.allocate(instance_bytes.len() as u64, 16)?;
            alloc.write_bytes(instance_bytes);
            let dst_offset = inst_off + slot_id as u64 * size_of::<GpuChunkInstance>() as u64;
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(dst_offset)
                .size(instance_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.scene_buffer, &[region]);
            }
        }

        // Copy indirect template to scene_buffer indirect template region (region 1)
        {
            let indirect_bytes = draw_cmd_as_bytes(&indirect);
            let mut alloc = staging_ring.allocate(indirect_bytes.len() as u64, 4)?;
            alloc.write_bytes(indirect_bytes);
            let dst_offset =
                indirect_off + slot_id as u64 * size_of::<vk::DrawIndexedIndirectCommand>() as u64;
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(dst_offset)
                .size(indirect_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.scene_buffer, &[region]);
            }
        }

        // Copy draw slot mapping via staging (region 2)
        if let Some(dsw) = draw_slot_write {
            self.record_draw_slot_copy(device, cmd, staging_ring, slot_off, dsw)?;
        }

        // Copy dense indirect entry via staging (region 3)
        self.record_dense_indirect_copy(device, cmd, staging_ring, dense_off, dense_indirect_write)?;

        Ok(())
    }

    /// Record vkCmdCopyBuffer commands for a remove operation via staging ring.
    ///
    /// Zeroes the appropriate regions in scene_buffer (D-06).
    pub fn record_remove(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        remove: SlotRemove,
    ) -> Result<()> {
        let (inst_off, indirect_off, slot_off, dense_off, _) =
            scene_buffer_region_offsets(self.capacity);

        // Zero out the slot's vertex data
        {
            let zero_bytes = vec![0_u8; vertex_slot_stride_bytes()];
            let mut alloc = staging_ring.allocate(zero_bytes.len() as u64, 16)?;
            alloc.write_bytes(&zero_bytes);
            let dst_offset = remove.slot_id as u64 * vertex_slot_stride_bytes() as u64;
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(dst_offset)
                .size(zero_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.vertex_buffer, &[region]);
            }
        }

        // Zero out the slot's index data
        {
            let zero_bytes = vec![0_u8; index_slot_stride_bytes()];
            let mut alloc = staging_ring.allocate(zero_bytes.len() as u64, 4)?;
            alloc.write_bytes(&zero_bytes);
            let dst_offset = remove.slot_id as u64 * index_slot_stride_bytes() as u64;
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(dst_offset)
                .size(zero_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.index_buffer, &[region]);
            }
        }

        // Zero out GpuChunkInstance in scene_buffer (region 0)
        {
            let zero_instance = [GpuChunkInstance::default()];
            let instance_bytes = cast_slice(&zero_instance);
            let mut alloc = staging_ring.allocate(instance_bytes.len() as u64, 16)?;
            alloc.write_bytes(instance_bytes);
            let dst_offset =
                inst_off + remove.slot_id as u64 * size_of::<GpuChunkInstance>() as u64;
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(dst_offset)
                .size(instance_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.scene_buffer, &[region]);
            }
        }

        // Zero out indirect template in scene_buffer (region 1)
        {
            let zero_indirect = vk::DrawIndexedIndirectCommand::default();
            let indirect_bytes = draw_cmd_as_bytes(&zero_indirect);
            let mut alloc = staging_ring.allocate(indirect_bytes.len() as u64, 4)?;
            alloc.write_bytes(indirect_bytes);
            let dst_offset = indirect_off
                + remove.slot_id as u64 * size_of::<vk::DrawIndexedIndirectCommand>() as u64;
            let region = vk::BufferCopy::default()
                .src_offset(alloc.offset)
                .dst_offset(dst_offset)
                .size(indirect_bytes.len() as u64);
            unsafe {
                device.cmd_copy_buffer(cmd, alloc.buffer, self.scene_buffer, &[region]);
            }
        }

        // Update draw slot and dense indirect mappings (regions 2 & 3)
        for dsw in remove.draw_slot_writes {
            self.record_draw_slot_copy(device, cmd, staging_ring, slot_off, dsw)?;
        }
        for diw in remove.dense_indirect_writes {
            self.record_dense_indirect_copy(device, cmd, staging_ring, dense_off, diw)?;
        }

        Ok(())
    }

    /// Record all pending uploads for a frame. Called from `record_chunk_delta_uploads`.
    pub fn record_uploads(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        pending_deltas: &mut std::collections::VecDeque<super::RenderDelta>,
    ) -> Result<()> {
        while let Some(delta) = pending_deltas.pop_front() {
            match delta {
                super::RenderDelta::Upsert { key, mesh } => {
                    let upload = self.slot_allocator.prepare_upload(key, &mesh)?;
                    self.record_upload(device, cmd, staging_ring, upload)?;
                }
                super::RenderDelta::Remove { key } => {
                    if let Some(remove) = self.prepare_remove(key) {
                        self.record_remove(device, cmd, staging_ring, remove)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn record_draw_slot_copy(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        slot_region_offset: u64,
        write: DrawSlotWrite,
    ) -> Result<()> {
        let draw_slot = [write.slot_id];
        let slot_bytes = cast_slice(&draw_slot);
        let mut alloc = staging_ring.allocate(slot_bytes.len() as u64, 4)?;
        alloc.write_bytes(slot_bytes);
        let dst_offset = slot_region_offset + write.draw_index as u64 * size_of::<u32>() as u64;
        let region = vk::BufferCopy::default()
            .src_offset(alloc.offset)
            .dst_offset(dst_offset)
            .size(slot_bytes.len() as u64);
        unsafe {
            device.cmd_copy_buffer(cmd, alloc.buffer, self.scene_buffer, &[region]);
        }
        Ok(())
    }

    fn record_dense_indirect_copy(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        dense_region_offset: u64,
        write: DenseIndirectWrite,
    ) -> Result<()> {
        self.dense_indirect_shadow[write.draw_index as usize] = write.command;
        let indirect_bytes = draw_cmd_as_bytes(&write.command);
        let mut alloc = staging_ring.allocate(indirect_bytes.len() as u64, 4)?;
        alloc.write_bytes(indirect_bytes);
        let dst_offset = dense_region_offset
            + write.draw_index as u64 * size_of::<vk::DrawIndexedIndirectCommand>() as u64;
        let region = vk::BufferCopy::default()
            .src_offset(alloc.offset)
            .dst_offset(dst_offset)
            .size(indirect_bytes.len() as u64);
        unsafe {
            device.cmd_copy_buffer(cmd, alloc.buffer, self.scene_buffer, &[region]);
        }
        Ok(())
    }

    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        if let Some(allocation) = self.scene_allocation.take() {
            destroy_allocated_buffer(renderer, self.scene_buffer, allocation)?;
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

/// Reinterpret a `Copy + repr(C)` struct as a byte slice for GPU buffer writes.
///
/// # Safety
///
/// The caller must ensure `T` is `#[repr(C)]` with no padding bytes.
/// Currently only used with `vk::DrawIndexedIndirectCommand` (5 * 4-byte fields, repr(C)).
fn draw_cmd_as_bytes(value: &vk::DrawIndexedIndirectCommand) -> &[u8] {
    const {
        assert!(
            size_of::<vk::DrawIndexedIndirectCommand>() == 5 * 4,
            "DrawIndexedIndirectCommand layout changed -- review safety"
        );
    }
    unsafe {
        std::slice::from_raw_parts(
            (value as *const vk::DrawIndexedIndirectCommand).cast::<u8>(),
            size_of::<vk::DrawIndexedIndirectCommand>(),
        )
    }
}
