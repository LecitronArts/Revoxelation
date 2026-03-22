use anyhow::{Result, anyhow};

pub fn decode_spirv_words(bytes: &[u8]) -> Result<Vec<u32>> {
    if bytes.len() % 4 != 0 {
        return Err(anyhow!("SPIR-V byte length must be a multiple of 4"));
    }

    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}
