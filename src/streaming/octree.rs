//! Flat-array octree for LOD node enumeration.
//!
//! `StreamingOctree` stores nodes in a `Vec` (breadth-first order).
//! Each node carries chunk-space coordinates and its LOD level.
//! The tree is built once from a `radius_chunks` + `levels` config and
//! then read-only during traversal.
//!
//! A `key_index` (`HashMap<ChunkKey, usize>`) provides O(1) lookup by key.
//! `recenter()` returns an `OctreeDiff` so callers know exactly which
//! nodes were added/removed without a full set comparison.

use std::collections::{HashMap, HashSet};

use super::types::ChunkKey;

// ---------------------------------------------------------------------------
// OctreeDiff
// ---------------------------------------------------------------------------

/// Delta produced by [`StreamingOctree::recenter`] when the centre actually
/// moves.  Contains the chunk keys that appeared/disappeared relative to the
/// previous octree state.
#[derive(Debug, Clone, Default)]
pub struct OctreeDiff {
    /// Keys present in the *new* octree but absent from the old one.
    pub added: Vec<ChunkKey>,
    /// Keys present in the *old* octree but absent from the new one.
    pub removed: Vec<ChunkKey>,
}

// ---------------------------------------------------------------------------
// OctreeNode
// ---------------------------------------------------------------------------

/// A single node in the streaming octree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OctreeNode {
    pub key: ChunkKey,
    /// Index of the parent node in the flat array, or `None` for the root.
    pub parent: Option<usize>,
    /// Indices of child nodes in the flat array.
    pub children: Vec<usize>,
}

