//! Micropayment scheduler: sends one payment per second of stream delivered.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tokio::sync::Mutex;

use crate::channel::{ChannelState, StateChannel};

pub struct MicropaymentScheduler {
    channel:      Arc<Mutex<StateChannel>>,
    rate_per_sec: u64,
    running:      Arc<AtomicBool>,
}

impl MicropaymentScheduler {
    pub fn new(channel: Arc<Mutex<StateChannel>>, rate_per_sec: u64) -> Self {
        Self {
            channel,
            rate_per_sec,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the payment loop: one `channel.pay(rate_per_sec)` per second.
    /// If the channel balance is exhausted, the loop stops automatically.
    /// Payee counter-signature timeout handling is the integration layer's responsibility.
    pub async fn start(&self, stream_id: &str) {
        self.running.store(true, Ordering::Relaxed);

        let channel = Arc::clone(&self.channel);
        let rate = self.rate_per_sec;
        let running = Arc::clone(&self.running);
        let stream_id = stream_id.to_string();

        tokio::spawn(async move {
            while running.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;

                let mut ch = channel.lock().await;
                match ch.pay(rate) {
                    Ok(state) => {
                        tracing::debug!(
                            stream = %stream_id,
                            seq = state.sequence,
                            payee_balance = state.payee_balance,
                            "micropayment sent"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            stream = %stream_id,
                            err = %e,
                            "micropayment failed, stopping scheduler"
                        );
                        running.store(false, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });
    }

    /// Stop the payment loop and return the latest channel state for settlement.
    pub async fn stop(&self) -> ChannelState {
        self.running.store(false, Ordering::Relaxed);
        self.channel.lock().await.latest_state().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::StateChannel;
    use prism_core::Identity;

    #[tokio::test]
    async fn scheduler_stop_returns_latest_state() {
        let payer = Arc::new(Identity::generate());
        let payee_id = Identity::generate();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();

        let channel = StateChannel::open(Arc::clone(&payer), payee_pubkey, 10_000);
        let channel = Arc::new(Mutex::new(channel));

        let scheduler = MicropaymentScheduler::new(Arc::clone(&channel), 100);
        scheduler.start("test-stream").await;

        // Wait 2 payments
        tokio::time::sleep(std::time::Duration::from_millis(2_200)).await;

        let state = scheduler.stop().await;
        assert!(state.sequence >= 2, "expected >= 2 payments, got seq={}", state.sequence);
        assert!(state.payee_balance >= 200, "expected payee_balance >= 200");
        assert_eq!(state.payer_balance + state.payee_balance, 10_000);
    }

    #[tokio::test]
    async fn scheduler_stops_on_exhausted_balance() {
        let payer = Arc::new(Identity::generate());
        let payee_id = Identity::generate();
        let payee_pubkey = *payee_id.verifying_key.as_bytes();

        let channel = StateChannel::open(Arc::clone(&payer), payee_pubkey, 3);
        let channel = Arc::new(Mutex::new(channel));

        let scheduler = MicropaymentScheduler::new(Arc::clone(&channel), 1);
        scheduler.start("test-stream").await;

        // 4 seconds → balance exhausted after 3 payments
        tokio::time::sleep(std::time::Duration::from_millis(4_200)).await;

        let state = scheduler.stop().await;
        assert_eq!(state.payer_balance, 0);
        assert_eq!(state.payee_balance, 3);
    }
}
