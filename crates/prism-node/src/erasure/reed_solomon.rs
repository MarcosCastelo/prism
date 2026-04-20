//! Reed-Solomon erasure coding wrapper.
#![allow(dead_code)]
//!
//! Exposes two configurations:
//!
//! | Config   | Data | Parity | Total | Min needed | Max loss tolerated |
//! |----------|------|--------|-------|------------|--------------------|
//! | Standard | 10   | 4      | 14    | 10 of 14   | 4                  |
//! | Reduced  | 4    | 2      | 6     | 4 of 6     | 2                  |
//!
//! Standard is used when ≥10 Class C nodes are available.
//! Reduced is used when fewer Class C nodes are reachable.
//!
//! Encoding embeds the original payload length as an 8-byte big-endian prefix
//! so reconstruction always returns exactly the original bytes.

use reed_solomon_erasure::galois_8::ReedSolomon;

/// Which RS scheme to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsConfig {
    /// RS(10, 4): 10 data + 4 parity = 14 total.
    /// Reconstruction requires **any 10 of the 14** fragments.
    /// Tolerates loss of **up to 4** fragments.
    Standard,

    /// RS(4, 2): 4 data + 2 parity = 6 total.
    /// Reconstruction requires **any 4 of the 6** fragments.
    /// Tolerates loss of **up to 2** fragments.
    /// Use when fewer than 10 Class C nodes are available.
    Reduced,
}

impl RsConfig {
    fn data_frags(self) -> usize {
        match self {
            Self::Standard => 10,
            Self::Reduced => 4,
        }
    }
    fn parity_frags(self) -> usize {
        match self {
            Self::Standard => 4,
            Self::Reduced => 2,
        }
    }
}

/// Reed-Solomon encoder / decoder.
pub struct ReedSolomonCoder {
    config: RsConfig,
    rs: ReedSolomon,
}

impl ReedSolomonCoder {
    pub fn new(config: RsConfig) -> Self {
        let rs = ReedSolomon::new(config.data_frags(), config.parity_frags())
            .expect("static RS config is always valid");
        Self { config, rs }
    }

    /// Number of data fragments (10 or 4).
    pub fn data_frags(&self) -> usize {
        self.config.data_frags()
    }

    /// Number of parity fragments (4 or 2).
    pub fn parity_frags(&self) -> usize {
        self.config.parity_frags()
    }

    /// Total fragments = data + parity (14 or 6).
    pub fn total_frags(&self) -> usize {
        self.data_frags() + self.parity_frags()
    }

    /// Split `payload` into `total_frags()` equal-sized fragments.
    ///
    /// The first `data_frags()` elements are data; the last `parity_frags()`
    /// are Reed-Solomon parity. All fragments have the same byte length.
    ///
    /// An 8-byte big-endian length prefix is prepended internally so that
    /// `reconstruct` can recover exactly the original bytes.
    pub fn encode(&self, payload: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
        let data = self.data_frags();

        // Prepend 8-byte length so reconstruction returns exact original payload.
        let original_len = payload.len() as u64;
        let mut prefixed = Vec::with_capacity(8 + payload.len());
        prefixed.extend_from_slice(&original_len.to_be_bytes());
        prefixed.extend_from_slice(payload);

        // Pad to an exact multiple of data_frags.
        let shard_size = prefixed.len().div_ceil(data);
        prefixed.resize(shard_size * data, 0u8);

        // Build shards: data shards from split, parity shards as zeroed buffers.
        let mut shards: Vec<Vec<u8>> = prefixed.chunks_exact(shard_size).map(|c| c.to_vec()).collect();
        shards.extend(vec![vec![0u8; shard_size]; self.parity_frags()]);

        debug_assert_eq!(shards.len(), self.total_frags());

        self.rs.encode(&mut shards).map_err(|e| anyhow::anyhow!("RS encode failed: {e:?}"))?;

        tracing::trace!(
            config = ?self.config,
            payload_bytes = payload.len(),
            shard_size,
            total_frags = shards.len(),
            "RS encode complete"
        );

        Ok(shards)
    }

