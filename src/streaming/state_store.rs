//! Chunk state machine enforcement.
//!
//! `ChunkStateStore` is the single authority for chunk state transitions.
//! It validates edges, rejects invalid transitions, and increments the
//! revision counter only when a chunk enters `Active` or `Inactive`.

use std::collections::HashMap;

use super::types::{ChunkEntry, ChunkKey, ChunkState};

// ---------------------------------------------------------------------------
// TransitionError
// ---------------------------------------------------------------------------

/// Returned when a requested state transition is not permitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransitionError {
    /// The chunk key is not tracked in the store.
    NotTracked {
        key: ChunkKey,
        attempted_to: ChunkState,
    },
    /// The requested state transition is not a valid edge.
    InvalidTransition {
        key: ChunkKey,
        from: ChunkState,
        to: ChunkState,
    },
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotTracked { key, attempted_to } => {
                write!(
                    f,
                    "chunk {:?} is not tracked in the state store (attempted transition to {:?})",
                    key, attempted_to
                )
            }
            Self::InvalidTransition { key, from, to } => {
                write!(
                    f,
                    "invalid transition for {:?}: {:?} -> {:?}",
                    key, from, to
                )
            }
        }
    }
}

impl std::error::Error for TransitionError {}

// ---------------------------------------------------------------------------
// Allowed edges
// ---------------------------------------------------------------------------

/// Returns `true` when the transition `from -> to` is a valid edge.
fn is_valid_transition(from: ChunkState, to: ChunkState) -> bool {
    use ChunkState::*;
    matches!(
        (from, to),
        (Inactive, Queued)
            | (Queued, Loading)
            | (Queued, Inactive)
            | (Loading, Active)
            | (Loading, Error { .. })
            | (Active, Upgrading)
            | (Active, Downgrading)
            | (Active, Unloading)
            | (Upgrading, Active)
            | (Upgrading, Unloading)
            | (Downgrading, Active)
            | (Downgrading, Unloading)
            | (Unloading, Inactive)
            | (Error { .. }, Queued)
            | (Error { .. }, Inactive)
    )
}

/// Returns `true` when entering `to` should increment the revision counter.
#[inline]
fn increments_revision(to: ChunkState) -> bool {
    matches!(to, ChunkState::Active | ChunkState::Inactive)
}

// ---------------------------------------------------------------------------
// ChunkStateStore
// ---------------------------------------------------------------------------

/// Stores and manages the lifecycle state of every tracked chunk.
#[derive(Debug, Default)]
pub struct ChunkStateStore {
    entries: HashMap<ChunkKey, ChunkEntry>,
}

impl ChunkStateStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a chunk into the store with the `Inactive` state.
    ///
    /// If the chunk is already tracked this is a no-op (returns the
    /// existing entry unchanged).
    pub fn insert_inactive(&mut self, key: ChunkKey) -> &ChunkEntry {
        self.entries
            .entry(key)
            .or_insert_with(|| ChunkEntry::new(key))
    }

    /// Attempt to transition `key` to `to`.
    ///
    /// Returns `Ok(&ChunkEntry)` with the updated entry on success, or
    /// `Err(TransitionError)` if the edge is not allowed or the key is
    /// not yet tracked.
    pub fn transition_to(
        &mut self,
        key: ChunkKey,
        to: ChunkState,
    ) -> Result<&ChunkEntry, TransitionError> {
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or(TransitionError::NotTracked {
                key,
                attempted_to: to,
            })?;

        let from = entry.state;
        if !is_valid_transition(from, to) {
            return Err(TransitionError::InvalidTransition { key, from, to });
        }

        entry.state = to;
        if increments_revision(to) {
            entry.revision += 1;
        }

        Ok(entry)
    }

    /// Return an immutable reference to a tracked entry, if present.
    pub fn get(&self, key: &ChunkKey) -> Option<&ChunkEntry> {
        self.entries.get(key)
    }

    /// Number of tracked chunks.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove a chunk from the store entirely (HIGH-03).
    ///
    /// Called when a chunk transitions to Inactive and should no longer be tracked,
    /// preventing unbounded HashMap growth.
    pub fn remove(&mut self, key: &ChunkKey) -> Option<ChunkEntry> {
        self.entries.remove(key)
    }

    /// Return the set of keys whose current state is `Active`.
    pub fn active_set(&self) -> std::collections::HashSet<ChunkKey> {
        self.entries
            .iter()
            .filter(|(_, e)| e.state == ChunkState::Active)
            .map(|(k, _)| *k)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ChunkState::*;

    fn key(n: i32) -> ChunkKey {
        ChunkKey::new(n, 0, 0, 0)
    }

    // -----------------------------------------------------------------------
    // transition_inactive_to_queued
    // -----------------------------------------------------------------------
    #[test]
    fn transition_inactive_to_queued() {
        let mut store = ChunkStateStore::new();
        let k = key(1);
        store.insert_inactive(k);
        let entry = store.transition_to(k, Queued).expect("should succeed");
        assert_eq!(entry.state, Queued);
        // revision must NOT increment on Queued
        assert_eq!(entry.revision, 0);
    }

    // -----------------------------------------------------------------------
    // transition_invalid_inactive_to_loading
    // -----------------------------------------------------------------------
    #[test]
    fn transition_invalid_inactive_to_loading() {
        let mut store = ChunkStateStore::new();
        let k = key(2);
        store.insert_inactive(k);
        let err = store
            .transition_to(k, Loading)
            .expect_err("must be rejected");
        match err {
            TransitionError::InvalidTransition { key: ek, from, to } => {
                assert_eq!(ek, k);
                assert_eq!(from, Inactive);
                assert_eq!(to, Loading);
            }
            other => panic!("expected InvalidTransition, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // revision_increments_on_active
    // -----------------------------------------------------------------------
    #[test]
    fn revision_increments_on_active() {
        let mut store = ChunkStateStore::new();
        let k = key(3);
        store.insert_inactive(k);
        // revision starts at 0 (Inactive inserted, no increment yet)
        store.transition_to(k, Queued).unwrap();
        store.transition_to(k, Loading).unwrap();
        let entry = store.transition_to(k, Active).expect("Loading->Active ok");
        assert_eq!(entry.state, Active);
        assert_eq!(entry.revision, 1, "revision must increment on Active entry");
    }

    // -----------------------------------------------------------------------
    // revision_increments_on_inactive
    // -----------------------------------------------------------------------
    #[test]
    fn revision_increments_on_inactive() {
        let mut store = ChunkStateStore::new();
        let k = key(4);
        store.insert_inactive(k);
        // Walk full path to Inactive via Unloading
        store.transition_to(k, Queued).unwrap();
        store.transition_to(k, Loading).unwrap();
        store.transition_to(k, Active).unwrap(); // rev -> 1
        store.transition_to(k, Unloading).unwrap();
        let entry = store
            .transition_to(k, Inactive)
            .expect("Unloading->Inactive ok");
        assert_eq!(
            entry.revision, 2,
            "revision must increment again on Inactive entry"
        );
    }
}
