use crate::renderer::light_sampler::build_shift_mapping;
use crate::renderer::protocol::{ChunkMapEntryGpu, ChunkMetaGpu, EmissiveVoxelGpu};

use super::payload_builder::{GpuWorldPayload, build_payload};
use crate::world::VoxelWorld;

#[derive(Debug)]
pub struct PreparedWorldSync {
    pub payload: GpuWorldPayload,
    pub remap: Vec<u32>,
}

#[derive(Debug)]
pub struct WorldSyncRejection {
    pub issues: Vec<String>,
    pub reason: String,
}

pub fn prepare_world_sync(
    world: &VoxelWorld,
    frame_index: u32,
    emissive_signatures: &[u32],
    max_storage_binding_size: u64,
) -> Result<PreparedWorldSync, WorldSyncRejection> {
    let payload = build_payload(world);
    let remap = if frame_index == 0 && emissive_signatures.len() == 1 {
        (0..payload.emissive_signatures.len())
            .map(|idx| idx as u32)
            .collect::<Vec<_>>()
    } else {
        build_shift_mapping(emissive_signatures, &payload.emissive_signatures)
    };

    if let Err(issues) =
        validate_world_sync_payload(&payload, remap.len(), max_storage_binding_size)
    {
        let reason = summarize_world_sync_rejection(&issues);
        return Err(WorldSyncRejection { issues, reason });
    }

    Ok(PreparedWorldSync { payload, remap })
}

pub fn record_world_sync_rejection_state(
    reject_count: &mut u32,
    last_reason: &mut String,
    reason: &str,
) {
    *reject_count = reject_count.saturating_add(1);
    *last_reason = reason.to_owned();
}

pub fn record_world_sync_success_state(
    chunk_map_dropped_entries: &mut u32,
    last_reason: &mut String,
    dropped_entries: u32,
) {
    *chunk_map_dropped_entries = dropped_entries;
    last_reason.clear();
}

