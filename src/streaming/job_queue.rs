//! Bounded priority queue for chunk background jobs.
//!
//! `ChunkJobQueue` is a thread-safe, capacity-bounded ordered set backed by a
//! `BTreeSet`, ordered by screen-space error (SSE). When the queue is full the
//! lowest-SSE task is evicted to make room for higher-priority work.
//!
//! All mutating operations are O(log n) per element instead of the O(n log n)
//! drain-and-rebuild that the previous `BinaryHeap` implementation required.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::Mutex;

use super::types::ChunkKey;

// ---------------------------------------------------------------------------
// PrioritizedTask
// ---------------------------------------------------------------------------

/// A queued chunk load/generate task ordered by screen-space error.
///
/// `sse_bits` stores the raw bit pattern of the `f32` SSE value so that
/// comparison is cheap and deterministic (NaN-free; callers must guarantee
/// this via `compute_sse`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrioritizedTask {
    pub key: ChunkKey,
    pub lod_level: u8,
    /// `f32::to_bits(sse_value)` -- higher bits == higher priority.
    pub sse_bits: u32,
}

impl PrioritizedTask {
    pub fn new(key: ChunkKey, lod_level: u8, sse: f32) -> Self {
        Self {
            key,
            lod_level,
            sse_bits: sse.to_bits(),
        }
    }
}

// BTreeSet ordering: primary key is `sse_bits` (ascending), with a
// deterministic tiebreaker on `(key.x, key.y, key.z, key.lod_level,
// lod_level)` so that two tasks with the same SSE but different chunk
// keys are never considered equal (which would cause silent dedup).
impl PartialOrd for PrioritizedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sse_bits
            .cmp(&other.sse_bits)
            .then_with(|| self.key.x.cmp(&other.key.x))
            .then_with(|| self.key.y.cmp(&other.key.y))
            .then_with(|| self.key.z.cmp(&other.key.z))
            .then_with(|| self.key.lod_level.cmp(&other.key.lod_level))
            .then_with(|| self.lod_level.cmp(&other.lod_level))
    }
}

// ---------------------------------------------------------------------------
// ChunkJobQueue
// ---------------------------------------------------------------------------

/// Thread-safe, capacity-bounded priority queue for chunk jobs.
pub struct ChunkJobQueue {
    inner: Mutex<BTreeSet<PrioritizedTask>>,
    capacity: usize,
}

impl ChunkJobQueue {
    /// Create a new queue with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(BTreeSet::new()),
            capacity,
        }
    }

    /// Enqueue a task. If the queue is at capacity, the task with the lowest
    /// SSE is evicted — but only if the new task has strictly higher SSE.
    /// If the new task has equal or lower SSE than the lowest, it is rejected
    /// and returned instead (HIGH-06).
    pub fn enqueue(&self, task: PrioritizedTask) -> Option<PrioritizedTask> {
        let mut set = self.inner.lock().unwrap();
        if set.len() < self.capacity {
            set.insert(task);
            None
        } else {
            // O(log n): peek at the minimum element.
            let min = set.first().expect("non-empty set");
            // HIGH-06: Reject new task if its SSE is <= the lowest existing task's SSE.
            if task.sse_bits <= min.sse_bits {
                return Some(task); // rejected
            }
            // Evict the lowest-SSE task and insert the new one — both O(log n).
            let evicted = set.pop_first().unwrap();
            set.insert(task);
            Some(evicted)
        }
    }

    /// Pop up to `n` tasks ordered by highest SSE first.
    pub fn drain_up_to(&self, n: usize) -> Vec<PrioritizedTask> {
        let mut set = self.inner.lock().unwrap();
        let count = n.min(set.len());
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(t) = set.pop_last() {
                result.push(t);
            }
        }
        result
    }

    /// Remove all queued tasks matching `key`. Returns `true` if at least one
    /// was found (in-flight tasks are NOT cancelled here; use the cancel flag).
    pub fn cancel_queued(&self, key: ChunkKey) -> bool {
        let mut set = self.inner.lock().unwrap();
        let before = set.len();
        set.retain(|t| t.key != key);
        set.len() < before
    }

    /// Current number of tasks in the queue.
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::types::ChunkKey;

    fn key(n: i32) -> ChunkKey {
        ChunkKey::new(n, 0, 0, 0)
    }

    fn task(n: i32, sse: f32) -> PrioritizedTask {
        PrioritizedTask::new(key(n), 0, sse)
    }

    // -----------------------------------------------------------------------
    // queue_evicts_lowest_sse
    // -----------------------------------------------------------------------
    #[test]
    fn queue_evicts_lowest_sse() {
        let q = ChunkJobQueue::new(2);
        assert!(q.enqueue(task(1, 1.0)).is_none());
        assert!(q.enqueue(task(2, 2.0)).is_none());
        // Queue full; third enqueue should evict sse=1.0
        let evicted = q.enqueue(task(3, 3.0));
        assert!(evicted.is_some(), "expected an eviction");
        assert_eq!(
            evicted.unwrap().sse_bits,
            1.0f32.to_bits(),
            "evicted task should have lowest SSE"
        );
        assert_eq!(q.len(), 2);
    }

    // -----------------------------------------------------------------------
    // queue_drain_highest_first
    // -----------------------------------------------------------------------
    #[test]
    fn queue_drain_highest_first() {
        let q = ChunkJobQueue::new(10);
        q.enqueue(task(1, 1.0));
        q.enqueue(task(2, 3.0));
        q.enqueue(task(3, 2.0));
        let drained = q.drain_up_to(2);
        assert_eq!(drained.len(), 2);
        // First should be highest SSE = 3.0
        assert_eq!(
            drained[0].sse_bits,
            3.0f32.to_bits(),
            "first drained should be highest SSE"
        );
    }

    // -----------------------------------------------------------------------
    // cancel_queued_removes
    // -----------------------------------------------------------------------
    #[test]
    fn cancel_queued_removes() {
        let q = ChunkJobQueue::new(10);
        q.enqueue(task(1, 1.0));
        let found = q.cancel_queued(key(1));
        assert!(found, "cancel should return true when key found");
        assert_eq!(q.len(), 0, "queue should be empty after cancel");
    }
}
