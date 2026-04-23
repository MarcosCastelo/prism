use dashmap::DashMap;
use std::time::Instant;

pub struct ChatRateLimiter {
    counters: DashMap<String, (u32, Instant)>,
}

impl ChatRateLimiter {
    pub fn new() -> Self {
        Self { counters: DashMap::new() }
    }

    /// Returns true if the message from this pubkey is allowed (≤ 2/s).
    pub fn allow(&self, pubkey_hex: &str) -> bool {
        let now = Instant::now();
        let mut entry = self.counters.entry(pubkey_hex.to_string()).or_insert((0, now));
        let (count, window_start) = entry.value_mut();
        if now.duration_since(*window_start).as_secs() >= 1 {
            *window_start = now;
            *count = 1;
            true
        } else if *count < 2 {
            *count += 1;
            true
        } else {
            false
        }
    }
}

impl Default for ChatRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_two_messages_per_second() {
        let limiter = ChatRateLimiter::new();
        assert!(limiter.allow("alice"));
        assert!(limiter.allow("alice"));
        assert!(!limiter.allow("alice"));
    }

    #[test]
    fn different_pubkeys_are_independent() {
        let limiter = ChatRateLimiter::new();
        assert!(limiter.allow("alice"));
        assert!(limiter.allow("alice"));
        assert!(!limiter.allow("alice"));
        // bob is unaffected
        assert!(limiter.allow("bob"));
        assert!(limiter.allow("bob"));
        assert!(!limiter.allow("bob"));
    }
}
