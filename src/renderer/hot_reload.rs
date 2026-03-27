//! Shader hot-reload support (debug builds only).
//!
//! Every `CHECK_INTERVAL` frames, checks modification times of shader source
//! files. If a file has changed, recompiles it via `shaderc`, destroys the
//! old pipeline, and recreates it using the pipeline cache.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;
use ash::vk;

use super::Renderer;
use super::spirv::create_shader_module;

/// How often (in frames) to poll shader file modification times.
const CHECK_INTERVAL: u64 = 60;

/// Tracks shader files and their last-known modification times.
pub struct ShaderHotReload {
    /// Map of shader source path → last modification time.
    mod_times: HashMap<String, SystemTime>,
    /// Frame counter for interval-based checking.
    frame_counter: u64,
    /// Lazily initialized shaderc compiler.
    compiler: Option<shaderc::Compiler>,
}

impl Default for ShaderHotReload {
    fn default() -> Self {
        Self::new()
    }
}

impl ShaderHotReload {
    /// Create a new hot-reload tracker for all known shader source files.
    pub fn new() -> Self {
        let mut mod_times = HashMap::new();
        for path in super::shader_source_files() {
            if let Ok(metadata) = fs::metadata(path)
                && let Ok(modified) = metadata.modified() {
                    mod_times.insert(path.to_string(), modified);
                }
        }
        Self {
            mod_times,
            frame_counter: 0,
            compiler: None,
        }
    }

    /// Check for shader changes and reload if needed. Call once per frame.
    ///
    /// Returns `Ok(true)` if any shader was reloaded, `Ok(false)` otherwise.
    pub fn check_and_reload(&mut self, renderer: &mut Renderer) -> Result<bool> {
        self.frame_counter += 1;
        if !self.frame_counter.is_multiple_of(CHECK_INTERVAL) {
            return Ok(false);
        }

        let mut changed_shaders: Vec<String> = Vec::new();

        for path_str in super::shader_source_files() {
            let Ok(metadata) = fs::metadata(path_str) else {
                continue;
            };
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            let prev = self.mod_times.get(*path_str);
            if prev.is_none_or(|prev_time| modified > *prev_time) {
                changed_shaders.push(path_str.to_string());
                self.mod_times.insert(path_str.to_string(), modified);
            }
        }

        if changed_shaders.is_empty() {
            return Ok(false);
        }

        log::info!("Shader hot-reload: detected changes in {:?}", changed_shaders);

        // Lazily create the compiler.
        if self.compiler.is_none() {
            self.compiler = shaderc::Compiler::new().ok();
            if self.compiler.is_none() {
                log::error!("Shader hot-reload: failed to create shaderc compiler");
                return Ok(false);
            }
        }
        let compiler = self.compiler.as_ref().unwrap();

        // Wait for GPU idle before recreating pipelines.
        unsafe {
            let _ = renderer.device_ctx.device.device_wait_idle();
        }

        let device = renderer.device_ctx.device.clone();
        let cache_handle = renderer
            .pipeline_cache
            .as_ref()
            .map(|c| c.handle())
            .unwrap_or(vk::PipelineCache::null());

        for shader_path in &changed_shaders {
            let result = self.reload_single_shader(
                &device,
                cache_handle,
                renderer,
                compiler,
                shader_path,
            );
            match result {
                Ok(()) => log::info!("Shader hot-reload: successfully reloaded {}", shader_path),
                Err(e) => log::error!("Shader hot-reload: failed to reload {}: {e:#}", shader_path),
            }
        }

        Ok(true)
    }

    fn reload_single_shader(
        &self,
        device: &ash::Device,
        cache_handle: vk::PipelineCache,
        renderer: &mut Renderer,
        compiler: &shaderc::Compiler,
        shader_path: &str,
    ) -> Result<()> {
        let source = fs::read_to_string(shader_path)?;
        let kind = shader_kind(shader_path)?;
        let artifact = compiler
            .compile_into_spirv(&source, kind, shader_path, "main", None)
            .map_err(|e| anyhow::anyhow!("shader compilation failed: {e}"))?;

        let spv_bytes = artifact.as_binary_u8();
        let new_module = create_shader_module(device, spv_bytes)?;

        // Determine which pipeline to recreate based on the shader file name.
        let file_name = Path::new(shader_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        

        if file_name.starts_with("chunk_mesh") {
            self.rebuild_mesh_pipeline(device, cache_handle, renderer, new_module, &file_name)
        } else if file_name.starts_with("chunk_cull") {
            self.rebuild_cull_pipeline(device, cache_handle, renderer, new_module)
        } else {
            // Shaders we don't hot-reload (hiz_generate, egui) — just log and skip.
            unsafe { device.destroy_shader_module(new_module, None); }
            log::info!("Shader hot-reload: no pipeline rebuild for {}", shader_path);
            Ok(())
        }
    }

    fn rebuild_mesh_pipeline(
        &self,
        device: &ash::Device,
        _cache_handle: vk::PipelineCache,
        renderer: &mut Renderer,
        new_module: vk::ShaderModule,
        _file_name: &str,
    ) -> Result<()> {
        // For mesh pipeline, we need both vert and frag — just destroy and recreate via the normal path.
        unsafe { device.destroy_shader_module(new_module, None); }

        if let Some(old_pipeline) = renderer.mesh_pipeline.take() {
            old_pipeline.destroy(renderer);
        }
        let bindless_layout = renderer.bindless.as_ref()
            .expect("bindless must be initialized for hot-reload")
            .descriptor_set_layout;
        renderer.mesh_pipeline = Some(super::mesh_pipeline::ChunkMeshPipeline::new(renderer, bindless_layout)?);
        Ok(())
    }

    fn rebuild_cull_pipeline(
        &self,
        device: &ash::Device,
        _cache_handle: vk::PipelineCache,
        renderer: &mut Renderer,
        new_module: vk::ShaderModule,
    ) -> Result<()> {
        unsafe { device.destroy_shader_module(new_module, None); }

        if let Some(old_pipeline) = renderer.cull_pipeline.take() {
            old_pipeline.destroy(renderer);
        }
        let bindless_layout = renderer.bindless.as_ref()
            .expect("bindless must be initialized for hot-reload")
            .descriptor_set_layout;
        renderer.cull_pipeline = Some(super::cull_pipeline::ChunkCullPipeline::new(renderer, bindless_layout)?);
        Ok(())
    }
}

fn shader_kind(path: &str) -> Result<shaderc::ShaderKind> {
    if path.ends_with(".vert") {
        Ok(shaderc::ShaderKind::Vertex)
    } else if path.ends_with(".frag") {
        Ok(shaderc::ShaderKind::Fragment)
    } else if path.ends_with(".comp") {
        Ok(shaderc::ShaderKind::Compute)
    } else {
        Err(anyhow::anyhow!("unsupported shader extension for {path}"))
    }
}
