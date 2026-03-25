use std::sync::{Mutex, OnceLock};

use anyhow::{Result, anyhow};

use super::Renderer;

static RENDERER: OnceLock<Mutex<Renderer>> = OnceLock::new();

pub fn install_renderer(renderer: Renderer) -> Result<()> {
    RENDERER
        .set(Mutex::new(renderer))
        .map_err(|_| anyhow!("renderer already initialized"))
}

pub fn renderer_state() -> Option<&'static Mutex<Renderer>> {
    RENDERER.get()
}