fn validate_world_sync_payload(
    payload: &GpuWorldPayload,
    remap_len: usize,
    max_storage_binding_size: u64,
) -> std::result::Result<(), Vec<String>> {
    let mut issues = Vec::new();
    check_storage_slice_limit::<u32>(
        "voxel-buffer",
        payload.voxel_words.len(),
        max_storage_binding_size,
        &mut issues,
    );
    check_storage_slice_limit::<ChunkMetaGpu>(
        "chunk-meta-buffer",
        payload.chunk_meta.len(),
        max_storage_binding_size,
        &mut issues,
    );
    check_storage_slice_limit::<ChunkMapEntryGpu>(
        "chunk-map-buffer",
        payload.chunk_map.len(),
        max_storage_binding_size,
        &mut issues,
    );
    check_storage_slice_limit::<EmissiveVoxelGpu>(
        "emissive-voxel-buffer",
        payload.emissive_voxels.len(),
        max_storage_binding_size,
        &mut issues,
    );
    check_storage_slice_limit::<f32>(
        "emissive-cdf-buffer",
        payload.emissive_cdf.len(),
        max_storage_binding_size,
        &mut issues,
    );
    check_storage_slice_limit::<u32>(
        "emissive-remap-buffer",
        remap_len,
        max_storage_binding_size,
        &mut issues,
    );
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn summarize_world_sync_rejection(issues: &[String]) -> String {
    let Some(first) = issues.first() else {
        return "world sync rejected: unknown".to_string();
    };
    if issues.len() == 1 {
        first.clone()
    } else {
        format!("{first} (+{} more)", issues.len().saturating_sub(1))
    }
}

fn check_storage_slice_limit<T>(
    label: &str,
    element_count: usize,
    max_storage_binding_size: u64,
    issues: &mut Vec<String>,
) {
    let stride = std::mem::size_of::<T>() as u64;
    let Some(byte_size) = (element_count as u64).checked_mul(stride) else {
        issues.push(format!(
            "{label}: size overflow (elements={element_count}, stride={stride})"
        ));
        return;
    };
    if byte_size > max_storage_binding_size {
        issues.push(format!(
            "{label}: requires {byte_size} bytes (elements={element_count}, stride={stride}), limit={max_storage_binding_size}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::renderer::light_sampler::INVALID_EMITTER_INDEX;
    use crate::world::{CHUNK_VOLUME, Chunk, ChunkCoord};

    #[test]
    fn world_sync_limit_validation_rejects_oversized_buffers() {
        let payload = GpuWorldPayload {
            voxel_words: vec![0_u32; 512],
            ..GpuWorldPayload::default()
        };
        let result = validate_world_sync_payload(&payload, 1, 64);
        assert!(result.is_err());
        let issues = result.unwrap_err();
        assert!(issues.iter().any(|issue| issue.contains("voxel-buffer")));
    }

    #[test]
    fn sync_rejection_updates_count_and_reason() {
        let mut reject_count = 0_u32;
        let mut reason = String::new();
        let issues = vec![
            "voxel-buffer: requires 1024 bytes, limit=64".to_string(),
            "chunk-map-buffer: requires 512 bytes, limit=64".to_string(),
        ];
        let summarized = summarize_world_sync_rejection(&issues);
        record_world_sync_rejection_state(&mut reject_count, &mut reason, &summarized);
        assert_eq!(reject_count, 1);
        assert!(reason.contains("voxel-buffer"));
        assert!(reason.contains("+1 more"));

        let next_reason = "emissive-cdf-buffer: requires 128 bytes, limit=64";
        record_world_sync_rejection_state(&mut reject_count, &mut reason, next_reason);
        assert_eq!(reject_count, 2);
        assert_eq!(reason, next_reason);
    }

    #[test]
    fn sync_success_clears_reason_and_updates_dropped_entries() {
        let mut reject_count = 0_u32;
        let mut reason = String::new();
        let mut dropped_entries = 0_u32;

        record_world_sync_rejection_state(&mut reject_count, &mut reason, "rejected");
        record_world_sync_success_state(&mut dropped_entries, &mut reason, 3);

        assert_eq!(reject_count, 1);
        assert_eq!(reason, "");
        assert_eq!(dropped_entries, 3);
    }

    #[test]
    fn prepare_world_sync_uses_identity_remap_on_initial_frame() {
        let world = VoxelWorld::new();
        let prepared = prepare_world_sync(&world, 0, &[0], u64::MAX).expect("expected sync plan");
        assert_eq!(prepared.remap, vec![0]);
        assert_eq!(prepared.payload.emissive_signatures, vec![0]);
    }

    #[test]
    fn prepare_world_sync_uses_shift_mapping_after_initial_frame() {
        let world = VoxelWorld::new();
        let prepared =
            prepare_world_sync(&world, 2, &[12345], u64::MAX).expect("expected sync plan");
        assert_eq!(prepared.remap, vec![INVALID_EMITTER_INDEX]);
    }

    #[test]
    fn prepare_world_sync_rejects_oversized_payload_with_reason() {
        let world = VoxelWorld::new();
        let coord = ChunkCoord::new(0, 0, 0);
        world.chunks.insert(
            coord,
            Arc::new(Chunk {
                coord,
                voxels: vec![0_u32; CHUNK_VOLUME],
            }),
        );

        let rejected = prepare_world_sync(&world, 0, &[0], 64).unwrap_err();
        assert!(!rejected.issues.is_empty());
        assert!(
            rejected
                .issues
                .iter()
                .any(|issue| issue.contains("voxel-buffer"))
        );
        assert!(rejected.reason.contains("voxel-buffer"));
    }

    #[test]
    fn summarize_rejection_returns_unknown_for_empty_list() {
        assert_eq!(
            summarize_world_sync_rejection(&[]),
            "world sync rejected: unknown".to_string()
        );
    }

    #[test]
    fn validate_payload_accepts_sizes_at_exact_limit() {
        let payload = GpuWorldPayload {
            voxel_words: vec![0_u32; 8],
            chunk_meta: vec![ChunkMetaGpu::empty()],
            chunk_map: vec![ChunkMapEntryGpu::empty()],
            emissive_voxels: vec![EmissiveVoxelGpu::empty()],
            emissive_cdf: vec![1.0],
            ..GpuWorldPayload::default()
        };
        let bytes = (payload.voxel_words.len() * std::mem::size_of::<u32>()) as u64;
        let result = validate_world_sync_payload(&payload, 1, bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_payload_reports_overflow_issue() {
        let payload = GpuWorldPayload::default();
        let result = validate_world_sync_payload(&payload, usize::MAX, 64);
        assert!(result.is_err());
        let issues = result.unwrap_err();
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("emissive-remap-buffer") && issue.contains("overflow"))
        );
    }
}
