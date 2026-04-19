#![allow(dead_code)]

use super::benchmark::CapacityReport;

/// Returns the virtual node count for the given capacity report.
pub fn vnodes_count(report: &CapacityReport) -> u32 {
    report.vnodes_count
}

/// Returns the normalised weight (0.0–1.0).
pub fn weight(report: &CapacityReport) -> f32 {
    report.weight
}
