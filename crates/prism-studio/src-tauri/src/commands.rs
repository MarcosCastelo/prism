//! Tauri commands invoked from the React frontend via `invoke("command_name", {...})`.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State, Window};
use tokio::sync::Mutex;

use crate::events::emit_metrics_loop;

// ---------------------------------------------------------------------------
// Shared app state
// ---------------------------------------------------------------------------

pub struct StreamState {
    pub stream_id: Option<String>,
    pub started_at: Option<Instant>,
    pub quality_preset: Option<String>,
}

pub type AppState = Arc<Mutex<StreamState>>;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct StreamMetrics {
    pub active_nodes: u32,
    pub viewer_count: u32,
    pub bitrate_kbps: u32,
    pub latency_ms: u32,
    pub uptime_s: u64,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Start a live stream.
///
/// Returns the `stream_id` on success or an error message string.
#[tauri::command]
pub async fn start_stream(
    window: Window,
    title: String,
    quality_preset: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let mut s = state.lock().await;
    if s.stream_id.is_some() {
        return Err("a stream is already running".to_string());
    }

    tracing::info!(title, quality_preset, "starting stream");

    // Derive a deterministic stream_id placeholder (real impl uses Identity + start_ts).
    let stream_id = format!("{:x}", rand_stream_id(&title));
    s.stream_id = Some(stream_id.clone());
    s.started_at = Some(Instant::now());
    s.quality_preset = Some(quality_preset.clone());

    // Start the metrics emission loop (fires every 5 s).
    let w = window.clone();
    let sid = stream_id.clone();
    tokio::spawn(async move {
        emit_metrics_loop(w, sid).await;
    });

    tracing::info!(stream_id, "stream started");
    Ok(stream_id)
}

/// Stop the running stream.
#[tauri::command]
pub async fn stop_stream(state: State<'_, AppState>) -> Result<(), String> {
    let mut s = state.lock().await;
    if s.stream_id.is_none() {
        return Err("no stream is running".to_string());
    }
    tracing::info!(stream_id = ?s.stream_id, "stopping stream");
    s.stream_id = None;
    s.started_at = None;
    s.quality_preset = None;
    Ok(())
}

/// Return current stream metrics (polled by frontend and emitted via events).
#[tauri::command]
pub async fn get_metrics(state: State<'_, AppState>) -> Result<StreamMetrics, String> {
    let s = state.lock().await;
    let uptime_s = s
        .started_at
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    Ok(StreamMetrics {
        active_nodes: 4,  // real impl: query DHT neighbour table
        viewer_count: 0,  // real impl: count manifest requests on Class A
        bitrate_kbps: quality_to_bitrate(s.quality_preset.as_deref().unwrap_or("low")),
        latency_ms: 10_000,
        uptime_s,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn quality_to_bitrate(preset: &str) -> u32 {
    match preset {
        "low"    => 400,
        "medium" => 1_000,
        "high"   => 2_500,
        "ultra"  => 5_500,
        _        => 400,
    }
}

fn rand_stream_id(seed: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    seed.hash(&mut h);
    h.finish()
}

/// Build the Tauri application with all commands registered.
pub fn app_handle(state: AppState) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![start_stream, stop_stream, get_metrics])
}
