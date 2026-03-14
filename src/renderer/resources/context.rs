use super::bind_groups::BindGroupExecutorInput;
use super::restir_storage::RestirStorage;
use super::surface::RebuiltSurfaceResources;
use crate::renderer::world::upload::UploadedWorldResources;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceVersionState {
    world_generation: u64,
    surface_generation: u64,
}

impl ResourceVersionState {
    pub const fn world_generation(self) -> u64 {
        self.world_generation
    }

    pub const fn surface_generation(self) -> u64 {
        self.surface_generation
    }

    pub const fn dependency_signature(self) -> u64 {
        (self.world_generation << 32) | (self.surface_generation & 0xFFFF_FFFF)
    }

    pub fn on_world_upload(&mut self) {
        self.world_generation = self.world_generation.saturating_add(1);
    }

    pub fn on_surface_rebuild(&mut self) {
        self.surface_generation = self.surface_generation.saturating_add(1);
    }
}

#[derive(Debug)]
pub struct WorldUploadResources {
    pub voxel_buffer: wgpu::Buffer,
    pub chunk_meta_buffer: wgpu::Buffer,
    pub chunk_map_buffer: wgpu::Buffer,
    pub emissive_voxel_buffer: wgpu::Buffer,
    pub emissive_cdf_buffer: wgpu::Buffer,
    pub emissive_remap_buffer: wgpu::Buffer,
    pub importance_map_texture: wgpu::Texture,
    pub importance_map_view: wgpu::TextureView,
}

impl From<UploadedWorldResources> for WorldUploadResources {
    fn from(value: UploadedWorldResources) -> Self {
        Self {
            voxel_buffer: value.voxel_buffer,
            chunk_meta_buffer: value.chunk_meta_buffer,
            chunk_map_buffer: value.chunk_map_buffer,
            emissive_voxel_buffer: value.emissive_voxel_buffer,
            emissive_cdf_buffer: value.emissive_cdf_buffer,
            emissive_remap_buffer: value.emissive_remap_buffer,
            importance_map_texture: value.importance_map_texture,
            importance_map_view: value.importance_map_view,
        }
    }
}

#[derive(Debug)]
pub struct SurfaceHistoryResources {
    pub output_texture: wgpu::Texture,
    pub output_view: wgpu::TextureView,
    pub accumulation_buffer: wgpu::Buffer,
    pub restir_storage: RestirStorage,
    pub svgf_ping_buffer: wgpu::Buffer,
    pub svgf_pong_buffer: wgpu::Buffer,
    pub svgf_debug_buffer: wgpu::Buffer,
    pub svgf_init_uniform_buffer: wgpu::Buffer,
    pub svgf_resolve_uniform_buffer: wgpu::Buffer,
    pub svgf_atrous_uniform_buffers: Vec<wgpu::Buffer>,
}

impl From<RebuiltSurfaceResources> for SurfaceHistoryResources {
    fn from(value: RebuiltSurfaceResources) -> Self {
        Self {
            output_texture: value.output_texture,
            output_view: value.output_view,
            accumulation_buffer: value.accumulation_buffer,
            restir_storage: value.restir_storage,
            svgf_ping_buffer: value.svgf_ping_buffer,
            svgf_pong_buffer: value.svgf_pong_buffer,
            svgf_debug_buffer: value.svgf_debug_buffer,
            svgf_init_uniform_buffer: value.svgf_init_uniform_buffer,
            svgf_resolve_uniform_buffer: value.svgf_resolve_uniform_buffer,
            svgf_atrous_uniform_buffers: value.svgf_atrous_uniform_buffers,
        }
    }
}

#[derive(Debug)]
pub struct RendererResourceContext {
    pub world: WorldUploadResources,
    pub surface: SurfaceHistoryResources,
    versions: ResourceVersionState,
}

impl RendererResourceContext {
    pub fn new(world: WorldUploadResources, surface: SurfaceHistoryResources) -> Self {
        Self {
            world,
            surface,
            versions: ResourceVersionState::default(),
        }
    }

    pub fn versions(&self) -> ResourceVersionState {
        self.versions
    }

    pub fn apply_world_upload(&mut self, resources: UploadedWorldResources) {
        self.world = resources.into();
        self.versions.on_world_upload();
    }

