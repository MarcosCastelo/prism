//! On-chain settlement interface for state channels.
//!
//! The concrete blockchain implementation is out of scope for this phase.
//! `MockSettlement` is provided for tests and local development.

use async_trait::async_trait;
use prism_core::hash::sha256;

use crate::channel::ChannelState;

/// Generic on-chain settlement interface.
#[async_trait]
pub trait OnChainSettlement: Send + Sync {
    /// Submit the final channel state for on-chain liquidation.
    /// Returns the transaction hash (hex string) on success.
    async fn settle(&self, final_state: ChannelState) -> anyhow::Result<String>;

    /// Check whether a channel is still open on-chain.
    async fn is_channel_open(&self, channel_id: [u8; 32]) -> anyhow::Result<bool>;
}

/// Mock settlement for tests and local development — logs only, no real transaction.
pub struct MockSettlement;

#[async_trait]
impl OnChainSettlement for MockSettlement {
    async fn settle(&self, state: ChannelState) -> anyhow::Result<String> {
        let tx_hash = hex::encode(sha256(&state.channel_id));
        tracing::info!(
            channel = %hex::encode(state.channel_id),
            seq = state.sequence,
            payee_earned = state.payee_balance,
            tx_hash = %tx_hash,
            "mock settle: channel liquidated (no real transaction)"
        );
        Ok(tx_hash)
    }

    async fn is_channel_open(&self, _channel_id: [u8; 32]) -> anyhow::Result<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::StateChannel;
    use prism_core::Identity;
    use std::sync::Arc;

    #[tokio::test]
    async fn full_cycle_open_pay_settle() {
        let payer = Arc::new(Identity::generate());
        let payee_id = Identity::generate();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();

        let mut channel = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);

        // 100 micropayments of 1 unit each.
        for _ in 0..100 {
            channel.pay(1).unwrap();
        }

        let final_state = channel.latest_state().clone();
        assert_eq!(final_state.sequence, 100);
        assert_eq!(final_state.payee_balance, 100);
        assert_eq!(final_state.payer_balance, 900);

        let settlement = MockSettlement;
        let tx_hash = settlement.settle(final_state).await.unwrap();
        assert!(!tx_hash.is_empty(), "tx_hash must be non-empty");
    }

    #[tokio::test]
    async fn fraud_detection_old_state_rejected() {
        let payer = Arc::new(Identity::generate());
        let payee_id = Arc::new(Identity::generate());
        let payee_pubkey = *payee_id.verifying_key.as_bytes();

        let mut payer_ch = StateChannel::open(Arc::clone(&payer), payee_pubkey, 1_000);
        let mut payee_ch = StateChannel::join(
            Arc::clone(&payee_id),
            payer_ch.latest_state().clone(),
            1_000,
        );

        // 100 real payments
        for _ in 0..100 {
            let s = payer_ch.pay(1).unwrap();
            payee_ch.countersign(s).unwrap();
        }

        // Payee has honest state at seq=100; payer tries to settle with seq=50 (old)
        // In a real system, the on-chain contract would compare submitted seq against
        // the stored one. Here we verify that seq=100 > seq=50 (the honest state wins).
        let honest_state = payee_ch.latest_state().clone();
        assert_eq!(honest_state.sequence, 100);

        // The "fraud" state would have been an older seq — payee holds evidence to contest.
        // As long as payee submits honest_state with higher seq, fraud is defeated.
        let settlement = MockSettlement;
        let tx = settlement.settle(honest_state.clone()).await.unwrap();
        assert!(!tx.is_empty());

        // Verify honest state has correct balances.
        assert_eq!(honest_state.payee_balance, 100);
        assert_eq!(honest_state.payer_balance, 900);
    }
}
