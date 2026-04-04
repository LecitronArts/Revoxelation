use std::{collections::HashMap, mem::size_of};

use anyhow::{Result, anyhow};
use ash::vk;
use bytemuck::{Pod, Zeroable, cast_slice};
use gpu_allocator::{
    MemoryLocation,
    vulkan::{Allocation, AllocationScheme},
};

#[allow(unused_imports)]
use crate::{
    meshing::{MeshletMesh, PackedMesh, PackedVertex},
    streaming::types::{CHUNK_EDGE, ChunkKey},
};

use super::coords;
use super::staging_ring::StagingRing;
use super::{Renderer, create_allocated_buffer, destroy_allocated_buffer};

pub const INITIAL_CAPACITY: usize = 1024;

/// Helper: allocate from the staging ring, write data, and record a vkCmdCopyBuffer (REFAC-04).
///
/// Eliminates the repeated allocate→write→copy pattern across upload/remove functions.
fn stage_and_copy(
    staging_ring: &mut StagingRing,
    device: &ash::Device,
    cmd: vk::CommandBuffer,
    data: &[u8],
    alignment: u64,
    dst_buffer: vk::Buffer,
    dst_offset: u64,
) -> Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    let mut alloc = staging_ring.allocate(data.len() as u64, alignment)?;
    alloc.write_bytes(data);
    let region = vk::BufferCopy::default()
        .src_offset(alloc.offset)
        .dst_offset(dst_offset)
        .size(data.len() as u64);
    unsafe {
        device.cmd_copy_buffer(cmd, alloc.buffer, dst_buffer, &[region]);
    }
    Ok(())
}

/// Growth threshold factor: grow when active > capacity * GROW_THRESHOLD.
const GROW_THRESHOLD: f64 = 0.9;
pub const MAX_QUADS_PER_CHUNK: usize = 4096;

/// Per-chunk GPU instance data in the unified scene_buffer (64 bytes, #[repr(C)]).
///
/// Used by the vertex shader via `gl_InstanceIndex`
/// (= firstInstance = slot_id) and by the cull compute shader.
/// Stored in region 0 of scene_buffer.
///
/// Layout (D-02, POLISH-08):
///   aabb_min:      [f32; 3]  — 12 bytes
///   material_id:   u32       —  4 bytes
///   aabb_max:      [f32; 3]  — 12 bytes
///   lod_level:     u32       —  4 bytes
///   chunk_origin:  [f32; 3]  — 12 bytes
///   chunk_scale:   f32       —  4 bytes
///   spawn_time:    f32       —  4 bytes (seconds since engine start, for fade-in)
///   _pad_fade:     [u32; 3]  — 12 bytes (padding to 64 bytes for std430 alignment)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuChunkInstance {
    pub aabb_min: [f32; 3],
    pub material_id: u32,
    pub aabb_max: [f32; 3],
    pub lod_level: u32,
    pub chunk_origin: [f32; 3],
    pub chunk_scale: f32,
    /// Seconds since engine start when this chunk was activated (POLISH-08).
    pub spawn_time: f32,
    /// Alignment padding to 64 bytes for GLSL std430 array stride.
    pub _pad_fade: [u32; 3],
}

