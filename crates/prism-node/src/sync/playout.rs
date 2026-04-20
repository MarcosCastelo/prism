//! Playout buffer and chunk deadline calculation.
//!
//! Prism uses a global deadline model (PRD §Sincronização):
//!   deadline_global = chunk.timestamp_ms + configured_latency_ms
//!   wait_ms         = deadline_global - now_unix_ms()
//!
//! NTP (chrony) is a system prerequisite; precision of 10–50 ms is sufficient
//! for a 7–20 s buffer. PTP is available for operators requiring < 1 ms drift.
#![allow(dead_code)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prism_proto::VideoChunk;

/// Acceptable configured latency range (ms). PRD: 7_000–20_000.
const MIN_LATENCY_MS: u64 = 7_000;
const MAX_LATENCY_MS: u64 = 20_000;

/// Buffer that controls when a received chunk is handed to the decoder.
pub struct PlayoutBuffer {
    /// Target end-to-end latency from streamer capture to viewer output (ms).
    pub configured_latency_ms: u64,
}

impl PlayoutBuffer {
    /// Create a buffer with the given latency.
    ///
    /// `latency_ms` is clamped to [7_000, 20_000] ms if outside the valid range.
    pub fn new(latency_ms: u64) -> Self {
        let clamped = latency_ms.clamp(MIN_LATENCY_MS, MAX_LATENCY_MS);
        if clamped != latency_ms {
            tracing::warn!(
                requested = latency_ms,
                clamped,
                "configured_latency_ms out of [7000, 20000] range — clamped"
            );
        }
        Self { configured_latency_ms: clamped }
    }

    /// Global playback deadline for a chunk.
    ///
    /// `deadline_global = chunk.timestamp_ms + configured_latency_ms`
    pub fn deadline_for(&self, chunk: &VideoChunk) -> u64 {
        chunk.timestamp_ms.saturating_add(self.configured_latency_ms)
    }

    /// How long this node should wait before releasing the chunk downstream.
    ///
    /// Returns `Some(duration)` when the chunk is not yet due.
    /// Returns `None` when the deadline has passed — deliver immediately or
    /// apply frame concealment.
    pub fn wait_duration(&self, chunk: &VideoChunk) -> Option<Duration> {
        let deadline = self.deadline_for(chunk);
        let now = now_ms();
        if deadline > now {
            Some(Duration::from_millis(deadline - now))
        } else {
            None
        }
    }

    /// Returns `true` if the chunk's deadline has already passed and it should
    /// be discarded (or concealment applied) rather than buffered.
    pub fn is_late(&self, chunk: &VideoChunk) -> bool {
        self.wait_duration(chunk).is_none()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use prism_proto::VideoChunk;

    fn chunk_at(timestamp_ms: u64) -> VideoChunk {
        VideoChunk {
            stream_id: "test".to_string(),
            sequence: 1,
            timestamp_ms,
            payload: vec![],
            streamer_pubkey: vec![],
            streamer_sig: vec![],
            prev_chunk_hash: vec![],
            layer_hashes: vec![],
        }
    }

    // PRD-specified test
    #[test]
    fn playout_buffer_deadline_calculation() {
        let buffer = PlayoutBuffer { configured_latency_ms: 10_000 };
        let chunk = chunk_at(1_000_000);
        assert_eq!(buffer.deadline_for(&chunk), 1_010_000);
    }

    #[test]
    fn latency_clamp_lower() {
        let buf = PlayoutBuffer::new(1_000); // below MIN
        assert_eq!(buf.configured_latency_ms, MIN_LATENCY_MS);
    }

    #[test]
    fn latency_clamp_upper() {
        let buf = PlayoutBuffer::new(999_999); // above MAX
        assert_eq!(buf.configured_latency_ms, MAX_LATENCY_MS);
    }

    #[test]
    fn latency_in_range_unchanged() {
        let buf = PlayoutBuffer::new(10_000);
        assert_eq!(buf.configured_latency_ms, 10_000);
    }

    #[test]
    fn deadline_saturates_on_overflow() {
        // timestamp near u64::MAX should not panic
        let buf = PlayoutBuffer { configured_latency_ms: 10_000 };
        let chunk = chunk_at(u64::MAX - 1);
        let _ = buf.deadline_for(&chunk); // must not panic
    }

    #[test]
    fn future_chunk_returns_some_wait() {
        let buf = PlayoutBuffer::new(10_000);
        // A chunk timestamped far in the future will have a deadline ahead of now.
        let far_future_ts = now_ms() + 30_000;
        let chunk = chunk_at(far_future_ts);
        assert!(buf.wait_duration(&chunk).is_some());
        assert!(!buf.is_late(&chunk));
    }

    #[test]
    fn past_chunk_returns_none_wait() {
        let buf = PlayoutBuffer::new(7_000);
        // A chunk from 60 s ago: timestamp_ms = now - 60_000
        let old_ts = now_ms().saturating_sub(60_000);
        let chunk = chunk_at(old_ts);
        assert!(buf.wait_duration(&chunk).is_none());
        assert!(buf.is_late(&chunk));
    }
}
