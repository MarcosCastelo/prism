use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use lru::LruCache;
use prism_core::hash::sha256;
use prism_proto::ChatMessage;

use crate::vector_clock::VectorClock;

const PENDING_TTL: Duration = Duration::from_secs(30);
const DELIVERED_CACHE_SIZE: usize = 8192;

pub enum DeliveryEvent {
    /// Message ready for display in causal order.
    Message(Box<ChatMessage>),
    /// Unrecoverable gap (TTL expired) — show a visual gap indicator.
    Gap { count: usize },
}

pub struct MessageOrderer {
    local_clock:    VectorClock,
    /// msg_id → (msg, received_at)
    pending_buffer: BTreeMap<String, (ChatMessage, Instant)>,
    /// msg_ids already delivered (deduplication)
    delivered:      LruCache<String, ()>,
}

impl MessageOrderer {
    pub fn new() -> Self {
        Self {
            local_clock: VectorClock::new(),
            pending_buffer: BTreeMap::new(),
            delivered: LruCache::new(
                NonZeroUsize::new(DELIVERED_CACHE_SIZE)
                    .expect("invariant: DELIVERED_CACHE_SIZE > 0"),
            ),
        }
    }

    /// Processes a message received via gossip.
    /// Delivers immediately if prev_msg_id is known or empty;
    /// otherwise places in pending_buffer.
    pub fn on_receive(&mut self, msg: ChatMessage) -> Vec<DeliveryEvent> {
        if self.delivered.contains(&msg.id) {
            return vec![];
        }

        // Build the message's vector clock from the protobuf map.
        let msg_vc: VectorClock = VectorClock(
            msg.vector_clock
                .iter()
                .map(|(k, &v)| (k.clone(), v))
                .collect(),
        );

        // If prev_msg_id is empty or already delivered, deliver now.
        if msg.prev_msg_id.is_empty() || self.delivered.contains(&msg.prev_msg_id) {
            return self.deliver(msg, msg_vc);
        }

        // Otherwise buffer and try to flush.
        self.pending_buffer.insert(msg.id.clone(), (msg, Instant::now()));
        self.flush_pending()
    }

    /// Checks pending_buffer and delivers messages whose prev is now known.
    /// Messages older than 30s are evicted with a Gap event.
    pub fn flush_pending(&mut self) -> Vec<DeliveryEvent> {
        let mut events = Vec::new();
        let mut delivered_any = true;

        while delivered_any {
            delivered_any = false;
            let keys: Vec<String> = self.pending_buffer.keys().cloned().collect();
            for key in keys {
                if let Some((msg, _)) = self.pending_buffer.get(&key) {
                    if msg.prev_msg_id.is_empty() || self.delivered.contains(&msg.prev_msg_id) {
                        let (msg, _) = self.pending_buffer.remove(&key)
                            .expect("invariant: key present — obtained from same BTreeMap iteration");
                        let msg_vc: VectorClock = VectorClock(
                            msg.vector_clock.iter().map(|(k, &v)| (k.clone(), v)).collect(),
                        );
                        let mut new_events = self.deliver(msg, msg_vc);
                        events.append(&mut new_events);
                        delivered_any = true;
                    }
                }
            }
        }

        // Evict expired entries from pending_buffer.
        let now = Instant::now();
        let expired: Vec<String> = self
            .pending_buffer
            .iter()
            .filter(|(_, (_, received_at))| now.duration_since(*received_at) >= PENDING_TTL)
            .map(|(k, _)| k.clone())
            .collect();

        if !expired.is_empty() {
            let count = expired.len();
            for key in expired {
                self.pending_buffer.remove(&key);
            }
            events.push(DeliveryEvent::Gap { count });
        }

        events
    }

    fn deliver(&mut self, msg: ChatMessage, msg_vc: VectorClock) -> Vec<DeliveryEvent> {
        self.delivered.put(msg.id.clone(), ());
        let sender_hex = hex::encode(&msg.sender_pubkey);
        self.local_clock.increment(&sender_hex);
        self.local_clock.merge(&msg_vc);
        vec![DeliveryEvent::Message(Box::new(msg))]
    }
}

impl Default for MessageOrderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Sort concurrent messages deterministically by SHA-256(message.id) lexicographic order.
pub fn sort_concurrent(msgs: &mut [ChatMessage]) {
    msgs.sort_by(|a, b| {
        let ha = sha256(a.id.as_bytes());
        let hb = sha256(b.id.as_bytes());
        ha.cmp(&hb)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(id: &str, prev: &str, sender: &str) -> ChatMessage {
        ChatMessage {
            id: id.to_string(),
            sender_pubkey: hex::decode(sender).unwrap_or_default(),
            display_name: sender[..4].to_string(),
            stream_id: "test".to_string(),
            text: id.to_string(),
            timestamp_ms: 0,
            prev_msg_id: prev.to_string(),
            signature: vec![0u8; 64],
            vector_clock: Default::default(),
        }
    }

    #[test]
    fn delivers_message_with_no_prev() {
        let mut orderer = MessageOrderer::new();
        let msg = make_msg("msg1", "", "aabbccdd");
        let events = orderer.on_receive(msg);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], DeliveryEvent::Message(_)));
    }

    #[test]
    fn buffers_message_with_unknown_prev_then_delivers() {
        let mut orderer = MessageOrderer::new();
        let msg2 = make_msg("msg2", "msg1", "aabbccdd");
        let events = orderer.on_receive(msg2);
        // msg2 buffered, no delivery yet
        assert!(events.is_empty() || matches!(events[0], DeliveryEvent::Gap { .. }));

        let msg1 = make_msg("msg1", "", "aabbccdd");
        let events = orderer.on_receive(msg1);
        // msg1 delivered immediately
        let delivered: Vec<_> = events
            .iter()
            .filter_map(|e| if let DeliveryEvent::Message(m) = e { Some(m.id.clone()) } else { None })

            .collect();
        assert!(delivered.contains(&"msg1".to_string()));
    }

    #[test]
    fn concurrent_messages_ordered_deterministically() {
        let mut msgs = vec![
            make_msg("bbb", "", "aabbccdd"),
            make_msg("aaa", "", "aabbccdd"),
        ];
        let mut msgs2 = msgs.clone();
        msgs2.reverse();

        sort_concurrent(&mut msgs);
        sort_concurrent(&mut msgs2);

        assert_eq!(msgs[0].id, msgs2[0].id);
        assert_eq!(msgs[1].id, msgs2[1].id);
    }

    #[test]
    fn duplicate_message_ignored() {
        let mut orderer = MessageOrderer::new();
        let msg = make_msg("msg1", "", "aabbccdd");
        orderer.on_receive(msg.clone());
        let events = orderer.on_receive(msg);
        assert!(events.is_empty());
    }
}
