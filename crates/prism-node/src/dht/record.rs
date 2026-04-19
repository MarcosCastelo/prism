use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use libp2p::kad::Behaviour;
use prism_core::{hash::sha256, Identity};
use prism_proto::NodeRecord;
use prost::Message;
use tokio::sync::Mutex;

use crate::health::benchmark::CapacityReport;
use crate::transport::quic::PrismBehaviour;

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const TTL_MS: u64 = 10_000;
const RENEWAL_INTERVAL_MS: u64 = 10_000;
const WEIGHT_DELTA_THRESHOLD: f32 = 0.1;
const DHT_KEY_PREFIX: &[u8] = b"prism:node:";

pub struct DhtRecordManager {
    identity: Arc<Identity>,
    capacity: CapacityReport,
    last_weight: std::sync::atomic::AtomicU32,
}

impl DhtRecordManager {
    pub fn new(identity: Arc<Identity>, capacity: CapacityReport) -> Self {
        let initial_weight = capacity.weight.to_bits();
        Self {
            identity,
            capacity,
            last_weight: std::sync::atomic::AtomicU32::new(initial_weight),
        }
    }

    pub async fn publish_once(
        &self,
        kad: &mut Behaviour<kad::store::MemoryStore>,
    ) -> anyhow::Result<()> {
        let record = self.build_signed_record()?;
        let mut buf = Vec::new();
        record.encode(&mut buf).context("protobuf encode failed")?;

        let dht_key = Self::dht_key_for(&self.identity.node_id);
        kad.put_record(
            libp2p::kad::Record::new(dht_key, buf),
            libp2p::kad::Quorum::One,
        )
        .context("kad put_record failed")?;

        tracing::info!(
            node_id = %hex::encode(&self.identity.node_id[..4]),
            capacity_class = %self.capacity.capacity_class,
            "NodeRecord published to DHT"
        );
        Ok(())
    }

    pub fn start_renewal_loop(
        self: Arc<Self>,
        swarm: Arc<Mutex<libp2p::Swarm<PrismBehaviour>>>,
    ) {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(RENEWAL_INTERVAL_MS)).await;

                let current_weight = self.capacity.weight;
                let last_bits = self
                    .last_weight
                    .load(std::sync::atomic::Ordering::Relaxed);
                let last_weight = f32::from_bits(last_bits);

                if (current_weight - last_weight).abs() < WEIGHT_DELTA_THRESHOLD {
                    continue;
                }

                self.last_weight.store(
                    current_weight.to_bits(),
                    std::sync::atomic::Ordering::Relaxed,
                );

                let mut sw = swarm.lock().await;
                if let Err(e) =
                    self.publish_once(&mut sw.behaviour_mut().kademlia).await
                {
                    tracing::error!(err = %e, "NodeRecord renewal failed");
                }
            }
        });
    }

    pub fn verify_incoming_record(record: &NodeRecord) -> anyhow::Result<()> {
        let now = now_unix_ms();

        // 1. node_id == SHA-256(pubkey)
        let expected_node_id = sha256(&record.pubkey);
        if record.node_id != expected_node_id {
            anyhow::bail!("node_id/pubkey mismatch — identity forging attempt");
        }

        // 2. record not expired
        if record.expires_at_ms <= now {
            anyhow::bail!("record expired");
        }

        // 3. TTL not manipulated
        if record.expires_at_ms > now + 15_000 {
            anyhow::bail!("TTL too long — capping not allowed");
        }

        // 4. Ed25519 signature valid
        let pubkey_bytes: [u8; 32] = record
            .pubkey
            .as_slice()
            .try_into()
            .context("pubkey wrong length")?;
        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(&pubkey_bytes)
                .context("invalid pubkey bytes")?;
        let sig_bytes: [u8; 64] = record
            .signature
            .as_slice()
            .try_into()
            .context("signature wrong length")?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        let signed_payload = record_signed_payload(record);
        if !Identity::verify(&signed_payload, &signature, &verifying_key) {
            anyhow::bail!("invalid NodeRecord signature");
        }

        // 5. capacity_class valid
        match record.capacity_class.as_str() {
            "A" | "B" | "C" | "edge" => {}
            _ => anyhow::bail!("unknown capacity class: {}", record.capacity_class),
        }

        Ok(())
    }

    fn build_signed_record(&self) -> anyhow::Result<NodeRecord> {
        let expires_at_ms = now_unix_ms() + TTL_MS;
        let mut record = NodeRecord {
            node_id: self.identity.node_id.to_vec(),
            pubkey: self.identity.verifying_key.as_bytes().to_vec(),
            region: "unknown".into(),
            capacity_class: self.capacity.capacity_class.clone(),
            available_slots: self.capacity.vnodes_count,
            health_score: self.capacity.weight,
            expires_at_ms,
            signature: vec![],
        };

        let payload = record_signed_payload(&record);
        let sig = self.identity.sign(&payload);
        record.signature = sig.to_bytes().to_vec();
        Ok(record)
    }

    pub fn dht_key_for(node_id: &[u8; 32]) -> libp2p::kad::RecordKey {
        let mut key = DHT_KEY_PREFIX.to_vec();
        key.extend_from_slice(node_id);
        libp2p::kad::RecordKey::new(&sha256(&key))
    }
}

