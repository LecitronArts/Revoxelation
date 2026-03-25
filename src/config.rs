use serde::Deserialize;
use std::fs;

/// Runtime configuration loaded from an optional `config.toml` file.
///
/// If the file doesn't exist, sensible defaults are used. All fields
/// take effect immediately (simple values) or on the next frame.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    /// Enable Hi-Z occlusion culling (default: true).
    pub hiz_enabled: bool,
    /// Show the debug HUD overlay (default: true).
    pub show_hud: bool,
    /// Camera movement speed in units/sec (default: 20.0).
    pub camera_speed: f32,
    /// Camera vertical field-of-view in degrees (default: 60.0).
    pub camera_fov: f32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            hiz_enabled: true,
            show_hud: true,
            camera_speed: 20.0,
            camera_fov: 60.0,
        }
    }
}

impl RuntimeConfig {
    /// Load from `config.toml` if it exists; otherwise return defaults.
    pub fn load() -> Self {
        match fs::read_to_string("config.toml") {
            Ok(contents) => match toml::from_str::<RuntimeConfig>(&contents) {
                Ok(config) => {
                    log::info!("Loaded runtime config from config.toml");
                    config
                }
                Err(e) => {
                    log::warn!("Failed to parse config.toml: {e} — using defaults");
                    Self::default()
                }
            },
            Err(_) => {
                log::info!("No config.toml found — using defaults");
                Self::default()
            }
        }
    }
}
