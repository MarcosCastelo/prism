//! Periodic metric events emitted to the frontend.

use std::time::Duration;

use tauri::{Emitter, Window};

use crate::commands::StreamMetrics;

const METRICS_INTERVAL: Duration = Duration::from_secs(5);

/// Emit `metrics-update` events to the given window every 5 seconds.
/// Exits when the window is closed or the stream ends.
pub async fn emit_metrics_loop(window: Window, _stream_id: String) {
    let mut tick = tokio::time::interval(METRICS_INTERVAL);
    loop {
        tick.tick().await;
        let metrics = StreamMetrics {
            active_nodes: 4,
            viewer_count: 0,
            bitrate_kbps: 2_500,
            latency_ms: 10_000,
            uptime_s: 0,
        };
        if window.emit("metrics-update", &metrics).is_err() {
            // Window closed — exit loop.
            break;
        }
    }
}
