use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct VectorClock(pub HashMap<String, u64>);

impl VectorClock {
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn increment(&mut self, sender_pubkey_hex: &str) {
        let counter = self.0.entry(sender_pubkey_hex.to_string()).or_insert(0);
        *counter += 1;
    }

    pub fn merge(&mut self, other: &VectorClock) {
        for (key, &val) in &other.0 {
            let entry = self.0.entry(key.clone()).or_insert(0);
            if val > *entry {
                *entry = val;
            }
        }
    }

    /// Returns true if self causally precedes other
    /// (self <= other in all entries, with at least one strictly less).
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        let mut strictly_less = false;
        for (key, &self_val) in &self.0 {
            let other_val = other.0.get(key).copied().unwrap_or(0);
            if self_val > other_val {
                return false;
            }
            if self_val < other_val {
                strictly_less = true;
            }
        }
        // keys in other but not in self count as self=0
        for (key, &other_val) in &other.0 {
            if !self.0.contains_key(key) && other_val > 0 {
                strictly_less = true;
            }
        }
        strictly_less
    }

    /// Returns true if the clocks are concurrent
    /// (neither A precedes B, nor B precedes A).
    pub fn concurrent_with(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self) && self != other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happens_before_is_correctly_detected() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("alice");

        let mut vc_b = vc_a.clone();
        vc_b.increment("bob");

        assert!(vc_a.happens_before(&vc_b));
        assert!(!vc_b.happens_before(&vc_a));
    }

    #[test]
    fn concurrent_clocks_detected() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("alice");

        let mut vc_b = VectorClock::new();
        vc_b.increment("bob");

        assert!(vc_a.concurrent_with(&vc_b));
        assert!(vc_b.concurrent_with(&vc_a));
    }

    #[test]
    fn merge_takes_max_per_key() {
        let mut vc_a = VectorClock::new();
        vc_a.increment("alice");
        vc_a.increment("alice");

        let mut vc_b = VectorClock::new();
        vc_b.increment("alice");
        vc_b.increment("bob");

        vc_a.merge(&vc_b);
        assert_eq!(vc_a.0["alice"], 2);
        assert_eq!(vc_a.0["bob"], 1);
    }

    #[test]
    fn equal_clocks_not_concurrent_not_before() {
        let mut vc = VectorClock::new();
        vc.increment("alice");
        let vc2 = vc.clone();
        assert!(!vc.happens_before(&vc2));
        assert!(!vc.concurrent_with(&vc2));
    }

    #[test]
    fn empty_clock_happens_before_non_empty() {
        let vc_empty = VectorClock::new();
        let mut vc_b = VectorClock::new();
        vc_b.increment("alice");
        assert!(vc_empty.happens_before(&vc_b));
    }
}
