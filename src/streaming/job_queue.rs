//! Bounded priority queue for chunk background jobs.
//!
//! `ChunkJobQueue` is a thread-safe, capacity-bounded max-heap ordered by
//! screen-space error (SSE). When the queue is full the lowest-SSE task is
//! evicted to make room for higher-priority work.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
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

// BinaryHeap is a max-heap; we want highest SSE at the front.
impl PartialOrd for PrioritizedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.sse_bits.cmp(&other.sse_bits)
    }
}

// ---------------------------------------------------------------------------
// ChunkJobQueue
// ---------------------------------------------------------------------------

/// Thread-safe, capacity-bounded priority queue for chunk jobs.
pub struct ChunkJobQueue {
    inner: Mutex<BinaryHeap<PrioritizedTask>>,
    capacity: usize,
}

impl ChunkJobQueue {
    /// Create a new queue with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(BinaryHeap::with_capacity(capacity)),
            capacity,
        }
    }

    /// Enqueue a task. If the queue is at capacity, the task with the lowest
    /// SSE is evicted and returned. Returns `None` if there was room.
    pub fn enqueue(&self, task: PrioritizedTask) -> Option<PrioritizedTask> {
        let mut heap = self.inner.lock().unwrap();
        if heap.len() < self.capacity {
            heap.push(task);
            None
        } else {
            // Collect all items, sort ascending by sse_bits, evict the lowest.
            let mut all: Vec<PrioritizedTask> = heap.drain().collect();
            all.sort_by_key(|t| t.sse_bits);
            let evicted = all.remove(0); // lowest SSE
            // Re-push the remainder plus the new task.
            for t in all {
                heap.push(t);
            }
            heap.push(task);
            Some(evicted)
        }
    }

    /// Pop up to `n` tasks from the max-heap (highest SSE first).
    pub fn drain_up_to(&self, n: usize) -> Vec<PrioritizedTask> {
        let mut heap = self.inner.lock().unwrap();
        let count = n.min(heap.len());
        let mut result = Vec::with_capacity(count);
        for _ in 0..count {
            if let Some(t) = heap.pop() {
                result.push(t);
            }
        }
        result
    }

    /// Remove all queued tasks matching `key`. Returns `true` if at least one
    /// was found (in-flight tasks are NOT cancelled here; use the cancel flag).
    pub fn cancel_queued(&self, key: ChunkKey) -> bool {
        let mut heap = self.inner.lock().unwrap();
        let before = heap.len();
        let remaining: Vec<PrioritizedTask> =
            heap.drain().filter(|t| t.key != key).collect();
        let found = remaining.len() < before;
        for t in remaining {
            heap.push(t);
        }
        found
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
        assert_eq!(evicted.unwrap().sse_bits, 1.0f32.to_bits(), "evicted task should have lowest SSE");
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
        assert_eq!(drained[0].sse_bits, 3.0f32.to_bits(), "first drained should be highest SSE");
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
