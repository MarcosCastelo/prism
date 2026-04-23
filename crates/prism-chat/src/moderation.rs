use dashmap::DashMap;

use ed25519_dalek::VerifyingKey;
use prism_core::{hash::sha256, Identity};
use prism_proto::{Blocklist, ChatMessage};

const SCORE_MIN: i32 = -100;
const SCORE_MAX: i32 = 100;
const SCORE_DISPLAY_THRESHOLD: i32 = -30;

pub struct ModerationEngine {
    active_blocklist: Option<Blocklist>,
    local_scores: DashMap<String, i32>,
}

impl ModerationEngine {
    pub fn new() -> Self {
        Self { active_blocklist: None, local_scores: DashMap::new() }
    }

    /// Updates the blocklist from gossip.
    /// Verifies the streamer's signature before applying.
    /// Rejects if version <= current (replay protection).
    pub fn apply_blocklist(
        &mut self,
        bl: Blocklist,
        streamer_pubkey: &VerifyingKey,
    ) -> anyhow::Result<()> {
        // Replay protection: reject older or equal version.
        if let Some(current) = &self.active_blocklist {
            if bl.version <= current.version {
                anyhow::bail!(
                    "blocklist version {} is not newer than current {}",
                    bl.version,
                    current.version
                );
            }
        }

        // Verify signature.
        if !verify_blocklist_signature(&bl, streamer_pubkey) {
            anyhow::bail!("blocklist signature invalid");
        }

        // Apply: set score to -100 for all blocked pubkeys.
        for entry in &bl.entries {
            let hex = hex::encode(&entry.blocked_pubkey);
            self.local_scores.insert(hex, SCORE_MIN);
        }

        self.active_blocklist = Some(bl);
        Ok(())
    }

    /// Returns true if the message should be displayed (not blocked, score >= threshold).
    pub fn should_display(&self, msg: &ChatMessage) -> bool {
        let pubkey_hex = hex::encode(&msg.sender_pubkey);
        let score = self.local_scores.get(&pubkey_hex).map(|s| *s).unwrap_or(0);
        score >= SCORE_DISPLAY_THRESHOLD
    }

    /// Records a valid message (increments score, capped at SCORE_MAX).
    pub fn record_valid_message(&self, pubkey_hex: &str) {
        let mut score = self.local_scores.entry(pubkey_hex.to_string()).or_insert(0);
        *score = (*score + 1).min(SCORE_MAX);
    }

    /// Penalizes a pubkey by reducing its score.
    pub fn penalize(&self, pubkey_hex: &str, amount: i32) {
        let mut score = self.local_scores.entry(pubkey_hex.to_string()).or_insert(0);
        *score = (*score - amount).max(SCORE_MIN);
    }

    /// Exports the current blocklist signed by the given identity (used by the streamer).
    pub fn export_blocklist(&self, stream_id: &str, identity: &Identity) -> Blocklist {
        let entries = self
            .active_blocklist
            .as_ref()
            .map(|bl| bl.entries.clone())
            .unwrap_or_default();

        let version = self
            .active_blocklist
            .as_ref()
            .map(|bl| bl.version + 1)
            .unwrap_or(1);

        let streamer_pubkey = identity.verifying_key.as_bytes().to_vec();

        let mut bl = Blocklist {
            stream_id: stream_id.to_string(),
            streamer_pubkey: streamer_pubkey.clone(),
            entries,
            version,
            signature: vec![],
        };

        let payload = blocklist_signing_payload(&bl);
        let payload_hash = sha256(&payload);
        let sig = identity.sign(&payload_hash);
        bl.signature = sig.to_bytes().to_vec();
        bl
    }
}

impl Default for ModerationEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Builds the signing payload for a Blocklist: SHA-256(fields 1-4 concatenated).
fn blocklist_signing_payload(bl: &Blocklist) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(bl.stream_id.as_bytes());
    payload.extend_from_slice(&bl.streamer_pubkey);
    // Entries: serialize each entry in order.
    for entry in &bl.entries {
        payload.extend_from_slice(&entry.blocked_pubkey);
        payload.extend_from_slice(entry.reason.as_bytes());
        payload.extend_from_slice(&entry.blocked_at_ms.to_le_bytes());
    }
    payload.extend_from_slice(&bl.version.to_le_bytes());
    payload
}

