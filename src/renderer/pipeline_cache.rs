use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use ash::vk;

/// File path for the persistent pipeline cache binary.
const CACHE_PATH: &str = "cache/pipeline.bin";

/// Wrapper around `vk::PipelineCache` that supports loading from and saving to disk.
///
/// On startup the cache is loaded from `cache/pipeline.bin` if the file exists;
/// otherwise an empty pipeline cache is created.  On shutdown (via `save`) the
/// current cache data is written back so subsequent runs benefit from faster
/// pipeline creation.
pub struct PipelineCache {
    handle: vk::PipelineCache,
}

impl PipelineCache {
    /// Load pipeline cache from disk (if available) or create an empty one.
    pub fn load(device: &ash::Device) -> Result<Self> {
        let initial_data = match fs::read(CACHE_PATH) {
            Ok(data) => {
                log::info!(
                    "Loaded pipeline cache from {} ({} bytes)",
                    CACHE_PATH,
                    data.len()
                );
                data
            }
            Err(_) => {
                log::info!(
                    "No existing pipeline cache at {} — creating empty",
                    CACHE_PATH
                );
                Vec::new()
            }
        };

        let create_info = vk::PipelineCacheCreateInfo::default().initial_data(&initial_data);

        let handle = unsafe {
            device
                .create_pipeline_cache(&create_info, None)
                .context("failed to create Vulkan pipeline cache")?
        };

        Ok(Self { handle })
    }

    /// Return the raw `vk::PipelineCache` handle for use in pipeline creation calls.
    pub fn handle(&self) -> vk::PipelineCache {
        self.handle
    }

    /// Persist the current pipeline cache data to `cache/pipeline.bin`.
    pub fn save(&self, device: &ash::Device) -> Result<()> {
        let data = unsafe {
            device
                .get_pipeline_cache_data(self.handle)
                .context("failed to get pipeline cache data")?
        };

        // Ensure the cache directory exists.
        if let Some(parent) = Path::new(CACHE_PATH).parent() {
            fs::create_dir_all(parent)
                .context("failed to create cache directory for pipeline cache")?;
        }

        fs::write(CACHE_PATH, &data).context("failed to write pipeline cache to disk")?;

        log::info!(
            "Saved pipeline cache to {} ({} bytes)",
            CACHE_PATH,
            data.len()
        );

        Ok(())
    }

    /// Destroy the Vulkan pipeline cache handle.
    ///
    /// Call `save` before this if you want to persist the data.
    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_pipeline_cache(self.handle, None);
        }
    }
}
