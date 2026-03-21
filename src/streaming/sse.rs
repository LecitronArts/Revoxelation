//! Screen-space error computation and active-set diffing.

use std::collections::HashSet;

use super::{
    octree::{StreamingOctree},
    types::{ChunkKey, LodConfig, SseConfig},
};

// ---------------------------------------------------------------------------
// compute_sse
// ---------------------------------------------------------------------------

/// Compute the screen-space error (in pixels) for a chunk node.
///
/// Formula: `sse = (geometric_error * screen_height) / (2 * dist * tan(fov/2))`
///
/// Guards:
/// - If `dist <= 0` the camera is inside the chunk; returns `f32::MAX`.
/// - If any intermediate value is non-finite the result is clamped to
///   `f32::MAX` (never NaN).
pub fn compute_sse(lod: &LodConfig, cfg: &SseConfig, dist: f32) -> f32 {
    if dist <= 0.0 {
        return f32::MAX;
    }

    let half_fov_tan = (cfg.fov_y_radians * 0.5).tan();
    if half_fov_tan <= 0.0 {
        return f32::MAX;
    }

    let denom = 2.0 * dist * half_fov_tan;
    if denom == 0.0 {
        return f32::MAX;
    }

    let raw = (lod.geometric_error * cfg.screen_height) / denom;

    if raw.is_nan() || raw.is_infinite() {
        f32::MAX
    } else {
        raw
    }
}

// ---------------------------------------------------------------------------
// diff_active_set
// ---------------------------------------------------------------------------

/// Result of comparing the desired active set against what is currently active.
#[derive(Debug, Default)]
pub struct ActiveSetDiff {
    /// Keys that need to become active (not yet in current_active).
    pub to_activate: Vec<ChunkKey>,
    /// Keys that should be deactivated (no longer needed).
    pub to_deactivate: Vec<ChunkKey>,
}

/// Walk `octree`, compute SSE for each node using `camera_dist_fn`, and
/// produce the delta relative to `current_active`.
///
/// A node is "desired" when its SSE >= `cfg.threshold_px`.
/// Nodes in `desired` but not in `current_active` → `to_activate`.
/// Nodes in `current_active` but not in `desired`  → `to_deactivate`.
///
/// `camera_dist_fn(key)` returns the distance in metres from the camera
/// to the chunk centre.
pub fn diff_active_set(
    octree: &StreamingOctree,
    lod_configs: &[LodConfig],
    cfg: &SseConfig,
    current_active: &HashSet<ChunkKey>,
    camera_dist_fn: impl Fn(&ChunkKey) -> f32,
) -> ActiveSetDiff {
    let mut desired: HashSet<ChunkKey> = HashSet::new();

    for node in octree.nodes() {
        let key = &node.key;
        let lod_idx = key.lod_level as usize;
        let lod = match lod_configs.get(lod_idx) {
            Some(l) => l,
            None => continue,
        };

        let dist = camera_dist_fn(key);
        let sse = compute_sse(lod, cfg, dist);

        if sse >= cfg.threshold_px {
            desired.insert(*key);
        }
    }

    let to_activate = desired
        .iter()
        .filter(|k| !current_active.contains(k))
        .copied()
        .collect();

    let to_deactivate = current_active
        .iter()
        .filter(|k| !desired.contains(k))
        .copied()
        .collect();

    ActiveSetDiff { to_activate, to_deactivate }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{
        octree::StreamingOctree,
        types::{LodConfig, SseConfig},
    };

    fn base_cfg() -> SseConfig {
        SseConfig::new(1080.0, std::f32::consts::FRAC_PI_2, 2.0, false)
    }

    fn base_lod() -> LodConfig {
        // geometric_error=4.0, dist=10.0, screen_h=1080, fov=PI/2
        // tan(PI/4) = 1.0  => denom = 2*10*1 = 20
        // sse = (4*1080)/20 = 216 px
        LodConfig::new(4.0, 4.0)
    }

    // -----------------------------------------------------------------------
    // sse_known_value
    // -----------------------------------------------------------------------
    #[test]
    fn sse_known_value() {
        let lod = base_lod();
        let cfg = base_cfg();
        let sse = compute_sse(&lod, &cfg, 10.0);
        // expected: (4.0 * 1080.0) / (2 * 10.0 * tan(PI/4))
        //         = 4320 / 20 = 216.0
        assert!(
            (sse - 216.0).abs() < 0.01,
            "expected ~216.0, got {sse}"
        );
    }

    // -----------------------------------------------------------------------
    // sse_zero_dist
    // -----------------------------------------------------------------------
    #[test]
    fn sse_zero_dist() {
        let lod = base_lod();
        let cfg = base_cfg();
        let sse = compute_sse(&lod, &cfg, 0.0);
        assert_eq!(sse, f32::MAX, "zero dist must return f32::MAX");
    }

    // -----------------------------------------------------------------------
    // sse_no_nan
    // -----------------------------------------------------------------------
    #[test]
    fn sse_no_nan() {
        let lod = LodConfig::new(f32::INFINITY, 4.0);
        let cfg = base_cfg();
        let sse = compute_sse(&lod, &cfg, 10.0);
        assert!(
            sse.is_finite() || sse == f32::MAX,
            "result must not be NaN; got {sse}"
        );
    }

    // -----------------------------------------------------------------------
    // diff_activate_all
    // -----------------------------------------------------------------------
    #[test]
    fn diff_activate_all() {
        // Build tiny octree: radius=1, levels=1 => only LOD-0 nodes
        let octree = StreamingOctree::build(1, 1);
        let lod_configs = vec![LodConfig::new(4.0, 4.0)];
        let cfg = base_cfg(); // threshold = 2 px
        let current_active: HashSet<ChunkKey> = HashSet::new();

        // dist = 10.0 everywhere => SSE = 216 >> 2 => all nodes desired
        let diff = diff_active_set(
            &octree,
            &lod_configs,
            &cfg,
            &current_active,
            |_| 10.0,
        );

        assert!(!diff.to_activate.is_empty(), "should activate all nodes");
        assert!(diff.to_deactivate.is_empty());
    }

    // -----------------------------------------------------------------------
    // diff_deactivate_none
    // -----------------------------------------------------------------------
    #[test]
    fn diff_deactivate_none() {
        let octree = StreamingOctree::build(1, 1);
        let lod_configs = vec![LodConfig::new(4.0, 4.0)];
        let cfg = base_cfg();

        // Prime current_active with all octree nodes
        let current_active: HashSet<ChunkKey> =
            octree.nodes().iter().map(|n| n.key).collect();

        // Same dist => same desired set => nothing to activate or deactivate
        let diff = diff_active_set(
            &octree,
            &lod_configs,
            &cfg,
            &current_active,
            |_| 10.0,
        );

        assert!(diff.to_activate.is_empty(), "nothing new to activate");
        assert!(diff.to_deactivate.is_empty(), "nothing to deactivate");
    }
}