    /// Reconstruct the original payload from `fragments`.
    ///
    /// `fragments` must have exactly `total_frags()` entries. Use `None` for
    /// each fragment that is unavailable. At least `data_frags()` entries must
    /// be `Some`; otherwise returns `Err`.
    pub fn reconstruct(&self, fragments: Vec<Option<Vec<u8>>>) -> anyhow::Result<Vec<u8>> {
        let total = self.total_frags();
        if fragments.len() != total {
            anyhow::bail!(
                "expected {} fragments, got {}",
                total,
                fragments.len()
            );
        }

        let present = fragments.iter().filter(|f| f.is_some()).count();
        if present < self.data_frags() {
            anyhow::bail!(
                "insufficient fragments for reconstruction: need {}, have {}",
                self.data_frags(),
                present
            );
        }

        let mut shards = fragments;
        self.rs
            .reconstruct(&mut shards)
            .map_err(|e| anyhow::anyhow!("RS reconstruct failed: {e:?}"))?;

        // Reassemble from the data shards only.
        let mut assembled: Vec<u8> = Vec::new();
        for shard in shards.into_iter().take(self.data_frags()) {
            assembled.extend_from_slice(&shard.expect("reconstruct filled all shards"));
        }

        // Recover exact original payload via the embedded length prefix.
        if assembled.len() < 8 {
            anyhow::bail!("reconstructed data is too short to contain length prefix");
        }
        let original_len =
            u64::from_be_bytes(assembled[..8].try_into().expect("8 bytes")) as usize;
        if 8 + original_len > assembled.len() {
            anyhow::bail!(
                "embedded length {} exceeds assembled size {}",
                original_len,
                assembled.len()
            );
        }

        tracing::trace!(
            config = ?self.config,
            present,
            original_len,
            "RS reconstruct complete"
        );

        Ok(assembled[8..8 + original_len].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PRD-specified tests ────────────────────────────────────────────────

    #[test]
    fn rs_standard_reconstruct_from_any_10_of_14() {
        let coder = ReedSolomonCoder::new(RsConfig::Standard);
        let payload = vec![0xABu8; 9_000]; // 9 KB representative chunk
        let frags = coder.encode(&payload).unwrap();
        assert_eq!(frags.len(), 14);

        // Test 4 distinct sets of 4 missing fragments (sliding window).
        for missing in 0..4 {
            let mut partial: Vec<Option<Vec<u8>>> =
                frags.iter().map(|f| Some(f.clone())).collect();
            partial[missing]     = None;
            partial[missing + 1] = None;
            partial[missing + 2] = None;
            partial[missing + 3] = None;
            let recovered = coder.reconstruct(partial).unwrap();
            assert_eq!(
                recovered, payload,
                "failed to recover when missing shards {missing}..{}",
                missing + 3
            );
        }
    }

    #[test]
    fn rs_standard_fails_with_only_9_fragments() {
        let coder = ReedSolomonCoder::new(RsConfig::Standard);
        let payload = vec![0xCDu8; 9_000];
        let frags = coder.encode(&payload).unwrap();

        let mut partial: Vec<Option<Vec<u8>>> =
            frags.iter().map(|f| Some(f.clone())).collect();
        // Remove 5 fragments → only 9 remain, need 10.
        partial[0] = None;
        partial[1] = None;
        partial[2] = None;
        partial[3] = None;
        partial[4] = None;
        assert!(
            coder.reconstruct(partial).is_err(),
            "must fail with only 9 fragments"
        );
    }

    // ── Additional tests ───────────────────────────────────────────────────

    #[test]
    fn rs_standard_accessors() {
        let c = ReedSolomonCoder::new(RsConfig::Standard);
        assert_eq!(c.data_frags(), 10);
        assert_eq!(c.parity_frags(), 4);
        assert_eq!(c.total_frags(), 14);
    }

    #[test]
    fn rs_reduced_accessors() {
        let c = ReedSolomonCoder::new(RsConfig::Reduced);
        assert_eq!(c.data_frags(), 4);
        assert_eq!(c.parity_frags(), 2);
        assert_eq!(c.total_frags(), 6);
    }

    #[test]
    fn rs_reduced_reconstruct_any_4_of_6() {
        let coder = ReedSolomonCoder::new(RsConfig::Reduced);
        let payload = vec![0x55u8; 4_000];
        let frags = coder.encode(&payload).unwrap();
        assert_eq!(frags.len(), 6);

        // Remove 2 shards (max tolerated for Reduced)
        let mut partial: Vec<Option<Vec<u8>>> =
            frags.iter().map(|f| Some(f.clone())).collect();
        partial[0] = None;
        partial[5] = None;
        let recovered = coder.reconstruct(partial).unwrap();
        assert_eq!(recovered, payload);
    }

    #[test]
    fn rs_reduced_fails_with_only_3_fragments() {
        let coder = ReedSolomonCoder::new(RsConfig::Reduced);
        let payload = vec![0x77u8; 2_000];
        let frags = coder.encode(&payload).unwrap();

        let mut partial: Vec<Option<Vec<u8>>> =
            frags.iter().map(|f| Some(f.clone())).collect();
        partial[0] = None;
        partial[1] = None;
        partial[2] = None; // 3 missing → only 3 present, need 4
        assert!(coder.reconstruct(partial).is_err());
    }

    #[test]
    fn encode_small_payload_roundtrip() {
        let coder = ReedSolomonCoder::new(RsConfig::Standard);
        let payload = b"hello prism".to_vec();
        let frags = coder.encode(&payload).unwrap();
        let all: Vec<Option<Vec<u8>>> = frags.into_iter().map(Some).collect();
        assert_eq!(coder.reconstruct(all).unwrap(), payload);
    }

    #[test]
    fn encode_empty_payload_roundtrip() {
        let coder = ReedSolomonCoder::new(RsConfig::Standard);
        let payload: Vec<u8> = vec![];
        let frags = coder.encode(&payload).unwrap();
        let all: Vec<Option<Vec<u8>>> = frags.into_iter().map(Some).collect();
        assert_eq!(coder.reconstruct(all).unwrap(), payload);
    }

    #[test]
    fn reconstruct_wrong_fragment_count_returns_err() {
        let coder = ReedSolomonCoder::new(RsConfig::Standard);
        let result = coder.reconstruct(vec![None; 5]); // 5 ≠ 14
        assert!(result.is_err());
    }

    #[test]
    fn all_fragments_present_standard_roundtrip() {
        let coder = ReedSolomonCoder::new(RsConfig::Standard);
        // 0-length padding edge-case: payload exactly divisible by data_frags
        let payload = vec![0xEEu8; 10_000]; // 10_000 / 10 = 1000 bytes/shard + 8-byte prefix rounds up
        let frags = coder.encode(&payload).unwrap();
        assert_eq!(frags.len(), 14);
        let all: Vec<Option<Vec<u8>>> = frags.into_iter().map(Some).collect();
        assert_eq!(coder.reconstruct(all).unwrap(), payload);
    }
}