/// Verifies that Blocklist.signature is valid under the given streamer pubkey.
pub fn verify_blocklist_signature(bl: &Blocklist, streamer_pubkey: &VerifyingKey) -> bool {
    use ed25519_dalek::{Signature, Verifier};

    let sig_bytes: [u8; 64] = match bl.signature.as_slice().try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let signature = Signature::from_bytes(&sig_bytes);

    let payload = blocklist_signing_payload(bl);
    let payload_hash = sha256(&payload);
    streamer_pubkey.verify(&payload_hash, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_signed_blocklist(identity: &Identity, stream_id: &str, version: u64) -> Blocklist {
        let streamer_pubkey = identity.verifying_key.as_bytes().to_vec();
        let mut bl = Blocklist {
            stream_id: stream_id.to_string(),
            streamer_pubkey,
            entries: vec![],
            version,
            signature: vec![],
        };
        let payload = blocklist_signing_payload(&bl);
        let payload_hash = sha256(&payload);
        let sig = identity.sign(&payload_hash);
        bl.signature = sig.to_bytes().to_vec();
        bl
    }

    #[test]
    fn valid_blocklist_applied() {
        let streamer = Identity::generate();
        let mut engine = ModerationEngine::new();
        let bl = make_signed_blocklist(&streamer, "test", 1);
        engine.apply_blocklist(bl, &streamer.verifying_key).unwrap();
    }

    #[test]
    fn blocklist_with_invalid_signature_is_rejected() {
        let streamer = Identity::generate();
        let attacker = Identity::generate();
        let mut engine = ModerationEngine::new();

        // Signed by attacker, but we verify against streamer's key.
        let fake_bl = make_signed_blocklist(&attacker, "test", 1);
        let result = engine.apply_blocklist(fake_bl, &streamer.verifying_key);
        assert!(result.is_err());
    }

    #[test]
    fn older_blocklist_version_is_rejected() {
        let streamer = Identity::generate();
        let mut engine = ModerationEngine::new();

        let bl_v2 = make_signed_blocklist(&streamer, "test", 2);
        engine.apply_blocklist(bl_v2, &streamer.verifying_key).unwrap();

        let bl_v1 = make_signed_blocklist(&streamer, "test", 1);
        assert!(engine.apply_blocklist(bl_v1, &streamer.verifying_key).is_err());
    }

    #[test]
    fn same_version_blocklist_rejected() {
        let streamer = Identity::generate();
        let mut engine = ModerationEngine::new();

        let bl = make_signed_blocklist(&streamer, "test", 1);
        engine.apply_blocklist(bl.clone(), &streamer.verifying_key).unwrap();
        // Replay: same version
        let bl2 = make_signed_blocklist(&streamer, "test", 1);
        assert!(engine.apply_blocklist(bl2, &streamer.verifying_key).is_err());
    }

    #[test]
    fn score_below_threshold_hides_messages() {
        let engine = ModerationEngine::new();
        let pubkey = vec![0u8; 32];
        let pubkey_hex = hex::encode(&pubkey);

        // Default score = 0, should display
        let msg = ChatMessage {
            id: "1".to_string(),
            sender_pubkey: pubkey.clone(),
            display_name: "X".to_string(),
            stream_id: "test".to_string(),
            text: "hi".to_string(),
            timestamp_ms: 0,
            prev_msg_id: String::new(),
            signature: vec![],
            vector_clock: Default::default(),
        };
        assert!(engine.should_display(&msg));

        engine.penalize(&pubkey_hex, 35);
        assert!(!engine.should_display(&msg));
    }

    #[test]
    fn export_blocklist_has_valid_signature() {
        let streamer = Identity::generate();
        let engine = ModerationEngine::new();
        let bl = engine.export_blocklist("my-stream", &streamer);
        assert!(verify_blocklist_signature(&bl, &streamer.verifying_key));
    }
}