/// Calculate byte offsets for the 4 regions of the unified scene_buffer.
///
/// Layout (D-01):
///   Region 0: GpuChunkInstance\[capacity\]           (64 bytes each)
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

    const INSTANCE_STRIDE: usize = size_of::<GpuChunkInstance>(); // 64
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
                let slot_id = self.free_slots.pop().ok_or_else(|| {
                    anyhow!("chunk pool exhausted at {} slots", self.slot_to_chunk.len())
                })?;
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
        let chunk_scale = coords::chunk_scale(key.lod_level);
        let chunk_origin = coords::chunk_origin(key.x, key.y, key.z, key.lod_level);

        let instance = GpuChunkInstance {
            aabb_min: coords::world_aabb(mesh.aabb_min, chunk_origin, chunk_scale),
            material_id: 0, // filled by material system (Plan 04)
            aabb_max: coords::world_aabb(mesh.aabb_max, chunk_origin, chunk_scale),
            lod_level: u32::from(key.lod_level),
            chunk_origin,
            chunk_scale,
            spawn_time: 0.0, // set by caller via SlotUpload (POLISH-08)
            _pad_fade: [0; 3],
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

        let vertex_bytes = cast_slice(mesh.vertices.as_ref())
            .to_vec()
            .into_boxed_slice();
        let index_bytes = cast_slice(mesh.indices.as_ref())
            .to_vec()
            .into_boxed_slice();

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
        self.slot_to_draw_index
            .get(slot_id as usize)
            .copied()
            .flatten()
    }

    /// Current capacity of this allocator.
    pub fn capacity(&self) -> usize {
        self.slot_to_chunk.len()
    }

    /// Grow the allocator to `new_capacity`, extending all internal vectors
    /// and adding new free slots from `old_capacity..new_capacity` (D-08).
    pub fn grow(&mut self, new_capacity: usize) -> Result<()> {
        let old_capacity = self.slot_to_chunk.len();
        if new_capacity <= old_capacity {
            log::error!("grow: new_capacity ({new_capacity}) must exceed old ({old_capacity})");
            return Err(anyhow!("grow: new_capacity must exceed old"));
        }
        self.slot_to_chunk.resize(new_capacity, None);
        self.slot_to_draw_index.resize(new_capacity, None);
        self.instance_shadow
            .resize(new_capacity, GpuChunkInstance::default());
        self.indirect_shadow
            .resize(new_capacity, vk::DrawIndexedIndirectCommand::default());
        // Add new free slots in reverse order so that the lowest new slot is popped first.
        for slot in (old_capacity as u32..new_capacity as u32).rev() {
            self.free_slots.push(slot);
        }
        Ok(())
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
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::VERTEX_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "chunk-pool-vertex",
        )?;

        // Allocate new index buffer
        let (new_index_buffer, new_index_allocation) = create_allocated_buffer(
            renderer,
            (index_slot_stride_bytes() * new_capacity) as u64,
            vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::INDEX_BUFFER,
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

        // Compute per-region offsets for both old and new capacities (CRIT-02).
        // Regions have different offsets in old vs new buffers due to align_up,
        // so a single flat copy would place data at wrong positions.
        let (old_inst, old_indirect, old_slot, old_dense, _) =
            scene_buffer_region_offsets(old_capacity);
        let (new_inst, new_indirect, new_slot, new_dense, _) =
            scene_buffer_region_offsets(new_capacity);

        const INSTANCE_STRIDE: u64 = size_of::<GpuChunkInstance>() as u64; // 48
        const INDIRECT_STRIDE: u64 = 20; // DrawIndexedIndirectCommand: 5 * u32
        const SLOT_STRIDE: u64 = size_of::<u32>() as u64; // 4

        let old_cap = old_capacity as u64;
        let inst_copy_size = old_cap * INSTANCE_STRIDE;
        let indirect_copy_size = old_cap * INDIRECT_STRIDE;
        let slot_copy_size = old_cap * SLOT_STRIDE;
        let dense_copy_size = old_cap * INDIRECT_STRIDE;

        super::helpers::submit_one_shot_commands(renderer, |device, cmd| {
            let vertex_copy = vk::BufferCopy::default().size(old_vertex_size);
            let index_copy = vk::BufferCopy::default().size(old_index_size);
            // Per-region scene_buffer copies with correct src/dst offsets (CRIT-02).
            let scene_copies = [
                // Region 0: GpuChunkInstance[]
                vk::BufferCopy::default()
                    .src_offset(old_inst)
                    .dst_offset(new_inst)
                    .size(inst_copy_size),
                // Region 1: DrawIndexedIndirectCommand[] (indirect templates)
                vk::BufferCopy::default()
                    .src_offset(old_indirect)
                    .dst_offset(new_indirect)
                    .size(indirect_copy_size),
                // Region 2: u32[] (draw slot mapping)
                vk::BufferCopy::default()
                    .src_offset(old_slot)
                    .dst_offset(new_slot)
                    .size(slot_copy_size),
                // Region 3: DrawIndexedIndirectCommand[] (dense indirect output)
                vk::BufferCopy::default()
                    .src_offset(old_dense)
                    .dst_offset(new_dense)
                    .size(dense_copy_size),
            ];
            unsafe {
                device.cmd_copy_buffer(cmd, old_vertex, new_vertex_buffer, &[vertex_copy]);
                device.cmd_copy_buffer(cmd, old_index, new_index_buffer, &[index_copy]);
                device.cmd_copy_buffer(cmd, old_scene, new_scene_buffer, &scene_copies);
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
        self.dense_indirect_shadow
            .resize(new_capacity, vk::DrawIndexedIndirectCommand::default());

        // Grow slot allocator
        self.slot_allocator.grow(new_capacity)?;

        // Update BindlessTable binding 0 to point to the new scene_buffer (D-05)
        bindless.register_buffer(
            &renderer.device_ctx.device,
            super::bindless::BINDING_SCENE,
            self.scene_buffer,
            vk::WHOLE_SIZE,
        );

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
        stage_and_copy(
            staging_ring,
            device,
            cmd,
            &vertex_bytes,
            16,
            self.vertex_buffer,
            vertex_offset_bytes,
        )?;

        // Copy index data via staging
        stage_and_copy(
            staging_ring,
            device,
            cmd,
            &index_bytes,
            4,
            self.index_buffer,
            index_offset_bytes,
        )?;

        // Copy GpuChunkInstance to scene_buffer instance region (region 0)
        {
            let instance_arr = [instance];
            let instance_bytes = cast_slice(&instance_arr);
            let dst_offset = inst_off + slot_id as u64 * size_of::<GpuChunkInstance>() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                instance_bytes,
                16,
                self.scene_buffer,
                dst_offset,
            )?;
        }

        // Copy indirect template to scene_buffer indirect template region (region 1)
        {
            let pod = DrawCmdPod::from_vk(&indirect);
            let indirect_bytes = bytemuck::bytes_of(&pod);
            let dst_offset =
                indirect_off + slot_id as u64 * size_of::<vk::DrawIndexedIndirectCommand>() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                indirect_bytes,
                4,
                self.scene_buffer,
                dst_offset,
            )?;
        }

        // Copy draw slot mapping via staging (region 2)
        if let Some(dsw) = draw_slot_write {
            self.record_draw_slot_copy(device, cmd, staging_ring, slot_off, dsw)?;
        }

        // Copy dense indirect entry via staging (region 3)
        self.record_dense_indirect_copy(
            device,
            cmd,
            staging_ring,
            dense_off,
            dense_indirect_write,
        )?;

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
            let dst_offset = remove.slot_id as u64 * vertex_slot_stride_bytes() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                &zero_bytes,
                16,
                self.vertex_buffer,
                dst_offset,
            )?;
        }

        // Zero out the slot's index data
        {
            let zero_bytes = vec![0_u8; index_slot_stride_bytes()];
            let dst_offset = remove.slot_id as u64 * index_slot_stride_bytes() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                &zero_bytes,
                4,
                self.index_buffer,
                dst_offset,
            )?;
        }

        // Zero out GpuChunkInstance in scene_buffer (region 0)
        {
            let zero_instance = [GpuChunkInstance::default()];
            let instance_bytes = cast_slice(&zero_instance);
            let dst_offset =
                inst_off + remove.slot_id as u64 * size_of::<GpuChunkInstance>() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                instance_bytes,
                16,
                self.scene_buffer,
                dst_offset,
            )?;
        }

        // Zero out indirect template in scene_buffer (region 1)
        {
            let zero_indirect = vk::DrawIndexedIndirectCommand::default();
            let pod = DrawCmdPod::from_vk(&zero_indirect);
            let indirect_bytes = bytemuck::bytes_of(&pod);
            let dst_offset = indirect_off
                + remove.slot_id as u64 * size_of::<vk::DrawIndexedIndirectCommand>() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                indirect_bytes,
                4,
                self.scene_buffer,
                dst_offset,
            )?;
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
    ///
    /// When the staging ring is exhausted mid-batch, remaining deltas are deferred
    /// (pushed back to the front of `pending_deltas`) for retry next frame.
    /// Partially uploaded deltas within the same frame are still valid.
    pub fn record_uploads(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        pending_deltas: &mut std::collections::VecDeque<super::RenderDelta>,
    ) -> Result<()> {
        while let Some(delta) = pending_deltas.pop_front() {
            let result = match &delta {
                super::RenderDelta::Upsert { key, mesh } => {
                    let packed = mesh.to_packed_mesh();
                    match self.slot_allocator.prepare_upload(*key, &packed) {
                        Ok(upload) => self.record_upload(device, cmd, staging_ring, upload),
                        Err(e) => Err(e),
                    }
                }
                super::RenderDelta::Remove { key } => {
                    if let Some(remove) = self.prepare_remove(*key) {
                        self.record_remove(device, cmd, staging_ring, remove)
                    } else {
                        Ok(())
                    }
                }
            };

            if let Err(e) = result {
                // Staging ring exhaustion — defer this delta and all remaining to next frame.
                let remaining = pending_deltas.len() + 1; // +1 for the current failed delta
                log::warn!(
                    "staging ring exhausted: deferred {} chunk deltas to next frame ({})",
                    remaining,
                    e,
                );
                // Push the failed delta back to the front so it retries first next frame.
                pending_deltas.push_front(delta);
                return Ok(());
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
        let dst_offset = slot_region_offset + write.draw_index as u64 * size_of::<u32>() as u64;
        stage_and_copy(
            staging_ring,
            device,
            cmd,
            slot_bytes,
            4,
            self.scene_buffer,
            dst_offset,
        )
    }

    fn record_dense_indirect_copy(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        dense_region_offset: u64,
        write: DenseIndirectWrite,
    ) -> Result<()> {
        debug_assert!(
            (write.draw_index as usize) < self.capacity,
            "dense_indirect_shadow: draw_index {} out of bounds (capacity {})",
            write.draw_index,
            self.capacity,
        );
        self.dense_indirect_shadow[write.draw_index as usize] = write.command;
        let pod = DrawCmdPod::from_vk(&write.command);
        let indirect_bytes = bytemuck::bytes_of(&pod);
        let dst_offset = dense_region_offset
            + write.draw_index as u64 * size_of::<vk::DrawIndexedIndirectCommand>() as u64;
        stage_and_copy(
            staging_ring,
            device,
            cmd,
            indirect_bytes,
            4,
            self.scene_buffer,
            dst_offset,
        )
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

// chunk_origin and chunk_scale are now in coords.rs (CRIT-05 fix).

/// Safe #[repr(C)] Pod wrapper for DrawIndexedIndirectCommand, used for bytemuck casts.
///
/// Mirrors the 5 u32 fields of `vk::DrawIndexedIndirectCommand` but derives Pod + Zeroable
/// so we can use `bytemuck::bytes_of` for safe byte-level access.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct DrawCmdPod {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    vertex_offset: i32,
    first_instance: u32,
}

impl DrawCmdPod {
    /// Convert a `vk::DrawIndexedIndirectCommand` to a safe Pod wrapper.
    fn from_vk(cmd: &vk::DrawIndexedIndirectCommand) -> Self {
        Self {
            index_count: cmd.index_count,
            instance_count: cmd.instance_count,
            first_index: cmd.first_index,
            vertex_offset: cmd.vertex_offset,
            first_instance: cmd.first_instance,
        }
    }
}

// world_aabb is now in coords.rs (CRIT-05 fix).

// ===========================================================================
// MeshletPool — meshlet-granular GPU SSBO management (MSHL-01, D-04..D-09)
// ===========================================================================

/// Per-meshlet GPU metadata (64 bytes, #[repr(C)], Pod+Zeroable).
///
/// Layout (D-04, MSHL-05):
///   center:          [f32; 3]  — bounding sphere center (local-space)   12B
///   radius:          f32       — bounding sphere radius                  4B
///   cone_axis:       [f32; 3]  — orientation cone axis (normalized)     12B
///   cone_cutoff:     f32       — cos(half-angle)                         4B
///   vertex_offset:   u32       — into meshlet_vertex_buffer              4B
///   triangle_offset: u32       — into meshlet_tri_buffer                 4B
///   vertex_count:    u32       — max 64                                  4B
///   triangle_count:  u32       — max 124                                 4B
///   chunk_slot:      u32       — which chunk this meshlet belongs to     4B
///   lod_level:       u32       — LOD level (0 = original, 1 = simplified) 4B
///   parent_error:    f32       — simplification error for LOD selection   4B
///   group_id:        u32       — LOD group ID                            4B
///   Total: 64 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuMeshlet {
    pub center: [f32; 3],
    pub radius: f32,
    pub cone_axis: [f32; 3],
    pub cone_cutoff: f32,
    pub vertex_offset: u32,
    pub triangle_offset: u32,
    pub vertex_count: u32,
    pub triangle_count: u32,
    pub chunk_slot: u32,
    pub lod_level: u32,
    pub parent_error: f32,
    pub group_id: u32,
}

impl GpuMeshlet {
    pub fn zeroed() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

/// Initial meshlet capacity (D-06).
pub const INITIAL_MESHLET_CAPACITY: usize = 65536;
/// Initial vertex capacity: 65536 * 64 (D-07).
pub const INITIAL_MESHLET_VERTEX_CAPACITY: usize = INITIAL_MESHLET_CAPACITY * 64;
/// Initial triangle index capacity (u32): 65536 * 124 * 3 (D-07).
pub const INITIAL_MESHLET_TRI_CAPACITY: usize = INITIAL_MESHLET_CAPACITY * 124 * 3;
/// Growth threshold for meshlet buffers.
const MESHLET_GROW_THRESHOLD: f64 = 0.9;

/// Tracks the GPU buffer ranges occupied by a single chunk's meshlets (CRIT-04).
///
/// Used both in `chunk_ranges` (active) and `free_ranges` (available for reuse).
#[derive(Debug, Clone, Copy)]
pub struct MeshletRange {
    pub meshlet_start: u32,
    pub meshlet_count: u32,
    pub vertex_start: u32,
    pub vertex_count: u32,
    pub tri_start: u32,
    pub tri_count: u32,
}

/// Meshlet-granular GPU storage with 6 SSBOs + retained scene_buffer (D-05).
///
/// Buffers:
///   binding 10: meshlet_meta_buffer      — GpuMeshlet[]
///   binding 11: meshlet_vertex_buffer    — PackedVertex[]
///   binding 12: meshlet_tri_buffer       — u32[] (widened from u8)
///   binding 13: visible_meshlet_buffer   — u32[] (cull output, Plan 02)
///   binding 14: meshlet_indirect_buffer  — indirect commands (Plan 03)
///   binding 15: meshlet_count_buffer     — u32 (visible meshlet count, Plan 02)
pub struct MeshletPool {
    pub meshlet_meta_buffer: vk::Buffer,
    meshlet_meta_allocation: Option<Allocation>,
    pub meshlet_vertex_buffer: vk::Buffer,
    meshlet_vertex_allocation: Option<Allocation>,
    pub meshlet_tri_buffer: vk::Buffer,
    meshlet_tri_allocation: Option<Allocation>,
    pub visible_meshlet_buffer: vk::Buffer,
    visible_meshlet_allocation: Option<Allocation>,
    pub meshlet_indirect_buffer: vk::Buffer,
    meshlet_indirect_allocation: Option<Allocation>,
    pub meshlet_count_buffer: vk::Buffer,
    meshlet_count_allocation: Option<Allocation>,
    shadow_visible_meshlets: Vec<u32>,
    shadow_draw_commands: Vec<vk::DrawIndexedIndirectCommand>,

    /// Per-chunk meshlet range tracking for removal + reclamation (D-09, CRIT-04).
    chunk_ranges: HashMap<ChunkKey, MeshletRange>,

    /// Freed ranges available for reuse on subsequent uploads (CRIT-04).
    free_ranges: Vec<MeshletRange>,

    /// Current meshlet capacity.
    meshlet_capacity: usize,
    /// Current meshlet vertex capacity.
    vertex_capacity: usize,
    /// Current meshlet triangle index (u32) capacity.
    tri_capacity: usize,
    /// Number of active meshlets.
    active_meshlet_count: u32,
    /// Running offset into meshlet_vertex_buffer.
    active_vertex_count: u32,
    /// Running offset into meshlet_tri_buffer (u32 indices).
    active_tri_count: u32,
    /// Append-only tail for new meshlet metadata ranges.
    meshlet_tail: u32,
    /// Append-only tail for meshlet vertex storage.
    vertex_tail: u32,
    /// Append-only tail for widened triangle index storage.
    tri_tail: u32,
}

impl MeshletPool {
    /// Allocate all meshlet buffers with initial capacities (D-06, D-07).
    pub fn new(renderer: &mut Renderer) -> Result<Self> {
        let meshlet_capacity = INITIAL_MESHLET_CAPACITY;
        let vertex_capacity = INITIAL_MESHLET_VERTEX_CAPACITY;
        let tri_capacity = INITIAL_MESHLET_TRI_CAPACITY;

        let usage = vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::STORAGE_BUFFER;

        let (meshlet_meta_buffer, meshlet_meta_allocation) = create_allocated_buffer(
            renderer,
            (meshlet_capacity * size_of::<GpuMeshlet>()) as u64,
            usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-meta",
        )?;

        let (meshlet_vertex_buffer, meshlet_vertex_allocation) = create_allocated_buffer(
            renderer,
            (vertex_capacity * size_of::<PackedVertex>()) as u64,
            usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-vertex",
        )?;

        let (meshlet_tri_buffer, meshlet_tri_allocation) = create_allocated_buffer(
            renderer,
            (tri_capacity * size_of::<u32>()) as u64,
            usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-tri",
        )?;

        let (visible_meshlet_buffer, visible_meshlet_allocation) = create_allocated_buffer(
            renderer,
            (meshlet_capacity * size_of::<u32>()) as u64,
            usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-visible",
        )?;

        // Indirect buffer: each visible meshlet gets one DrawIndexedIndirectCommand (20 bytes).
        let (meshlet_indirect_buffer, meshlet_indirect_allocation) = create_allocated_buffer(
            renderer,
            (meshlet_capacity * 20) as u64,
            usage | vk::BufferUsageFlags::INDIRECT_BUFFER,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-indirect",
        )?;

        // Count buffer: single u32.
        let (meshlet_count_buffer, meshlet_count_allocation) = create_allocated_buffer(
            renderer,
            size_of::<u32>() as u64,
            usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-count",
        )?;

        Ok(Self {
            meshlet_meta_buffer,
            meshlet_meta_allocation: Some(meshlet_meta_allocation),
            meshlet_vertex_buffer,
            meshlet_vertex_allocation: Some(meshlet_vertex_allocation),
            meshlet_tri_buffer,
            meshlet_tri_allocation: Some(meshlet_tri_allocation),
            visible_meshlet_buffer,
            visible_meshlet_allocation: Some(visible_meshlet_allocation),
            meshlet_indirect_buffer,
            meshlet_indirect_allocation: Some(meshlet_indirect_allocation),
            meshlet_count_buffer,
            meshlet_count_allocation: Some(meshlet_count_allocation),
            shadow_visible_meshlets: vec![0; meshlet_capacity],
            shadow_draw_commands: vec![vk::DrawIndexedIndirectCommand::default(); meshlet_capacity],
            chunk_ranges: HashMap::new(),
            free_ranges: Vec::new(),
            meshlet_capacity,
            vertex_capacity,
            tri_capacity,
            active_meshlet_count: 0,
            active_vertex_count: 0,
            active_tri_count: 0,
            meshlet_tail: 0,
            vertex_tail: 0,
            tri_tail: 0,
        })
    }

    fn clear_shadow_range(&mut self, range: MeshletRange) {
        let start = range.meshlet_start as usize;
        let end = (range.meshlet_start + range.meshlet_count) as usize;
        for command in &mut self.shadow_draw_commands[start..end] {
            *command = vk::DrawIndexedIndirectCommand::default();
        }
    }

    fn reserve_range(
        &mut self,
        key: ChunkKey,
        meshlet_count: u32,
        vertex_count: u32,
        tri_count: u32,
    ) -> Result<MeshletRange> {
        if let Some(old_range) = self.chunk_ranges.remove(&key) {
            self.active_meshlet_count -= old_range.meshlet_count;
            self.active_vertex_count -= old_range.vertex_count;
            self.active_tri_count -= old_range.tri_count;
            self.clear_shadow_range(old_range);
            self.free_ranges.push(old_range);
        }

        let reuse_idx = self.free_ranges.iter().position(|r| {
            r.meshlet_count >= meshlet_count
                && r.vertex_count >= vertex_count
                && r.tri_count >= tri_count
        });

        let range = if let Some(idx) = reuse_idx {
            let free = self.free_ranges.swap_remove(idx);
            MeshletRange {
                meshlet_start: free.meshlet_start,
                meshlet_count,
                vertex_start: free.vertex_start,
                vertex_count,
                tri_start: free.tri_start,
                tri_count,
            }
        } else {
            let meshlet_end = self
                .meshlet_tail
                .checked_add(meshlet_count)
                .ok_or_else(|| anyhow!("meshlet append exceeds capacity"))?;
            let vertex_end = self
                .vertex_tail
                .checked_add(vertex_count)
                .ok_or_else(|| anyhow!("meshlet append exceeds capacity"))?;
            let tri_end = self
                .tri_tail
                .checked_add(tri_count)
                .ok_or_else(|| anyhow!("meshlet append exceeds capacity"))?;

            if meshlet_end as usize > self.meshlet_capacity
                || vertex_end as usize > self.vertex_capacity
                || tri_end as usize > self.tri_capacity
            {
                return Err(anyhow!(
                    "meshlet pool exhausted: need meshlets={}, vertices={}, triangles={} with tails ({}, {}, {}) and capacities ({}, {}, {})",
                    meshlet_count,
                    vertex_count,
                    tri_count,
                    self.meshlet_tail,
                    self.vertex_tail,
                    self.tri_tail,
                    self.meshlet_capacity,
                    self.vertex_capacity,
                    self.tri_capacity,
                ));
            }

            let range = MeshletRange {
                meshlet_start: self.meshlet_tail,
                meshlet_count,
                vertex_start: self.vertex_tail,
                vertex_count,
                tri_start: self.tri_tail,
                tri_count,
            };
            self.meshlet_tail = meshlet_end;
            self.vertex_tail = vertex_end;
            self.tri_tail = tri_end;
            range
        };

        self.chunk_ranges.insert(key, range);
        self.active_meshlet_count += meshlet_count;
        self.active_vertex_count += vertex_count;
        self.active_tri_count += tri_count;
        Ok(range)
    }

    /// Upload a MeshletMesh for a chunk, converting MeshletDescriptors to GpuMeshlets
    /// and widening u8 triangle indices to u32 (D-05).
    pub fn record_upload(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
        key: ChunkKey,
        mesh: &crate::meshing::MeshletMesh,
        chunk_slot: u32,
    ) -> Result<()> {
        use crate::meshing::MeshletDescriptor;

        // Pre-compute widened triangle indices to know the count before slot selection.
        let widened_tris: Vec<u32> = mesh.triangles.iter().map(|&i| u32::from(i)).collect();
        let meshlet_count = mesh.meshlets.len() as u32;
        let vertex_count = mesh.vertices.len() as u32;
        let tri_count = widened_tris.len() as u32;
        let range = self.reserve_range(key, meshlet_count, vertex_count, tri_count)?;
        let meshlet_start = range.meshlet_start;
        let vertex_start = range.vertex_start;
        let tri_start = range.tri_start;

        // Build GpuMeshlet array.
        let gpu_meshlets: Vec<GpuMeshlet> = mesh
            .meshlets
            .iter()
            .map(|desc: &MeshletDescriptor| GpuMeshlet {
                center: desc.center,
                radius: desc.radius,
                cone_axis: desc.cone_axis,
                cone_cutoff: desc.cone_cutoff,
                vertex_offset: vertex_start + desc.vertex_offset,
                triangle_offset: tri_start + desc.triangle_offset,
                vertex_count: desc.vertex_count,
                triangle_count: desc.triangle_count,
                chunk_slot,
                lod_level: u32::from(desc.lod_level),
                parent_error: desc.parent_error,
                group_id: desc.group_id,
            })
            .collect();
        for (local_index, desc) in mesh.meshlets.iter().enumerate() {
            let meshlet_id = meshlet_start as usize + local_index;
            self.shadow_draw_commands[meshlet_id] = vk::DrawIndexedIndirectCommand {
                index_count: desc.triangle_count * 3,
                instance_count: 1,
                first_index: tri_start + desc.triangle_offset,
                vertex_offset: (vertex_start + desc.vertex_offset) as i32,
                first_instance: 0,
            };
        }

        // Upload meshlet metadata via staging.
        {
            let meta_bytes = cast_slice::<GpuMeshlet, u8>(&gpu_meshlets);
            let dst_offset = meshlet_start as u64 * size_of::<GpuMeshlet>() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                meta_bytes,
                16,
                self.meshlet_meta_buffer,
                dst_offset,
            )?;
        }

        // Upload vertex data via staging.
        {
            let vertex_bytes = cast_slice::<PackedVertex, u8>(&mesh.vertices);
            let dst_offset = vertex_start as u64 * size_of::<PackedVertex>() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                vertex_bytes,
                16,
                self.meshlet_vertex_buffer,
                dst_offset,
            )?;
        }

        // Upload widened triangle indices via staging.
        {
            let tri_bytes = cast_slice::<u32, u8>(&widened_tris);
            let dst_offset = tri_start as u64 * size_of::<u32>() as u64;
            stage_and_copy(
                staging_ring,
                device,
                cmd,
                tri_bytes,
                4,
                self.meshlet_tri_buffer,
                dst_offset,
            )?;
        }

        Ok(())
    }

    /// Remove meshlet range for a chunk, decrementing active counts and
    /// pushing the freed range for reuse (CRIT-04, D-09).
    pub fn record_remove(&mut self, key: ChunkKey) {
        if let Some(range) = self.chunk_ranges.remove(&key) {
            self.active_meshlet_count -= range.meshlet_count;
            self.active_vertex_count -= range.vertex_count;
            self.active_tri_count -= range.tri_count;
            self.clear_shadow_range(range);
            self.free_ranges.push(range);
        }
    }

    /// Returns true when active meshlets exceed 90% of capacity (D-06).
    pub fn needs_grow(&self) -> bool {
        let meshlet_threshold = self.meshlet_capacity as f64 * MESHLET_GROW_THRESHOLD;
        let vertex_threshold = self.vertex_capacity as f64 * MESHLET_GROW_THRESHOLD;
        let tri_threshold = self.tri_capacity as f64 * MESHLET_GROW_THRESHOLD;
        self.meshlet_tail as f64 > meshlet_threshold
            || self.vertex_tail as f64 > vertex_threshold
            || self.tri_tail as f64 > tri_threshold
    }

    /// Grow meshlet storage by 2x between frames, preserving uploaded geometry
    /// and rebinding the new buffers into the global bindless table.
    pub fn grow_capacity(
        &mut self,
        renderer: &mut Renderer,
        bindless: &super::bindless::BindlessTable,
    ) -> Result<()> {
        let old_meshlet_capacity = self.meshlet_capacity;
        let old_vertex_capacity = self.vertex_capacity;
        let old_tri_capacity = self.tri_capacity;
        let new_meshlet_capacity = old_meshlet_capacity * 2;
        let new_vertex_capacity = old_vertex_capacity * 2;
        let new_tri_capacity = old_tri_capacity * 2;
        log::info!(
            "MeshletPool: growing meshlets {old_meshlet_capacity}->{new_meshlet_capacity}, vertices {old_vertex_capacity}->{new_vertex_capacity}, triangles {old_tri_capacity}->{new_tri_capacity}"
        );

        let storage_usage = vk::BufferUsageFlags::TRANSFER_DST
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::STORAGE_BUFFER;

        let (new_meshlet_meta_buffer, new_meshlet_meta_allocation) = create_allocated_buffer(
            renderer,
            (new_meshlet_capacity * size_of::<GpuMeshlet>()) as u64,
            storage_usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-meta",
        )?;
        let (new_meshlet_vertex_buffer, new_meshlet_vertex_allocation) = create_allocated_buffer(
            renderer,
            (new_vertex_capacity * size_of::<PackedVertex>()) as u64,
            storage_usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-vertex",
        )?;
        let (new_meshlet_tri_buffer, new_meshlet_tri_allocation) = create_allocated_buffer(
            renderer,
            (new_tri_capacity * size_of::<u32>()) as u64,
            storage_usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-tri",
        )?;
        let (new_visible_meshlet_buffer, new_visible_meshlet_allocation) = create_allocated_buffer(
            renderer,
            (new_meshlet_capacity * size_of::<u32>()) as u64,
            storage_usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-visible",
        )?;
        let (new_meshlet_indirect_buffer, new_meshlet_indirect_allocation) =
            create_allocated_buffer(
                renderer,
                (new_meshlet_capacity * size_of::<DrawCmdPod>()) as u64,
                storage_usage | vk::BufferUsageFlags::INDIRECT_BUFFER,
                MemoryLocation::GpuOnly,
                AllocationScheme::GpuAllocatorManaged,
                "meshlet-pool-indirect",
            )?;
        let (new_meshlet_count_buffer, new_meshlet_count_allocation) = create_allocated_buffer(
            renderer,
            size_of::<u32>() as u64,
            storage_usage,
            MemoryLocation::GpuOnly,
            AllocationScheme::GpuAllocatorManaged,
            "meshlet-pool-count",
        )?;

        let old_meshlet_meta_buffer = self.meshlet_meta_buffer;
        let old_meshlet_vertex_buffer = self.meshlet_vertex_buffer;
        let old_meshlet_tri_buffer = self.meshlet_tri_buffer;
        let old_visible_meshlet_buffer = self.visible_meshlet_buffer;
        let old_meshlet_indirect_buffer = self.meshlet_indirect_buffer;
        let old_meshlet_count_buffer = self.meshlet_count_buffer;

        let meshlet_meta_copy_size = self.meshlet_tail as u64 * size_of::<GpuMeshlet>() as u64;
        let meshlet_vertex_copy_size = self.vertex_tail as u64 * size_of::<PackedVertex>() as u64;
        let meshlet_tri_copy_size = self.tri_tail as u64 * size_of::<u32>() as u64;
        let visible_meshlet_copy_size = old_meshlet_capacity as u64 * size_of::<u32>() as u64;
        let meshlet_indirect_copy_size =
            old_meshlet_capacity as u64 * size_of::<DrawCmdPod>() as u64;
        let meshlet_count_copy_size = size_of::<u32>() as u64;

        super::helpers::submit_one_shot_commands(renderer, |device, cmd| {
            let copy = |src: vk::Buffer, dst: vk::Buffer, size: u64| unsafe {
                if size > 0 {
                    let region = vk::BufferCopy::default().size(size);
                    device.cmd_copy_buffer(cmd, src, dst, &[region]);
                }
            };

            copy(
                old_meshlet_meta_buffer,
                new_meshlet_meta_buffer,
                meshlet_meta_copy_size,
            );
            copy(
                old_meshlet_vertex_buffer,
                new_meshlet_vertex_buffer,
                meshlet_vertex_copy_size,
            );
            copy(
                old_meshlet_tri_buffer,
                new_meshlet_tri_buffer,
                meshlet_tri_copy_size,
            );
            copy(
                old_visible_meshlet_buffer,
                new_visible_meshlet_buffer,
                visible_meshlet_copy_size,
            );
            copy(
                old_meshlet_indirect_buffer,
                new_meshlet_indirect_buffer,
                meshlet_indirect_copy_size,
            );
            copy(
                old_meshlet_count_buffer,
                new_meshlet_count_buffer,
                meshlet_count_copy_size,
            );
            Ok(())
        })?;

        if let Some(allocation) = self.meshlet_count_allocation.take() {
            destroy_allocated_buffer(renderer, self.meshlet_count_buffer, allocation)?;
        }
        if let Some(allocation) = self.meshlet_indirect_allocation.take() {
            destroy_allocated_buffer(renderer, self.meshlet_indirect_buffer, allocation)?;
        }
        if let Some(allocation) = self.visible_meshlet_allocation.take() {
            destroy_allocated_buffer(renderer, self.visible_meshlet_buffer, allocation)?;
        }
        if let Some(allocation) = self.meshlet_tri_allocation.take() {
            destroy_allocated_buffer(renderer, self.meshlet_tri_buffer, allocation)?;
        }
        if let Some(allocation) = self.meshlet_vertex_allocation.take() {
            destroy_allocated_buffer(renderer, self.meshlet_vertex_buffer, allocation)?;
        }
        if let Some(allocation) = self.meshlet_meta_allocation.take() {
            destroy_allocated_buffer(renderer, self.meshlet_meta_buffer, allocation)?;
        }

        self.meshlet_meta_buffer = new_meshlet_meta_buffer;
        self.meshlet_meta_allocation = Some(new_meshlet_meta_allocation);
        self.meshlet_vertex_buffer = new_meshlet_vertex_buffer;
        self.meshlet_vertex_allocation = Some(new_meshlet_vertex_allocation);
        self.meshlet_tri_buffer = new_meshlet_tri_buffer;
        self.meshlet_tri_allocation = Some(new_meshlet_tri_allocation);
        self.visible_meshlet_buffer = new_visible_meshlet_buffer;
        self.visible_meshlet_allocation = Some(new_visible_meshlet_allocation);
        self.meshlet_indirect_buffer = new_meshlet_indirect_buffer;
        self.meshlet_indirect_allocation = Some(new_meshlet_indirect_allocation);
        self.meshlet_count_buffer = new_meshlet_count_buffer;
        self.meshlet_count_allocation = Some(new_meshlet_count_allocation);
        self.meshlet_capacity = new_meshlet_capacity;
        self.vertex_capacity = new_vertex_capacity;
        self.tri_capacity = new_tri_capacity;
        self.shadow_visible_meshlets.resize(new_meshlet_capacity, 0);
        self.shadow_draw_commands.resize(
            new_meshlet_capacity,
            vk::DrawIndexedIndirectCommand::default(),
        );

        bindless.register_meshlet_buffers(&renderer.device_ctx.device, self);

        log::info!(
            "MeshletPool: growth complete, new capacities = meshlets {}, vertices {}, triangles {}",
            self.meshlet_capacity,
            self.vertex_capacity,
            self.tri_capacity,
        );
        Ok(())
    }

    /// Active meshlet count.
    pub fn active_meshlet_count(&self) -> u32 {
        self.active_meshlet_count
    }

    /// Current meshlet buffer capacity (MED-05).
    /// Used for max_draw_count in indirect draw calls instead of hardcoded constant.
    pub fn meshlet_capacity(&self) -> usize {
        self.meshlet_capacity
    }

    /// Meshlet range for a chunk.
    pub fn chunk_range(&self, key: ChunkKey) -> Option<(u32, u32)> {
        self.chunk_ranges
            .get(&key)
            .map(|r| (r.meshlet_start, r.meshlet_count))
    }

    pub fn record_shadow_draw_setup(
        &mut self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        staging_ring: &mut StagingRing,
    ) -> Result<()> {
        let mut ranges: Vec<MeshletRange> = self.chunk_ranges.values().copied().collect();
        ranges.sort_by_key(|range| range.meshlet_start);

        let mut shadow_draw_count = 0usize;
        for range in ranges {
            for meshlet_id in range.meshlet_start..range.meshlet_start + range.meshlet_count {
                let command = self.shadow_draw_commands[meshlet_id as usize];
                if command.index_count == 0 {
                    continue;
                }
                self.shadow_visible_meshlets[shadow_draw_count] = meshlet_id;
                shadow_draw_count += 1;
            }
        }

        let shadow_draw_count_u32 = shadow_draw_count as u32;
        stage_and_copy(
            staging_ring,
            device,
            cmd,
            bytemuck::bytes_of(&shadow_draw_count_u32),
            4,
            self.meshlet_count_buffer,
            0,
        )?;
        if shadow_draw_count == 0 {
            return Ok(());
        }

        let visible_bytes =
            cast_slice::<u32, u8>(&self.shadow_visible_meshlets[..shadow_draw_count]);
        stage_and_copy(
            staging_ring,
            device,
            cmd,
            visible_bytes,
            4,
            self.visible_meshlet_buffer,
            0,
        )?;

        let dense_shadow_commands: Vec<DrawCmdPod> = self.shadow_visible_meshlets
            [..shadow_draw_count]
            .iter()
            .map(|&meshlet_id| DrawCmdPod::from_vk(&self.shadow_draw_commands[meshlet_id as usize]))
            .collect();
        let dense_shadow_bytes = cast_slice::<DrawCmdPod, u8>(&dense_shadow_commands);
        stage_and_copy(
            staging_ring,
            device,
            cmd,
            dense_shadow_bytes,
            4,
            self.meshlet_indirect_buffer,
            0,
        )?;

        Ok(())
    }

    /// Destroy all GPU allocations.
    pub fn destroy(mut self, renderer: &mut Renderer) -> Result<()> {
        macro_rules! free_buf {
            ($buf:expr, $alloc:expr) => {
                if let Some(alloc) = $alloc.take() {
                    destroy_allocated_buffer(renderer, $buf, alloc)?;
                }
            };
        }
        free_buf!(self.meshlet_count_buffer, self.meshlet_count_allocation);
        free_buf!(
            self.meshlet_indirect_buffer,
            self.meshlet_indirect_allocation
        );
        free_buf!(self.visible_meshlet_buffer, self.visible_meshlet_allocation);
        free_buf!(self.meshlet_tri_buffer, self.meshlet_tri_allocation);
        free_buf!(self.meshlet_vertex_buffer, self.meshlet_vertex_allocation);
        free_buf!(self.meshlet_meta_buffer, self.meshlet_meta_allocation);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_meshlet_pool(
        meshlet_capacity: usize,
        vertex_capacity: usize,
        tri_capacity: usize,
    ) -> MeshletPool {
        MeshletPool {
            meshlet_meta_buffer: vk::Buffer::null(),
            meshlet_meta_allocation: None,
            meshlet_vertex_buffer: vk::Buffer::null(),
            meshlet_vertex_allocation: None,
            meshlet_tri_buffer: vk::Buffer::null(),
            meshlet_tri_allocation: None,
            visible_meshlet_buffer: vk::Buffer::null(),
            visible_meshlet_allocation: None,
            meshlet_indirect_buffer: vk::Buffer::null(),
            meshlet_indirect_allocation: None,
            meshlet_count_buffer: vk::Buffer::null(),
            meshlet_count_allocation: None,
            shadow_visible_meshlets: vec![0; meshlet_capacity],
            shadow_draw_commands: vec![vk::DrawIndexedIndirectCommand::default(); meshlet_capacity],
            chunk_ranges: HashMap::new(),
            free_ranges: Vec::new(),
            meshlet_capacity,
            vertex_capacity,
            tri_capacity,
            active_meshlet_count: 0,
            active_vertex_count: 0,
            active_tri_count: 0,
            meshlet_tail: 0,
            vertex_tail: 0,
            tri_tail: 0,
        }
    }

    #[test]
    fn meshlet_reserve_append_does_not_overlap_live_tail_after_non_tail_remove() {
        let mut pool = test_meshlet_pool(64, 64, 64);
        let a = pool
            .reserve_range(ChunkKey::new(0, 0, 0, 0), 10, 10, 10)
            .unwrap();
        let b = pool
            .reserve_range(ChunkKey::new(1, 0, 0, 0), 10, 10, 10)
            .unwrap();

        pool.record_remove(ChunkKey::new(0, 0, 0, 0));

        let c = pool
            .reserve_range(ChunkKey::new(2, 0, 0, 0), 15, 15, 15)
            .unwrap();
        assert_eq!(a.meshlet_start, 0);
        assert_eq!(b.meshlet_start, 10);
        assert_eq!(c.meshlet_start, 20);
        assert!(c.meshlet_start >= b.meshlet_start + b.meshlet_count);
        assert_eq!(c.vertex_start, 20);
        assert_eq!(c.tri_start, 20);
    }

    #[test]
    fn meshlet_reserve_errors_when_append_exceeds_capacity() {
        let mut pool = test_meshlet_pool(20, 20, 20);
        pool.reserve_range(ChunkKey::new(0, 0, 0, 0), 10, 10, 10)
            .unwrap();
        pool.reserve_range(ChunkKey::new(1, 0, 0, 0), 10, 10, 10)
            .unwrap();
        pool.record_remove(ChunkKey::new(0, 0, 0, 0));

        let err = pool
            .reserve_range(ChunkKey::new(2, 0, 0, 0), 15, 15, 15)
            .unwrap_err();
        assert!(err.to_string().contains("meshlet pool exhausted"));
    }
}
