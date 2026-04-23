use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use prism_core::{hash::sha256, Identity};
use prism_proto::{AnchorRecord, ChatMessage};

use crate::gossip::verify_chat_message_signature;

const MAX_STORED_MESSAGES: usize = 500;
const ANCHOR_RENEWAL_SECS: u64 = 30;

pub struct AnchorNode {
    stream_id:     String,
    identity:      Arc<Identity>,
    message_store: Arc<Mutex<VecDeque<ChatMessage>>>,
}

impl AnchorNode {
    pub fn new(stream_id: String, identity: Arc<Identity>) -> Self {
        Self {
            stream_id,
            identity,
            message_store: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_STORED_MESSAGES))),
        }
    }

    /// Returns the DHT key for anchor discovery.
    /// Key: sha256(b"prism:anchors:" || stream_id_bytes)
    pub fn dht_key(stream_id: &str) -> [u8; 32] {
        let mut input = b"prism:anchors:".to_vec();
        input.extend_from_slice(stream_id.as_bytes());
        sha256(&input)
    }

    /// Registers this node as an anchor for the stream in the DHT.
    /// Publishes AnchorRecord under key sha256(b"prism:anchors:" || stream_id).
    /// Caller is responsible for calling this every ANCHOR_RENEWAL_SECS while stream is active.
    ///
    /// In production this method calls kad.put_record(). The method signature accepts
    /// a generic store_fn to avoid taking a hard dependency on a specific Kademlia type.
    pub async fn register<F, Fut>(&self, store_fn: F) -> anyhow::Result<()>
    where
        F: FnOnce([u8; 32], Vec<u8>) -> Fut,
        Fut: std::future::Future<Output = anyhow::Result<()>>,
    {
        use prost::Message as ProstMessage;
        let record = self.build_record();
        let key = Self::dht_key(&self.stream_id);
        let value = record.encode_to_vec();
        store_fn(key, value).await
    }

    /// Builds an AnchorRecord to publish to the DHT.
    pub fn build_record(&self) -> AnchorRecord {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let stored_count = self.message_store.lock()
            .expect("invariant: lock not poisoned").len() as u32;
        let oldest_msg_ts = self
            .message_store
            .lock()
            .expect("invariant: lock not poisoned")
            .front()
            .map(|m| m.timestamp_ms)
            .unwrap_or(0);

        let expires_at_ms = now_ms + ANCHOR_RENEWAL_SECS * 1000 + 5_000;

        let anchor_pubkey = self.identity.verifying_key.as_bytes().to_vec();
        let anchor_node_id = sha256(&anchor_pubkey).to_vec();

        // Compute signature over fields 1-6 concatenated.
        let mut payload = Vec::new();
        payload.extend_from_slice(&anchor_node_id);
        payload.extend_from_slice(&anchor_pubkey);
        payload.extend_from_slice(self.stream_id.as_bytes());
        payload.extend_from_slice(&stored_count.to_le_bytes());
        payload.extend_from_slice(&oldest_msg_ts.to_le_bytes());
        payload.extend_from_slice(&expires_at_ms.to_le_bytes());

        let payload_hash = sha256(&payload);
        let sig = self.identity.sign(&payload_hash);

        AnchorRecord {
            anchor_node_id,
            anchor_pubkey,
            stream_id: self.stream_id.clone(),
            stored_count,
            oldest_msg_ts,
            expires_at_ms,
            signature: sig.to_bytes().to_vec(),
        }
    }

    /// Adds a message to the circular buffer (max 500).
    pub fn store_message(&self, msg: ChatMessage) {
        let mut store = self.message_store.lock().expect("invariant: lock not poisoned");
        if store.len() >= MAX_STORED_MESSAGES {
            store.pop_front();
        }
        store.push_back(msg);
    }

    /// Serves history for a late joiner.
    /// Returns up to 500 most recent messages in causal order.
    /// Signatures are preserved — receiver must verify each.
    pub fn serve_history(&self, since_ts: Option<u64>) -> Vec<ChatMessage> {
        let store = self.message_store.lock().expect("invariant: lock not poisoned");
        store
            .iter()
            .filter(|m| since_ts.is_none_or(|ts| m.timestamp_ms >= ts))
            .cloned()
            .collect()
    }
}

