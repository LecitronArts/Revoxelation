use anyhow::{Context, Result, anyhow};
use ash::vk;

pub fn decode_spirv_words(bytes: &[u8]) -> Result<Vec<u32>> {
    if !bytes.len().is_multiple_of(4) {
        return Err(anyhow!("SPIR-V byte length must be a multiple of 4"));
    }

    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

pub fn create_shader_module(device: &ash::Device, bytes: &[u8]) -> Result<vk::ShaderModule> {
    let code = decode_spirv_words(bytes)?;
    unsafe {
        device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
            .context("failed to create shader module")
    }
}
