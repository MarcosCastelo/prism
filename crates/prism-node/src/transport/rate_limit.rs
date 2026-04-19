use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Instant;

struct IpState {
    attempts: Vec<Instant>,
    #[allow(dead_code)]
    auth_failures: u32,
    ban_until: Option<Instant>,
}

pub struct ConnectionRateLimiter {
    state: Mutex<HashMap<IpAddr, IpState>>,
}

const WINDOW_SECS: u64 = 10;
const MAX_CONN_PER_WINDOW: usize = 10;
const MAX_IPS_PER_SLASH24: usize = 5;
#[allow(dead_code)]
const AUTH_FAIL_BAN_THRESHOLD: u32 = 5;
#[allow(dead_code)]
const AUTH_FAIL_BAN_SECS: u64 = 60;

impl ConnectionRateLimiter {
    pub fn new() -> Self {
        Self { state: Mutex::new(HashMap::new()) }
    }

    pub fn allow_connection(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let window = std::time::Duration::from_secs(WINDOW_SECS);

        let mut guard = self.state.lock().expect("rate limiter lock poisoned");

        // Check ban
        if let Some(entry) = guard.get(&ip) {
            if let Some(ban_until) = entry.ban_until {
                if now < ban_until {
                    return false;
                }
            }
        }

        // Count distinct /24 peers
        let slash24 = subnet24(ip);
        let slash24_count = guard
            .keys()
            .filter(|&&k| k != ip && subnet24(k) == slash24)
            .count();
        if slash24_count >= MAX_IPS_PER_SLASH24 {
            return false;
        }

        // Sliding window check
        let entry = guard.entry(ip).or_insert(IpState {
            attempts: Vec::new(),
            auth_failures: 0,
            ban_until: None,
        });

        entry.attempts.retain(|&t| now.duration_since(t) < window);

        if entry.attempts.len() >= MAX_CONN_PER_WINDOW {
            return false;
        }

        entry.attempts.push(now);

        true
    }

    #[allow(dead_code)]
    pub fn record_auth_failure(&self, ip: IpAddr) {
        let now = Instant::now();
        let mut guard = self.state.lock().expect("rate limiter lock poisoned");
        let entry = guard.entry(ip).or_insert(IpState {
            attempts: Vec::new(),
            auth_failures: 0,
            ban_until: None,
        });
        entry.auth_failures += 1;
        if entry.auth_failures >= AUTH_FAIL_BAN_THRESHOLD {
            entry.ban_until =
                Some(now + std::time::Duration::from_secs(AUTH_FAIL_BAN_SECS));
            tracing::warn!(ip = %ip, "IP banned for 60s after repeated auth failures");
        }
    }
}

impl Default for ConnectionRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

fn subnet24(ip: IpAddr) -> u32 {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            u32::from_be_bytes([o[0], o[1], o[2], 0])
        }
        IpAddr::V6(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn allows_up_to_limit() {
        let rl = ConnectionRateLimiter::new();
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        for _ in 0..MAX_CONN_PER_WINDOW {
            assert!(rl.allow_connection(ip));
        }
        assert!(!rl.allow_connection(ip));
    }

    #[test]
    fn bans_after_auth_failures() {
        let rl = ConnectionRateLimiter::new();
        let ip = IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8));
        for _ in 0..AUTH_FAIL_BAN_THRESHOLD {
            rl.record_auth_failure(ip);
        }
        assert!(!rl.allow_connection(ip));
    }

    #[test]
    fn limits_slash24() {
        let rl = ConnectionRateLimiter::new();
        // Register 5 distinct IPs in same /24
        for i in 1..=5u8 {
            let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, i));
            rl.allow_connection(ip);
        }
        // 6th IP from same /24 should be blocked
        let ip6 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 6));
        assert!(!rl.allow_connection(ip6));
        // Different /24 is fine
        let other = IpAddr::V4(Ipv4Addr::new(10, 0, 1, 1));
        assert!(rl.allow_connection(other));
    }
}
