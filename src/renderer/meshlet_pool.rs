//! MeshletPool — meshlet-granular GPU SSBO management (MSHL-01, D-04..D-09).
//!
//! Extracted from `chunk_pool.rs` for modularity. Manages 6 GPU SSBOs:
//!   - binding 10: meshlet_meta_buffer      — GpuMeshlet[]
//!   - binding 11: meshlet_vertex_buffer    — PackedVertex[]
//!   - binding 12: meshlet_tri_buffer       — u32[] (widened from u8)
//!   - binding 13: visible_meshlet_buffer   — u32[] (cull output)
//!   - binding 14: meshlet_indirect_buffer  — indirect commands
//!   - binding 15: meshlet_count_buffer     — u32 (visible meshlet count)

use std::{collections::HashMap, mem::size_of};

use anyhow::{Result, anyhow};
use ash::vk;
use bytemuck::{Pod, Zeroable, cast_slice};
use gpu_allocator::{
    MemoryLocation,
    vulkan::{Allocation, AllocationScheme},
};

use crate::{
    meshing::PackedVertex,
    streaming::types::ChunkKey,
};

use super::staging_ring::StagingRing;
use super::{Renderer, create_allocated_buffer, destroy_allocated_buffer};
use super::chunk_pool::stage_and_copy;

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
