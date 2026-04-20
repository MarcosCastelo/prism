//! Concrete `SeedRouter` implementation for the Studio streamer.
//!
//! `EmbeddedSeedRouter` delivers `VideoChunk` bytes to seed nodes over a
//! simple length-prefixed TCP protocol (4-byte big-endian length + payload).
//!
//! Seed addresses are provided at construction time as `"host:port"` strings.
//! Future enhancement: discover class-A seeds dynamically via the Kademlia DHT
//! by querying the record key `"{stream_id}:seeds"`.
//!
//! ## Wire protocol
//!
//! ```text
//! ┌─────────────────┬──────────────────────────┐
//! │  length: u32 BE │  VideoChunk protobuf bytes│
//! └─────────────────┴──────────────────────────┘
//! ```
//!
//! Seed nodes must implement the corresponding receiver (see `prism-node`
//! chunk ingestion handler) to decode and verify the incoming chunk.

use std::{future::Future, pin::Pin, sync::Arc};

use anyhow::Context;
use tokio::io::AsyncWriteExt;

use prism_ingest::injector::SeedRouter;

// ─────────────────────────────────────────────────────────────────────────────

/// TCP-based seed router for the Studio streamer.
///
/// Configure seed addresses via the `PRISM_SEED_ADDRS` environment variable
/// (comma-separated `host:port` list) or supply them directly with [`new`].
///
/// # Example
///
/// ```bash
/// PRISM_SEED_ADDRS=192.168.1.10:4002,192.168.1.11:4002 ./prism-studio
/// ```
pub struct EmbeddedSeedRouter {
    seeds: Vec<String>,
}

impl EmbeddedSeedRouter {
    /// Create a router with an explicit list of seed addresses (`"host:port"`).
    pub fn new(seeds: Vec<String>) -> Self {
        Self { seeds }
    }

    /// Read seed addresses from the `PRISM_SEED_ADDRS` environment variable.
    ///
    /// Falls back to an empty list if the variable is unset or empty.
    pub fn from_env() -> Self {
        let seeds = std::env::var("PRISM_SEED_ADDRS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        Self::new(seeds)
    }
}

impl SeedRouter for EmbeddedSeedRouter {
    /// Return up to `n` seed addresses from the configured list.
    ///
    /// Future: also query Kademlia DHT for class-A nodes registered under
    /// `"{stream_id}:seeds"` and merge with the static list.
    fn find_seeds(
        &self,
        _stream_id: String,
        n: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<String>> + Send + 'static>> {
        let seeds: Vec<String> = self.seeds.iter().take(n).cloned().collect();
        Box::pin(async move { seeds })
    }

    /// Open a TCP connection to `addr` and write the chunk using 4-byte
    /// length-prefix framing.
    fn send_chunk(
        &self,
        addr: String,
        chunk_bytes: Arc<[u8]>,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'static>> {
        Box::pin(async move {
            let mut stream = tokio::net::TcpStream::connect(&addr)
                .await
                .with_context(|| format!("TCP connect to seed {addr}"))?;

            let len = chunk_bytes.len() as u32;
            stream
                .write_all(&len.to_be_bytes())
                .await
                .context("write length prefix")?;
            stream
                .write_all(&chunk_bytes)
                .await
                .context("write chunk payload")?;
            stream.flush().await.context("flush TCP stream")?;

            tracing::debug!(addr = %addr, bytes = chunk_bytes.len(), "chunk sent to seed");
            Ok(())
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[test]
    fn from_env_empty_when_var_unset() {
        std::env::remove_var("PRISM_SEED_ADDRS");
        let r = EmbeddedSeedRouter::from_env();
        assert!(r.seeds.is_empty());
    }

    #[test]
    fn from_env_parses_comma_list() {
        std::env::set_var("PRISM_SEED_ADDRS", "1.2.3.4:4002,5.6.7.8:4002");
        let r = EmbeddedSeedRouter::from_env();
        assert_eq!(r.seeds, vec!["1.2.3.4:4002", "5.6.7.8:4002"]);
        std::env::remove_var("PRISM_SEED_ADDRS");
    }

    #[tokio::test]
    async fn find_seeds_returns_up_to_n() {
        let r = EmbeddedSeedRouter::new(vec!["a:1".into(), "b:2".into(), "c:3".into()]);
        let seeds = r.find_seeds("stream".into(), 2).await;
        assert_eq!(seeds, vec!["a:1", "b:2"]);
    }

    #[tokio::test]
    async fn send_chunk_delivers_length_prefixed_bytes() {
        // Bind a local TCP listener to act as the seed node.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let payload = b"hello chunk";
        let chunk_bytes: Arc<[u8]> = Arc::from(payload.as_ref());

        // Spawn receiver.
        let received = tokio::spawn(async move {
            let (mut conn, _) = listener.accept().await.unwrap();
            let mut len_buf = [0u8; 4];
            conn.read_exact(&mut len_buf).await.unwrap();
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut body = vec![0u8; len];
            conn.read_exact(&mut body).await.unwrap();
            body
        });

        let router = EmbeddedSeedRouter::new(vec![addr.to_string()]);
        router.send_chunk(addr.to_string(), chunk_bytes).await.unwrap();

        let body = received.await.unwrap();
        assert_eq!(body, payload);
    }
}
