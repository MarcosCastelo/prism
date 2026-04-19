use std::time::{Duration, Instant};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct CapacityReport {
    pub cpu_score: f32,
    pub bandwidth_mbps: f32,
    pub weight: f32,
    pub vnodes_count: u32,
    pub capacity_class: String,
}

pub async fn run_benchmark(config: &Config) -> CapacityReport {
    if let Some(class) = &config.capacity_class {
        let class = class.clone();
        tracing::info!(capacity_class = %class, "using manual capacity override");
        return CapacityReport {
            cpu_score: 0.5,
            bandwidth_mbps: 10.0,
            weight: 0.5,
            vnodes_count: 50,
            capacity_class: class,
        };
    }

    let cpu_score = measure_cpu_score().await;
    // Bandwidth estimation: placeholder using a small allocation time heuristic.
    // Real measurement would require an external probe; 10 Mbps is a conservative default.
    let bandwidth_mbps = estimate_bandwidth_mbps();

    let bw_score = (bandwidth_mbps / 100.0_f32).min(1.0);
    let weight = cpu_score.min(bw_score);
    let vnodes_count = ((weight * 100.0).floor() as u32).max(1);
    let capacity_class = classify(cpu_score, bandwidth_mbps);

    tracing::info!(
        cpu_score,
        bandwidth_mbps,
        weight,
        vnodes_count,
        capacity_class = %capacity_class,
        "benchmark complete"
    );

    CapacityReport { cpu_score, bandwidth_mbps, weight, vnodes_count, capacity_class }
}

async fn measure_cpu_score() -> f32 {
    tokio::task::spawn_blocking(|| {
        use sha2::{Digest, Sha256};
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut ops: u64 = 0;
        let data = [0u8; 64];
        while Instant::now() < deadline {
            let mut h = Sha256::new();
            h.update(data);
            let _ = h.finalize();
            ops += 1;
        }
        (ops as f32 / 1_000_000.0).min(1.0)
    })
    .await
    .unwrap_or(0.1)
}

fn estimate_bandwidth_mbps() -> f32 {
    // Placeholder: return a default. Real implementation would use a UDP probe.
    10.0
}

fn classify(cpu_score: f32, bandwidth_mbps: f32) -> String {
    if cpu_score < 0.3 && bandwidth_mbps >= 10.0 {
        "A".into()
    } else if cpu_score >= 0.5 {
        "B".into()
    } else if bandwidth_mbps < 5.0 {
        "C".into()
    } else {
        "edge".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_class_a() {
        assert_eq!(classify(0.2, 15.0), "A");
    }

    #[test]
    fn classify_class_b() {
        assert_eq!(classify(0.6, 7.0), "B");
    }

    #[test]
    fn classify_class_c() {
        assert_eq!(classify(0.4, 3.0), "C");
    }

    #[test]
    fn classify_edge() {
        assert_eq!(classify(0.4, 7.0), "edge");
    }

    #[test]
    fn vnodes_min_one() {
        let report = CapacityReport {
            cpu_score: 0.0,
            bandwidth_mbps: 0.0,
            weight: 0.0,
            vnodes_count: ((0.0_f32 * 100.0).floor() as u32).max(1),
            capacity_class: "edge".into(),
        };
        assert_eq!(report.vnodes_count, 1);
    }
}
