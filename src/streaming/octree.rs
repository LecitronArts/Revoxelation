//! Flat-array octree for LOD node enumeration.
//!
//! `StreamingOctree` stores nodes in a `Vec` (breadth-first order).
//! Each node carries chunk-space coordinates and its LOD level.
//! The tree is built once from a `radius_chunks` + `levels` config and
//! then read-only during traversal.

use super::types::ChunkKey;

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

/// Flat-array octree covering `(-radius..=radius)^3` chunks at each LOD.
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
}

impl StreamingOctree {
    /// Build the octree.
    ///
    /// * `radius_chunks` — half-extent in chunk-space at LOD 0 (finest).
    ///   Must be >= 1.
    /// * `levels` — number of LOD levels. Must be >= 1.
    pub fn build(radius_chunks: i32, levels: u8) -> Self {
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
            for cx in -r..=r {
                for cy in -r..=r {
                    for cz in -r..=r {
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
                            // Clamp to parent radius
                            let px = px.clamp(-pr, pr);
                            let py = py.clamp(-pr, pr);
                            let pz = pz.clamp(-pr, pr);
                            level_index[parent_lod as usize].get(&(px, py, pz)).copied()
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

        Self { nodes }
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