/// Canonical payload for NodeRecord signature: sha256(fields 1–7 concatenated).
pub fn record_signed_payload(record: &NodeRecord) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(&record.node_id);
    buf.extend_from_slice(&record.pubkey);
    buf.extend_from_slice(record.region.as_bytes());
    buf.extend_from_slice(record.capacity_class.as_bytes());
    buf.extend_from_slice(&record.available_slots.to_le_bytes());
    buf.extend_from_slice(&record.health_score.to_le_bytes());
    buf.extend_from_slice(&record.expires_at_ms.to_le_bytes());
    sha256(&buf).to_vec()
}

use libp2p::kad;

#[cfg(test)]
mod tests {
    use super::*;
    use prism_core::Identity;

    fn make_valid_node_record() -> NodeRecord {
        let id = Identity::generate();
        let expires_at_ms = now_unix_ms() + 10_000;
        let mut record = NodeRecord {
            node_id: id.node_id.to_vec(),
            pubkey: id.verifying_key.as_bytes().to_vec(),
            region: "test".into(),
            capacity_class: "edge".into(),
            available_slots: 10,
            health_score: 0.5,
            expires_at_ms,
            signature: vec![],
        };
        let payload = record_signed_payload(&record);
        let sig = id.sign(&payload);
        record.signature = sig.to_bytes().to_vec();
        record
    }

    #[test]
    fn verify_valid_record() {
        let record = make_valid_node_record();
        assert!(DhtRecordManager::verify_incoming_record(&record).is_ok());
    }

    #[test]
    fn verify_record_rejects_mismatched_node_id() {
        let mut record = make_valid_node_record();
        record.node_id = vec![0u8; 32];
        assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
    }

    #[test]
    fn verify_record_rejects_expired() {
        let mut record = make_valid_node_record();
        record.expires_at_ms = now_unix_ms() - 1;
        assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
    }

    #[test]
    fn verify_record_rejects_ttl_too_long() {
        let mut record = make_valid_node_record();
        record.expires_at_ms = now_unix_ms() + 60_000;
        assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
    }

    #[test]
    fn verify_record_rejects_invalid_signature() {
        let mut record = make_valid_node_record();
        record.health_score = 0.99; // tamper after signing
        assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
    }

    #[test]
    fn verify_record_rejects_unknown_capacity_class() {
        let id = Identity::generate();
        let expires_at_ms = now_unix_ms() + 10_000;
        let mut record = NodeRecord {
            node_id: id.node_id.to_vec(),
            pubkey: id.verifying_key.as_bytes().to_vec(),
            region: "test".into(),
            capacity_class: "Z".into(),
            available_slots: 10,
            health_score: 0.5,
            expires_at_ms,
            signature: vec![],
        };
        let payload = record_signed_payload(&record);
        let sig = id.sign(&payload);
        record.signature = sig.to_bytes().to_vec();
        assert!(DhtRecordManager::verify_incoming_record(&record).is_err());
    }
}