impl OctreeNode {
    pub fn new(key: ChunkKey, parent: Option<usize>) -> Self {
        Self {
            key,
            parent,
            children: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// StreamingOctree
// ---------------------------------------------------------------------------

/// Flat-array octree covering `(-radius..=radius)^3` chunks at each LOD,
/// centred on a given chunk-space origin.
///
/// `levels` is the number of LOD tiers (e.g. 3 means LOD0, LOD1, LOD2).
/// `radius_chunks` is the half-extent at the finest LOD; coarser levels
/// cover proportionally fewer nodes because each coarse chunk represents
/// 8× the world volume.
///
/// **Build strategy:** the tree is populated level by level starting at
/// `lod = levels-1` (coarsest, root tier) down to `lod = 0` (finest).
/// Each coarse node at level `L` is subdivided into up to 8 children at
/// level `L-1`.
pub struct StreamingOctree {
    nodes: Vec<OctreeNode>,
    /// O(1) lookup: `ChunkKey → index` into `nodes`.
    key_index: HashMap<ChunkKey, usize>,
    /// Configuration retained for rebuild (B5 fix: dynamic octree).
    radius_chunks: i32,
    levels: u8,
    /// Current centre in chunk-space at LOD 0 (B5 fix).
    center: [i32; 3],
}

impl StreamingOctree {
    /// Build the octree centred at the origin.
    ///
    /// * `radius_chunks` — half-extent in chunk-space at LOD 0 (finest).
    ///   Must be >= 1.
    /// * `levels` — number of LOD levels. Must be >= 1.
    pub fn build(radius_chunks: i32, levels: u8) -> Self {
        Self::build_at(radius_chunks, levels, [0, 0, 0])
    }

    /// Build the octree centred at a given chunk-space coordinate (B5 fix).
    pub fn build_at(radius_chunks: i32, levels: u8, center: [i32; 3]) -> Self {
        assert!(radius_chunks >= 1, "radius_chunks must be >= 1");
        assert!(levels >= 1, "levels must be >= 1");

        let mut nodes: Vec<OctreeNode> = Vec::new();

        // Build from coarsest level down to finest.
        // At coarsest level (lod = levels-1) the radius is
        // radius_chunks / 2^(levels-1), minimum 1.
        let _max_lod = (levels - 1) as i32;

        // We use a simple approach: for each lod level (coarse to fine)
        // enumerate all chunk coordinates within the scaled radius.
        // Parent-child links are established by mapping coordinates.

        // level_start[l] = first index in `nodes` for LOD l
        let mut level_start: Vec<usize> = vec![0; levels as usize];
        let mut level_index: Vec<std::collections::HashMap<(i32, i32, i32), usize>> = (0..levels)
            .map(|_| std::collections::HashMap::new())
            .collect();

        // Insert nodes coarsest -> finest so parent indices are known.
        for lod in (0..levels).rev() {
            let scale = 1i32 << (lod as i32); // chunks-per-coarse-cell at lod0
            // radius in coarse-chunk units
            let r = std::cmp::max(1, radius_chunks / scale);
            level_start[lod as usize] = nodes.len();
            for cx in (-r + center[0])..=(r + center[0]) {
                for cy in (-r + center[1])..=(r + center[1]) {
                    for cz in (-r + center[2])..=(r + center[2]) {
                        let key = ChunkKey::new(cx, cy, cz, lod);
                        // Find parent (one coarser level)
                        let parent_idx = if lod < levels - 1 {
                            let parent_lod = lod + 1;
                            let parent_scale = 1i32 << (parent_lod as i32);
                            let pr = std::cmp::max(1, radius_chunks / parent_scale);
                            // Map this node's coords to parent coords
                            let px = div_floor(cx * scale, parent_scale);
                            let py = div_floor(cy * scale, parent_scale);
                            let pz = div_floor(cz * scale, parent_scale);
                            // MED-12: Skip (don't create link) if parent coords
                            // are out of range — prevents incorrect topology.
                            // Note: parent range is also centred (B5).
                            let parent_center_x = center[0]; // In chunk-space at this parent LOD
                            let parent_center_y = center[1];
                            let parent_center_z = center[2];
                            if px < -pr + parent_center_x || px > pr + parent_center_x
                                || py < -pr + parent_center_y || py > pr + parent_center_y
                                || pz < -pr + parent_center_z || pz > pr + parent_center_z {
                                None
                            } else {
                                level_index[parent_lod as usize].get(&(px, py, pz)).copied()
                            }
                        } else {
                            None
                        };
                        let idx = nodes.len();
                        nodes.push(OctreeNode::new(key, parent_idx));
                        level_index[lod as usize].insert((cx, cy, cz), idx);
                        // Register as child of parent
                        if let Some(p) = parent_idx {
                            nodes[p].children.push(idx);
                        }
                    }
                }
            }
        }

        // Build the key_index from the final nodes vec.
        let key_index: HashMap<ChunkKey, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.key, i))
            .collect();

        Self { nodes, key_index, radius_chunks, levels, center }
    }

    /// Iterate all nodes in the tree.
    pub fn nodes(&self) -> &[OctreeNode] {
        &self.nodes
    }

    /// Number of nodes.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Current octree centre in chunk-space (B5 fix).
    pub fn center(&self) -> [i32; 3] {
        self.center
    }

    /// Rebuild the octree centred on a new chunk-space coordinate (B5 fix).
    /// Returns `Some(OctreeDiff)` when the centre actually changed, or `None`
    /// if the hysteresis threshold was not exceeded.
    ///
    /// Uses hysteresis: only rebuilds when the new centre is ≥2 chunks away
    /// (Manhattan distance) to prevent per-frame octree thrash.
    pub fn recenter(&mut self, new_center: [i32; 3]) -> Option<OctreeDiff> {
        let dx = (new_center[0] - self.center[0]).abs();
        let dy = (new_center[1] - self.center[1]).abs();
        let dz = (new_center[2] - self.center[2]).abs();
        if dx + dy + dz < 2 {
            return None;
        }

        // Capture old key set before rebuild.
        let old_keys: HashSet<ChunkKey> = self.key_index.keys().copied().collect();

        // Rebuild the octree at the new centre.
        *self = Self::build_at(self.radius_chunks, self.levels, new_center);

        // Compute diff between old and new key sets.
        let new_keys: HashSet<ChunkKey> = self.key_index.keys().copied().collect();

        let added: Vec<ChunkKey> = new_keys.difference(&old_keys).copied().collect();
        let removed: Vec<ChunkKey> = old_keys.difference(&new_keys).copied().collect();

        Some(OctreeDiff { added, removed })
    }

    /// Check whether the octree contains a node with the given key.
    pub fn contains_key(&self, key: &ChunkKey) -> bool {
        self.key_index.contains_key(key)
    }

    /// Look up a node by its `ChunkKey` in O(1).
    pub fn node_by_key(&self, key: &ChunkKey) -> Option<&OctreeNode> {
        self.key_index.get(key).map(|&idx| &self.nodes[idx])
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Floor division (rounds toward negative infinity).
#[inline]
fn div_floor(a: i32, b: i32) -> i32 {
    let d = a / b;
    let r = a % b;
    if (r != 0) && ((r < 0) != (b < 0)) {
        d - 1
    } else {
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octree_builds_without_panic() {
        // Should not panic with minimal config
        let tree = StreamingOctree::build(1, 1);
        assert!(!tree.is_empty());
    }

    #[test]
    fn octree_nodes_have_correct_lod() {
        let tree = StreamingOctree::build(1, 2);
        // All nodes must have lod_level < 2
        for node in tree.nodes() {
            assert!(node.key.lod_level < 2);
        }
    }
}
