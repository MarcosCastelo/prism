//! State channel: off-chain micropayment channel between payer and payee.
//!
//! Security invariants:
//!   - sequence is monotonically increasing (prevents replay)
//!   - payer_balance + payee_balance == initial_balance (value conservation)
//!   - payer_balance only decreases (no inflation attacks)
//!   - Both parties' Ed25519 signatures required on each committed state

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{Signature, VerifyingKey};
use prism_core::{hash::sha256, Identity};

/// Signed state of a payment channel.
///
/// Canonical signing payload: SHA-256(fields 1–6, no signatures).
#[derive(Clone)]
pub struct ChannelState {
    pub channel_id:       [u8; 32],
    pub payer_pubkey:     [u8; 32],
    pub payee_pubkey:     [u8; 32],
    /// Monotonically increasing — only states with higher seq are valid.
    pub sequence:         u64,
    pub payer_balance:    u64,
    pub payee_balance:    u64,
    pub payer_signature:  [u8; 64],
    pub payee_signature:  [u8; 64],
}

pub struct StateChannel {
    state:           ChannelState,
    identity:        Arc<Identity>,
    initial_balance: u64,
}

impl StateChannel {
    /// Open a new channel. Returns the initial state signed by the payer.
    /// Payee counter-signs via `countersign()` on their side.
    /// On-chain registration is the caller's responsibility.
    pub fn open(payer: Arc<Identity>, payee_pubkey: [u8; 32], initial_balance: u64) -> Self {
        let payer_pubkey = *payer.verifying_key.as_bytes();
        let open_ts = now_ms();

        let mut id_input = Vec::with_capacity(72);
        id_input.extend_from_slice(&payer_pubkey);
        id_input.extend_from_slice(&payee_pubkey);
        id_input.extend_from_slice(&open_ts.to_le_bytes());
        let channel_id = sha256(&id_input);

        let mut state = ChannelState {
            channel_id,
            payer_pubkey,
            payee_pubkey,
            sequence: 0,
            payer_balance: initial_balance,
            payee_balance: 0,
            payer_signature: [0u8; 64],
            payee_signature: [0u8; 64],
        };

        let payload = signing_payload(&state);
        state.payer_signature = payer.sign(&payload).to_bytes();

        Self { state, identity: payer, initial_balance }
    }

    /// Apply a micropayment: increment sequence, transfer `amount` from payer to payee.
    /// Returns the new state signed by the payer. Payee must counter-sign.
    pub fn pay(&mut self, amount: u64) -> anyhow::Result<ChannelState> {
        if amount > self.state.payer_balance {
            anyhow::bail!(
                "insufficient payer balance: {} < {}",
                self.state.payer_balance,
                amount
            );
        }

        let mut new_state = self.state.clone();
        new_state.sequence += 1;
        new_state.payer_balance -= amount;
        new_state.payee_balance += amount;
        new_state.payee_signature = [0u8; 64];

        let payload = signing_payload(&new_state);
        new_state.payer_signature = self.identity.sign(&payload).to_bytes();

        self.state = new_state.clone();
        Ok(new_state)
    }

    /// Counter-sign an incoming payer state (called by payee).
    ///
    /// Validates:
    ///   1. sequence > current_sequence
    ///   2. payer_balance + payee_balance == initial_balance (value conservation)
    ///   3. payer_balance <= current payer_balance (no inflation)
    ///   4. payer Ed25519 signature is valid
    pub fn countersign(&mut self, state: ChannelState) -> anyhow::Result<ChannelState> {
        if state.sequence <= self.state.sequence {
            anyhow::bail!(
                "replay rejected: incoming seq={} <= current seq={}",
                state.sequence,
                self.state.sequence
            );
        }

        if state.payer_balance + state.payee_balance != self.initial_balance {
            anyhow::bail!(
                "value conservation violated: {} + {} != {}",
                state.payer_balance,
                state.payee_balance,
                self.initial_balance
            );
        }

        if state.payer_balance > self.state.payer_balance {
            anyhow::bail!(
                "payer balance increased — inflation attack: {} > {}",
                state.payer_balance,
                self.state.payer_balance
            );
        }

        let payload = signing_payload(&state);
        let payer_vk = VerifyingKey::from_bytes(&state.payer_pubkey)
            .map_err(|_| anyhow::anyhow!("invalid payer pubkey in incoming state"))?;
        let payer_sig = Signature::from_bytes(&state.payer_signature);
        if !Identity::verify(&payload, &payer_sig, &payer_vk) {
            anyhow::bail!("invalid payer signature on incoming state");
        }

        let mut countersigned = state;
        countersigned.payee_signature = self.identity.sign(&payload).to_bytes();
        self.state = countersigned.clone();
        Ok(countersigned)
    }

    /// Create a payee-side view of an existing channel from the payer's initial state.
    /// Call this on the payee after receiving the `open` state from the payer.
    pub fn join(payee: Arc<Identity>, initial_state: ChannelState, initial_balance: u64) -> Self {
        Self {
            state: initial_state,
            identity: payee,
            initial_balance,
        }
    }

