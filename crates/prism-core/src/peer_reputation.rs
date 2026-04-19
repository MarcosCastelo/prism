use dashmap::DashMap;

pub struct PeerReputation {
    entries: DashMap<[u8; 32], ReputationEntry>,
}

#[derive(Clone)]
pub struct ReputationEntry {
    pub score: i32,
    pub ban_until_ms: u64,
    pub invalid_count: u32,
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl PeerReputation {
    pub fn new() -> Self {
        Self { entries: DashMap::new() }
    }

    pub fn penalize(&self, node_id: &[u8; 32], penalty: i32, reason: &str) {
        let mut entry = self.entries.entry(*node_id).or_insert(ReputationEntry {
            score: 0,
            ban_until_ms: 0,
            invalid_count: 0,
        });
        entry.score -= penalty;
        entry.invalid_count += 1;

        if entry.score < -50 && entry.ban_until_ms == 0 {
            let ban_until = now_unix_ms() + 60_000;
            entry.ban_until_ms = ban_until;
            tracing::warn!(
                node_id = %hex::encode(&node_id[..4]),
                reason = reason,
                score = entry.score,
                "peer banned for 60s"
            );
        } else {
            tracing::warn!(
                node_id = %hex::encode(&node_id[..4]),
                reason = reason,
                score = entry.score,
                "peer penalized"
            );
        }
    }

    pub fn reward(&self, node_id: &[u8; 32], reward: i32) {
        let mut entry = self.entries.entry(*node_id).or_insert(ReputationEntry {
            score: 0,
            ban_until_ms: 0,
            invalid_count: 0,
        });
        entry.score = (entry.score + reward).min(100);
    }

    pub fn is_banned(&self, node_id: &[u8; 32]) -> bool {
        if let Some(entry) = self.entries.get(node_id) {
            if entry.ban_until_ms > 0 && now_unix_ms() < entry.ban_until_ms {
                return true;
            }
        }
        false
    }

    pub fn score(&self, node_id: &[u8; 32]) -> Option<i32> {
        self.entries.get(node_id).map(|e| e.score)
    }
}

impl Default for PeerReputation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn penalize_triggers_ban_at_threshold() {
        let rep = PeerReputation::new();
        let node_id = [1u8; 32];
        rep.penalize(&node_id, 25, "test");
        rep.penalize(&node_id, 25, "test");
        assert!(!rep.is_banned(&node_id)); // score = -50, not yet banned
        rep.penalize(&node_id, 1, "test"); // score = -51, ban activated
        assert!(rep.is_banned(&node_id));
    }

    #[test]
    fn reward_caps_at_100() {
        let rep = PeerReputation::new();
        let node_id = [2u8; 32];
        rep.reward(&node_id, 200);
        assert_eq!(rep.score(&node_id), Some(100));
    }

    #[test]
    fn unknown_node_not_banned() {
        let rep = PeerReputation::new();
        assert!(!rep.is_banned(&[99u8; 32]));
        assert_eq!(rep.score(&[99u8; 32]), None);
    }
}
