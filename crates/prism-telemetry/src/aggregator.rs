//! Distributed telemetry aggregation via gossipsub.
//!
//! k-anonymity rule: `network_summary()` returns a summary over all received metrics.
//! Callers MUST check `total_nodes >= 10` before propagating the summary to other nodes
//! or exposing it to viewers — this prevents deanonymisation when too few nodes are known.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;

use crate::collector::NodeMetrics;

/// Minimum number of contributing nodes before a summary may be propagated (k-anonymity).
pub const K_ANONYMITY_THRESHOLD: u32 = 10;

/// Aggregated view of the Prism network. Never contains per-viewer data.
#[derive(serde::Serialize, Clone)]
pub struct NetworkSummary {
    pub total_nodes:      u32,
    pub total_streams:    u32,
    pub total_viewers:    u32,
    pub avg_health_score: f32,
    pub nodes_by_class:   HashMap<String, u32>,
    pub nodes_by_region:  HashMap<String, u32>,
    pub summary_at_ms:    u64,
}

impl NetworkSummary {
    /// Returns true if this summary meets the k-anonymity threshold and may be propagated.
    pub fn is_propagatable(&self) -> bool {
        self.total_nodes >= K_ANONYMITY_THRESHOLD
    }
}

/// Aggregates `NodeMetrics` received from gossipsub peers.
///
/// Gossipsub topic: `"prism/telemetry"` (integration wired externally).
pub struct TelemetryAggregator {
    received: DashMap<String, NodeMetrics>,
}

impl TelemetryAggregator {
    pub fn new() -> Self {
        Self { received: DashMap::new() }
    }

    /// Record (or replace) the latest metrics from a node.
    /// Only the most recent snapshot per `node_id` is kept.
    pub fn on_metrics_received(&self, metrics: NodeMetrics) {
        self.received.insert(metrics.node_id.clone(), metrics);
    }

    /// Return a network-wide summary aggregated from all known nodes.
    ///
    /// Never exposes individual node data — only aggregate counters.
    /// Check `summary.is_propagatable()` before forwarding to peers.
    pub fn network_summary(&self) -> NetworkSummary {
        let nodes: Vec<NodeMetrics> = self
            .received
            .iter()
            .map(|e| e.value().clone())
            .collect();

        let total_nodes = nodes.len() as u32;
        let total_streams: u32 = nodes.iter().map(|n| n.streams_serving).sum();
        let total_viewers: u32 = nodes.iter().map(|n| n.viewers_served).sum();
        let avg_health_score = if total_nodes > 0 {
            nodes.iter().map(|n| n.health_score).sum::<f32>() / total_nodes as f32
        } else {
            0.0
        };

        let mut nodes_by_class: HashMap<String, u32> = HashMap::new();
        let mut nodes_by_region: HashMap<String, u32> = HashMap::new();
        for node in &nodes {
            *nodes_by_class.entry(node.capacity_class.clone()).or_insert(0) += 1;
            *nodes_by_region.entry(node.region.clone()).or_insert(0) += 1;
        }

        let summary_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        NetworkSummary {
            total_nodes,
            total_streams,
            total_viewers,
            avg_health_score,
            nodes_by_class,
            nodes_by_region,
            summary_at_ms,
        }
    }
}

impl Default for TelemetryAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::NodeMetrics;

    fn make_node(id: &str, class: &str, region: &str, streams: u32, viewers: u32) -> NodeMetrics {
        NodeMetrics::with_values(
            id.to_string(),
            region.to_string(),
            class.to_string(),
            streams,
            viewers,
            20.0,
            100.0,
            0.8,
        )
    }

    #[test]
    fn empty_aggregator_returns_zero_summary() {
        let agg = TelemetryAggregator::new();
        let s = agg.network_summary();
        assert_eq!(s.total_nodes, 0);
        assert_eq!(s.total_streams, 0);
        assert_eq!(s.total_viewers, 0);
        assert!(!s.is_propagatable());
    }

    #[test]
    fn aggregates_totals_correctly() {
        let agg = TelemetryAggregator::new();
        for i in 0..5u32 {
            agg.on_metrics_received(make_node(&format!("node{i}"), "A", "us-east", 2, 100));
        }
        let s = agg.network_summary();
        assert_eq!(s.total_nodes, 5);
        assert_eq!(s.total_streams, 10);
        assert_eq!(s.total_viewers, 500);
    }

    #[test]
    fn k_anonymity_threshold_enforced() {
        let agg = TelemetryAggregator::new();

        // 9 nodes → not propagatable
        for i in 0..9u32 {
            agg.on_metrics_received(make_node(&format!("n{i}"), "edge", "eu-west", 1, 10));
        }
        assert!(!agg.network_summary().is_propagatable());

        // 10th node pushes it over the threshold
        agg.on_metrics_received(make_node("n9", "edge", "eu-west", 1, 10));
        assert!(agg.network_summary().is_propagatable());
    }

    #[test]
    fn nodes_by_class_and_region_correct() {
        let agg = TelemetryAggregator::new();
        agg.on_metrics_received(make_node("a1", "A", "us-east", 0, 0));
        agg.on_metrics_received(make_node("b1", "B", "us-east", 0, 0));
        agg.on_metrics_received(make_node("b2", "B", "eu-west", 0, 0));

        let s = agg.network_summary();
        assert_eq!(s.nodes_by_class["A"], 1);
        assert_eq!(s.nodes_by_class["B"], 2);
        assert_eq!(s.nodes_by_region["us-east"], 2);
        assert_eq!(s.nodes_by_region["eu-west"], 1);
    }

    #[test]
    fn latest_metrics_replace_older_ones() {
        let agg = TelemetryAggregator::new();
        agg.on_metrics_received(make_node("node1", "A", "us", 1, 50));
        agg.on_metrics_received(make_node("node1", "A", "us", 5, 200)); // updated

        let s = agg.network_summary();
        assert_eq!(s.total_nodes, 1);
        assert_eq!(s.total_streams, 5);
        assert_eq!(s.total_viewers, 200);
    }

    #[test]
    fn avg_health_score_computed_correctly() {
        let agg = TelemetryAggregator::new();
        agg.on_metrics_received(NodeMetrics::with_values(
            "n1".into(), "us".into(), "A".into(), 0, 0, 0.0, 0.0, 0.4,
        ));
        agg.on_metrics_received(NodeMetrics::with_values(
            "n2".into(), "us".into(), "A".into(), 0, 0, 0.0, 0.0, 0.8,
        ));

        let s = agg.network_summary();
        assert!((s.avg_health_score - 0.6).abs() < 0.001);
    }
}