    /// Return the most recent channel state.
    pub fn latest_state(&self) -> &ChannelState {
        &self.state
    }
}

/// Canonical signing payload for a ChannelState.
///
/// SHA-256(channel_id || payer_pubkey || payee_pubkey || sequence || payer_balance || payee_balance)
pub fn signing_payload(state: &ChannelState) -> Vec<u8> {
    let mut buf = Vec::with_capacity(32 + 32 + 32 + 8 + 8 + 8);
    buf.extend_from_slice(&state.channel_id);
    buf.extend_from_slice(&state.payer_pubkey);
    buf.extend_from_slice(&state.payee_pubkey);
    buf.extend_from_slice(&state.sequence.to_le_bytes());
    buf.extend_from_slice(&state.payer_balance.to_le_bytes());
    buf.extend_from_slice(&state.payee_balance.to_le_bytes());
    sha256(&buf).to_vec()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_payer_payee() -> (Arc<Identity>, Arc<Identity>) {
        (Arc::new(Identity::generate()), Arc::new(Identity::generate()))
    }

    #[test]
    fn open_creates_valid_initial_state() {
        let (payer, payee_id) = make_payer_payee();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();
        let ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);

        let s = ch.latest_state();
        assert_eq!(s.sequence, 0);
        assert_eq!(s.payer_balance, 1_000);
        assert_eq!(s.payee_balance, 0);
        assert_eq!(s.payer_pubkey, *payer.verifying_key.as_bytes());
        assert_eq!(s.payee_pubkey, payee_pubkey);

        // Payer signature must be valid.
        let payload = signing_payload(s);
        let vk = VerifyingKey::from_bytes(&s.payer_pubkey).unwrap();
        let sig = Signature::from_bytes(&s.payer_signature);
        assert!(Identity::verify(&payload, &sig, &vk));
    }

    #[test]
    fn pay_transfers_value_and_increments_sequence() {
        let (payer, payee_id) = make_payer_payee();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();
        let mut ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);

        let new_state = ch.pay(100).unwrap();
        assert_eq!(new_state.sequence, 1);
        assert_eq!(new_state.payer_balance, 900);
        assert_eq!(new_state.payee_balance, 100);
    }

    #[test]
    fn pay_100_times_correct_balances() {
        let (payer, payee_id) = make_payer_payee();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();
        let mut ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);

        for _ in 0..100 {
            ch.pay(1).unwrap();
        }

        let s = ch.latest_state();
        assert_eq!(s.sequence, 100);
        assert_eq!(s.payer_balance, 900);
        assert_eq!(s.payee_balance, 100);
    }

    #[test]
    fn pay_rejects_insufficient_balance() {
        let (payer, payee_id) = make_payer_payee();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();
        let mut ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 50);

        assert!(ch.pay(51).is_err());
        // Balance must be unchanged after rejection.
        assert_eq!(ch.latest_state().payer_balance, 50);
    }

    #[test]
    fn countersign_validates_and_signs() {
        let (payer, payee_id) = make_payer_payee();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();
        let mut payer_ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);
        let mut payee_ch = StateChannel::join(
            Arc::clone(&payee_id),
            payer_ch.latest_state().clone(),
            1_000,
        );

        let payment = payer_ch.pay(200).unwrap();
        let countersigned = payee_ch.countersign(payment).unwrap();

        // Both signatures must be valid.
        let payload = signing_payload(&countersigned);
        let payer_vk = VerifyingKey::from_bytes(&countersigned.payer_pubkey).unwrap();
        let payee_vk = VerifyingKey::from_bytes(&countersigned.payee_pubkey).unwrap();
        assert!(Identity::verify(&payload, &Signature::from_bytes(&countersigned.payer_signature), &payer_vk));
        assert!(Identity::verify(&payload, &Signature::from_bytes(&countersigned.payee_signature), &payee_vk));
    }

    #[test]
    fn countersign_rejects_replay() {
        let (payer, payee_id) = make_payer_payee();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();
        let mut payer_ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);
        let mut payee_ch = StateChannel::join(
            Arc::clone(&payee_id),
            payer_ch.latest_state().clone(),
            1_000,
        );

        let payment1 = payer_ch.pay(100).unwrap();
        payee_ch.countersign(payment1.clone()).unwrap();

        // Replaying the same state must fail.
        assert!(payee_ch.countersign(payment1).is_err());
    }

    #[test]
    fn countersign_rejects_fraudulent_state() {
        let (payer, payee_id) = make_payer_payee();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();
        let mut payer_ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);
        let _ = payer_ch.pay(100).unwrap();

        let mut payee_ch = StateChannel::join(
            Arc::clone(&payee_id),
            payer_ch.latest_state().clone(),
            1_000,
        );

        // Tamper: try to close with seq=50 (fraud — an older state would have higher payer_balance)
        // We simulate by directly crafting a state with invalid sig
        let mut fraud_state = payer_ch.latest_state().clone();
        fraud_state.sequence += 1;
        fraud_state.payer_balance = 100; // back to old balance (fraud attempt)
        fraud_state.payee_balance = 900;
        // sig is now invalid because we tampered the values

        assert!(payee_ch.countersign(fraud_state).is_err());
    }
}
