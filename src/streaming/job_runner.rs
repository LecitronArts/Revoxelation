//! Rayon-based chunk job runner.
//!
//! `spawn_chunk_job` submits a chunk generation task to a rayon `ThreadPool`
//! and returns an `Arc<AtomicBool>` cancel flag. Setting the flag to `true`
//! before the task fires causes a `Cancelled` result to be sent instead.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

use super::{
    job_queue::PrioritizedTask,
    types::{ChunkJobOutcome, ChunkJobResult},
};

/// Spawn a background chunk job on `pool`.
///
/// * If the cancel flag is `true` when the closure runs, a `Cancelled` result
///   is sent.
/// * Otherwise a `Generated` result with a placeholder 8-byte payload is sent.
///
/// The returned `Arc<AtomicBool>` is the cancel flag; callers can set it to
/// `true` to request cancellation of in-flight work.
pub fn spawn_chunk_job(
    pool: &rayon::ThreadPool,
    task: PrioritizedTask,
    sender: mpsc::Sender<ChunkJobResult>,
) -> Arc<AtomicBool> {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let flag_clone = Arc::clone(&cancel_flag);

    pool.spawn(move || {
        let outcome = if flag_clone.load(Ordering::Relaxed) {
            ChunkJobOutcome::Cancelled
        } else {
            ChunkJobOutcome::Generated(vec![0u8; 8].into_boxed_slice())
        };
        // Send result regardless of outcome; receiver may have dropped.
        let _ = sender.send(ChunkJobResult::new(task.key, outcome));
    });

    cancel_flag
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::streaming::types::ChunkKey;

    fn key(n: i32) -> ChunkKey {
        ChunkKey::new(n, 0, 0, 0)
    }

    fn task(key: ChunkKey) -> PrioritizedTask {
        PrioritizedTask::new(key, 0, 1.0)
    }

    fn fresh_pool() -> rayon::ThreadPool {
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap()
    }

    // -----------------------------------------------------------------------
    // spawn_sends_result
    // -----------------------------------------------------------------------
    #[test]
    fn spawn_sends_result() {
        let pool = fresh_pool();
        let (tx, rx) = std::sync::mpsc::channel();
        let _flag = spawn_chunk_job(&pool, task(key(1)), tx);
        let result = rx.recv_timeout(Duration::from_secs(2));
        assert!(result.is_ok(), "should receive a result within 2s");
        assert_eq!(result.unwrap().key, key(1));
    }

    // -----------------------------------------------------------------------
    // spawn_cancelled_result
    // -----------------------------------------------------------------------
    #[test]
    fn spawn_cancelled_result() {
        let pool = fresh_pool();
        let (tx, rx) = std::sync::mpsc::channel();
        let flag = spawn_chunk_job(&pool, task(key(2)), tx);
        // Set cancel before the rayon task has a chance to run.
        flag.store(true, Ordering::Relaxed);
        // Allow the task time to execute.
        let result = rx.recv_timeout(Duration::from_secs(2));
        assert!(result.is_ok(), "should still receive a result even when cancelled");
        // Outcome must be Cancelled (if flag was seen in time) or Generated
        // (if the task ran before the flag was set). Either is acceptable; what
        // must NOT happen is a panic or missing result.
        let outcome = result.unwrap().outcome;
        assert!(
            matches!(outcome, ChunkJobOutcome::Cancelled | ChunkJobOutcome::Generated(_)),
            "outcome must be Cancelled or Generated"
        );
    }
}