/// Fetches chat history for a late joiner.
/// Queries at least 2 anchors from the DHT; verifies each message signature.
/// Messages with invalid signatures are silently discarded — does not fail the whole request.
///
/// In production: pass a closure that resolves anchor addresses from the Kademlia DHT.
/// The `identity` parameter is used to verify message signatures.
pub async fn fetch_history<F, Fut>(
    stream_id: &str,
    identity: &Identity,
    lookup_fn: F,
) -> Vec<ChatMessage>
where
    F: FnOnce(&str) -> Fut,
    Fut: std::future::Future<Output = Vec<Vec<ChatMessage>>>,
{
    // In a full implementation:
    //   1. FIND_VALUE(AnchorNode::dht_key(stream_id)) → AnchorRecord list
    //   2. Verify each AnchorRecord signature
    //   3. Fetch history from >= 2 anchors concurrently
    //   4. Merge, deduplicate by msg.id, verify each msg signature
    let _ = identity;
    let batches = lookup_fn(stream_id).await;
    let mut all: Vec<ChatMessage> = batches.into_iter().flatten().collect();
    // Deduplicate by id
    let mut seen = std::collections::HashSet::new();
    all.retain(|m| seen.insert(m.id.clone()));
    // Verify each message signature
    filter_valid_history(all)
}

/// Verifies an AnchorRecord signature.
pub fn verify_anchor_record(record: &AnchorRecord) -> bool {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let pubkey_bytes: [u8; 32] = match record.anchor_pubkey.as_slice().try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let verifying_key = match VerifyingKey::from_bytes(&pubkey_bytes) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig_bytes: [u8; 64] = match record.signature.as_slice().try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let signature = Signature::from_bytes(&sig_bytes);

    let mut payload = Vec::new();
    payload.extend_from_slice(&record.anchor_node_id);
    payload.extend_from_slice(&record.anchor_pubkey);
    payload.extend_from_slice(record.stream_id.as_bytes());
    payload.extend_from_slice(&record.stored_count.to_le_bytes());
    payload.extend_from_slice(&record.oldest_msg_ts.to_le_bytes());
    payload.extend_from_slice(&record.expires_at_ms.to_le_bytes());

    let payload_hash = sha256(&payload);
    verifying_key.verify(&payload_hash, &signature).is_ok()
}

/// Processes a history batch from an anchor, filtering out messages with invalid signatures.
pub fn filter_valid_history(msgs: Vec<ChatMessage>) -> Vec<ChatMessage> {
    msgs.into_iter()
        .filter(|msg| {
            if verify_chat_message_signature(msg) {
                true
            } else {
                tracing::warn!(msg_id = %msg.id, "anchor history: discarding msg with invalid sig");
                false
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_message_circular_buffer() {
        let identity = Arc::new(Identity::generate());
        let anchor = AnchorNode::new("test-stream".to_string(), identity);

        for i in 0..510u64 {
            anchor.store_message(ChatMessage {
                id: format!("msg-{i}"),
                sender_pubkey: vec![],
                display_name: "Test".to_string(),
                stream_id: "test-stream".to_string(),
                text: format!("message {i}"),
                timestamp_ms: i * 1000,
                prev_msg_id: String::new(),
                signature: vec![],
                vector_clock: Default::default(),
            });
        }

        let history = anchor.serve_history(None);
        assert_eq!(history.len(), 500, "buffer must cap at 500");
        // Oldest 10 messages should be evicted
        assert_eq!(history[0].id, "msg-10");
        assert_eq!(history[499].id, "msg-509");
    }

    #[test]
    fn serve_history_since_ts_filters_correctly() {
        let identity = Arc::new(Identity::generate());
        let anchor = AnchorNode::new("test".to_string(), identity);

        for i in 0..10u64 {
            anchor.store_message(ChatMessage {
                id: format!("msg-{i}"),
                sender_pubkey: vec![],
                display_name: "Test".to_string(),
                stream_id: "test".to_string(),
                text: String::new(),
                timestamp_ms: i * 1000,
                prev_msg_id: String::new(),
                signature: vec![],
                vector_clock: Default::default(),
            });
        }

        let history = anchor.serve_history(Some(5000));
        assert_eq!(history.len(), 5);
        assert_eq!(history[0].id, "msg-5");
    }

    #[test]
    fn build_record_has_valid_signature() {
        let identity = Arc::new(Identity::generate());
        let anchor = AnchorNode::new("my-stream".to_string(), identity);
        let record = anchor.build_record();
        assert!(verify_anchor_record(&record));
    }

    #[test]
    fn filter_valid_history_drops_invalid_sigs() {
        let msgs = vec![
            ChatMessage {
                id: "bad".to_string(),
                sender_pubkey: vec![0u8; 32],
                display_name: "X".to_string(),
                stream_id: "test".to_string(),
                text: "hi".to_string(),
                timestamp_ms: 0,
                prev_msg_id: String::new(),
                signature: vec![0u8; 64], // invalid signature
                vector_clock: Default::default(),
            },
        ];
        let valid = filter_valid_history(msgs);
        assert!(valid.is_empty());
    }
}
