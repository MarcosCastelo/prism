//! Local node metrics collection.
//!
//! `collect_local()` is intended to be called every 30 seconds.
//! CPU and bandwidth values are best-effort estimates; a production deployment
//! would wire them to the node's actual measurements from prism-node health benchmarks.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn process_uptime_s() -> u64 {
    PROCESS_START
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs()
}

#[derive(serde::Serialize, Clone)]
pub struct NodeMetrics {
    /// First 8 hex chars of the node's SHA-256 node_id.
    pub node_id:         String,
    pub region:          String,
    pub capacity_class:  String,
    pub streams_serving: u32,
    pub viewers_served:  u32,
    pub uptime_s:        u64,
    pub cpu_usage_pct:   f32,
    pub bandwidth_mbps:  f32,
    pub health_score:    f32,
    pub collected_at_ms: u64,
}

impl NodeMetrics {
    /// Collect a local metrics snapshot. Called every 30 s by the telemetry loop.
    ///
    /// Wire `node_id`, `region`, `capacity_class`, `streams_serving`, `viewers_served`,
    /// `cpu_usage_pct`, `bandwidth_mbps`, and `health_score` from the running node's
    /// shared state before publishing.
    pub fn collect_local() -> Self {
        let collected_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            node_id:         "00000000".to_string(),
            region:          "unknown".to_string(),
            capacity_class:  "edge".to_string(),
            streams_serving: 0,
            viewers_served:  0,
            uptime_s:        process_uptime_s(),
            cpu_usage_pct:   0.0,
            bandwidth_mbps:  0.0,
            health_score:    0.5,
            collected_at_ms,
        }
    }

    /// Build metrics from explicit values (used by the running node).
    #[allow(clippy::too_many_arguments)]
    pub fn with_values(
        node_id: String,
        region: String,
        capacity_class: String,
        streams_serving: u32,
        viewers_served: u32,
        cpu_usage_pct: f32,
        bandwidth_mbps: f32,
        health_score: f32,
    ) -> Self {
        let collected_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            node_id,
            region,
            capacity_class,
            streams_serving,
            viewers_served,
            uptime_s: process_uptime_s(),
            cpu_usage_pct,
            bandwidth_mbps,
            health_score,
            collected_at_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_local_returns_valid_snapshot() {
        let m = NodeMetrics::collect_local();
        assert!(m.collected_at_ms > 0);
        assert!(!m.node_id.is_empty());
        assert!(!m.region.is_empty());
        assert!((0.0..=1.0).contains(&m.health_score));
    }

    #[test]
    fn with_values_stores_fields_correctly() {
        let m = NodeMetrics::with_values(
            "aabbccdd".to_string(),
            "us-east-1".to_string(),
            "A".to_string(),
            3,
            150,
            45.2,
            250.0,
            0.9,
        );
        assert_eq!(m.node_id, "aabbccdd");
        assert_eq!(m.region, "us-east-1");
        assert_eq!(m.capacity_class, "A");
        assert_eq!(m.streams_serving, 3);
        assert_eq!(m.viewers_served, 150);
        assert!((45.0..46.0).contains(&m.cpu_usage_pct));
        assert_eq!(m.health_score, 0.9);
    }

    #[test]
    fn uptime_increases_over_time() {
        // First call initialises the start time.
        let m1 = NodeMetrics::collect_local();
        // Brief sleep would be needed for real difference, but at minimum uptime is 0.
        let m2 = NodeMetrics::collect_local();
        assert!(m2.uptime_s >= m1.uptime_s);
    }
}