    pub fn apply_surface_rebuild(&mut self, resources: RebuiltSurfaceResources) {
        self.surface = resources.into();
        self.versions.on_surface_rebuild();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bind_group_input<'a>(
        &'a self,
        device: &'a wgpu::Device,
        trace_layout: &'a wgpu::BindGroupLayout,
        svgf_layout: &'a wgpu::BindGroupLayout,
        camera_buffer: &'a wgpu::Buffer,
        previous_camera_buffer: &'a wgpu::Buffer,
        tracer_uniform_buffer: &'a wgpu::Buffer,
    ) -> BindGroupExecutorInput<'a> {
        BindGroupExecutorInput {
            device,
            trace_layout,
            svgf_layout,
            output_view: &self.surface.output_view,
            accumulation_buffer: &self.surface.accumulation_buffer,
            camera_buffer,
            previous_camera_buffer,
            tracer_uniform_buffer,
            voxel_buffer: &self.world.voxel_buffer,
            chunk_meta_buffer: &self.world.chunk_meta_buffer,
            chunk_map_buffer: &self.world.chunk_map_buffer,
            emissive_voxel_buffer: &self.world.emissive_voxel_buffer,
            emissive_cdf_buffer: &self.world.emissive_cdf_buffer,
            emissive_remap_buffer: &self.world.emissive_remap_buffer,
            importance_map_view: &self.world.importance_map_view,
            svgf_init_uniform_buffer: &self.surface.svgf_init_uniform_buffer,
            svgf_resolve_uniform_buffer: &self.surface.svgf_resolve_uniform_buffer,
            svgf_atrous_uniform_buffers: &self.surface.svgf_atrous_uniform_buffers,
            svgf_ping_buffer: &self.surface.svgf_ping_buffer,
            svgf_pong_buffer: &self.surface.svgf_pong_buffer,
            svgf_debug_buffer: &self.surface.svgf_debug_buffer,
            restir_bindings: self.surface.restir_storage.bindings(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ResourceVersionState;

    #[test]
    fn resource_versions_start_at_zero() {
        let versions = ResourceVersionState::default();
        assert_eq!(versions.world_generation(), 0);
        assert_eq!(versions.surface_generation(), 0);
        assert_eq!(versions.dependency_signature(), 0);
    }

    #[test]
    fn world_upload_updates_only_world_generation() {
        let mut versions = ResourceVersionState::default();
        versions.on_world_upload();
        assert_eq!(versions.world_generation(), 1);
        assert_eq!(versions.surface_generation(), 0);
    }

    #[test]
    fn surface_rebuild_updates_only_surface_generation() {
        let mut versions = ResourceVersionState::default();
        versions.on_surface_rebuild();
        assert_eq!(versions.world_generation(), 0);
        assert_eq!(versions.surface_generation(), 1);
    }

    #[test]
    fn dependency_signature_tracks_both_generations() {
        let mut versions = ResourceVersionState::default();
        versions.on_world_upload();
        versions.on_surface_rebuild();
        versions.on_surface_rebuild();
        let signature = versions.dependency_signature();
        assert_eq!(signature >> 32, 1);
        assert_eq!(signature & 0xFFFF_FFFF, 2);
    }

    #[test]
    fn world_generation_saturates_at_max() {
        let mut versions = ResourceVersionState {
            world_generation: u64::MAX,
            surface_generation: 0,
        };
        versions.on_world_upload();
        assert_eq!(versions.world_generation(), u64::MAX);
        assert_eq!(versions.surface_generation(), 0);
    }

    #[test]
    fn surface_generation_saturates_at_max() {
        let mut versions = ResourceVersionState {
            world_generation: 0,
            surface_generation: u64::MAX,
        };
        versions.on_surface_rebuild();
        assert_eq!(versions.world_generation(), 0);
        assert_eq!(versions.surface_generation(), u64::MAX);
    }

    #[test]
    fn dependency_signature_uses_lower_32_bits_of_surface_generation() {
        let versions = ResourceVersionState {
            world_generation: 7,
            surface_generation: 0x1_0000_0005,
        };
        assert_eq!(versions.dependency_signature(), (7u64 << 32) | 5u64);
    }
}
