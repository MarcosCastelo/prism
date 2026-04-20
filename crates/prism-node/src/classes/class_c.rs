//! Class C node: Reed-Solomon fragment storage with TTL + late-joiner service.
//!
//! Responsibilities:
//! - Store RS fragments as they arrive (TTL 5 minutes)
//! - Respond to fragment requests for reconstruction
//! - Maintain a sliding window of the last 200 chunks for late-joiners
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use prism_proto::RsFragment;
use tokio::sync::Mutex;

const FRAGMENT_TTL_MS: u64 = 5 * 60 * 1_000; // 5 minutes
const LATE_JOINER_WINDOW: usize = 200;

// ---------------------------------------------------------------------------
// Fragment key
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct FragKey {
    stream_id: String,
    chunk_seq: u64,
    frag_index: u32,
}

// ---------------------------------------------------------------------------
// Stored entry with expiry
// ---------------------------------------------------------------------------

struct Entry {
    fragment: RsFragment,
    expires_at_ms: u64,
}

// ---------------------------------------------------------------------------
// Class C storage
// ---------------------------------------------------------------------------

/// Shared state for a Class C node.
pub struct ClassCStore {
    /// fragment → (data, expiry)
    fragments: DashMap<FragKey, Entry>,
    /// Ordered list of (stream_id, chunk_seq) for late-joiner window.
    /// Evicts oldest entries when > LATE_JOINER_WINDOW.
    late_joiner_index: Mutex<VecDeque<(String, u64)>>,
}

impl ClassCStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            fragments: DashMap::new(),
            late_joiner_index: Mutex::new(VecDeque::new()),
        })
    }

    /// Store a received RS fragment with a 5-minute TTL.
    pub async fn store_fragment(&self, frag: RsFragment) -> anyhow::Result<()> {
        let expires_at_ms = now_ms() + FRAGMENT_TTL_MS;
        let key = FragKey {
            stream_id: frag.stream_id.clone(),
            chunk_seq: frag.chunk_seq,
            frag_index: frag.frag_index,
        };

        // Track chunk in late-joiner window.
        let chunk_key = (frag.stream_id.clone(), frag.chunk_seq);
        {
            let mut idx = self.late_joiner_index.lock().await;
            // Add only if this (stream, seq) isn't already tracked.
            if !idx.contains(&chunk_key) {
                idx.push_back(chunk_key);
                while idx.len() > LATE_JOINER_WINDOW {
                    idx.pop_front();
                }
            }
        }

        self.fragments.insert(key, Entry { fragment: frag, expires_at_ms });

        tracing::trace!(expires_in_s = FRAGMENT_TTL_MS / 1_000, "fragment stored");
        Ok(())
    }

    /// Retrieve a specific fragment by (stream_id, chunk_seq, frag_index).
    ///
    /// Returns `Err` if the fragment is unknown or has expired.
    pub async fn serve_fragment(
        &self,
        stream_id: &str,
        chunk_seq: u64,
        frag_index: u32,
    ) -> anyhow::Result<RsFragment> {
        let key = FragKey {
            stream_id: stream_id.to_string(),
            chunk_seq,
            frag_index,
        };

        match self.fragments.get(&key) {
            Some(entry) if entry.expires_at_ms > now_ms() => {
                Ok(entry.fragment.clone())
            }
            Some(_) => {
                // Expired — remove it.
                self.fragments.remove(&key);
                anyhow::bail!(
                    "fragment {stream_id}:{chunk_seq}:{frag_index} has expired"
                )
            }
            None => anyhow::bail!(
                "fragment {stream_id}:{chunk_seq}:{frag_index} not found"
            ),
        }
    }

    /// Number of live (non-expired) fragments currently held.
    pub fn live_fragment_count(&self) -> usize {
        let now = now_ms();
        self.fragments.iter().filter(|e| e.expires_at_ms > now).count()
    }

    /// Evict all expired fragments. Should be called periodically.
    pub fn evict_expired(&self) {
        let now = now_ms();
        self.fragments.retain(|_, e| e.expires_at_ms > now);
    }

    /// Returns the (stream_id, chunk_seq) pairs in the late-joiner window.
    pub async fn late_joiner_chunks(&self) -> Vec<(String, u64)> {
        self.late_joiner_index.lock().await.iter().cloned().collect()
    }
}

impl Default for ClassCStore {
    fn default() -> Self {
        Self {
            fragments: DashMap::new(),
            late_joiner_index: Mutex::new(VecDeque::new()),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Module-level free functions matching PRD interface
// ---------------------------------------------------------------------------

/// Convenience wrapper: store a fragment in the provided store.
pub async fn store_fragment(
    store: &ClassCStore,
    frag: RsFragment,
) -> anyhow::Result<()> {
    store.store_fragment(frag).await
}

/// Convenience wrapper: retrieve a fragment from the provided store.
pub async fn serve_fragment(
    store: &ClassCStore,
    stream_id: &str,
    chunk_seq: u64,
    frag_index: u32,
) -> anyhow::Result<RsFragment> {
    store.serve_fragment(stream_id, chunk_seq, frag_index).await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn frag(stream_id: &str, seq: u64, idx: u32) -> RsFragment {
        RsFragment {
            stream_id: stream_id.to_string(),
            chunk_seq: seq,
            frag_index: idx,
            total_frags: 14,
            data_frags: 10,
            fragment: vec![idx as u8; 64],
            chunk_hash: vec![0u8; 32],
        }
    }

    #[tokio::test]
    async fn store_and_retrieve_fragment() {
        let store = ClassCStore::new();
        let f = frag("stream1", 10, 3);
        store_fragment(&store, f.clone()).await.unwrap();

        let got = serve_fragment(&store, "stream1", 10, 3).await.unwrap();
        assert_eq!(got.frag_index, 3);
        assert_eq!(got.fragment, vec![3u8; 64]);
    }

    #[tokio::test]
    async fn serve_missing_fragment_returns_err() {
        let store = ClassCStore::new();
        let result = serve_fragment(&store, "ghost", 99, 0).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn late_joiner_window_evicts_oldest() {
        let store = ClassCStore::new();
        // Insert LATE_JOINER_WINDOW + 5 distinct chunks.
        for seq in 0u64..(LATE_JOINER_WINDOW as u64 + 5) {
            store_fragment(&store, frag("s1", seq, 0)).await.unwrap();
        }
        let chunks = store.late_joiner_chunks().await;
        assert_eq!(
            chunks.len(),
            LATE_JOINER_WINDOW,
            "window must not exceed {LATE_JOINER_WINDOW}"
        );
        // Oldest entries (seq 0..4) must be evicted.
        assert!(!chunks.iter().any(|(_, seq)| *seq < 5));
    }

    #[tokio::test]
    async fn live_fragment_count_tracks_inserts() {
        let store = ClassCStore::new();
        assert_eq!(store.live_fragment_count(), 0);
        store_fragment(&store, frag("s", 1, 0)).await.unwrap();
        store_fragment(&store, frag("s", 1, 1)).await.unwrap();
        assert_eq!(store.live_fragment_count(), 2);
    }

    #[tokio::test]
    async fn duplicate_chunk_not_double_counted_in_window() {
        let store = ClassCStore::new();
        // Store two fragments for the same chunk.
        store_fragment(&store, frag("s", 5, 0)).await.unwrap();
        store_fragment(&store, frag("s", 5, 1)).await.unwrap();
        let chunks = store.late_joiner_chunks().await;
        assert_eq!(chunks.len(), 1, "same chunk must appear once in the window");
    }
}
