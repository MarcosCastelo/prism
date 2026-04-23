//! Tauri commands invoked from the React frontend via `invoke("command_name", {...})`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{State, Window};
use tokio::sync::Mutex;

use prism_core::Identity;
use prism_encoder::segmenter::Segment;
use prism_ingest::{injector::StreamInjector, rtmp_server::{MediaFrame, RtmpServer}};
use prism_node::streamer_router::EmbeddedSeedRouter;

use crate::events::emit_metrics_loop;

// ─────────────────────────────────────────────────────────────────────────────
// Shared app state
// ─────────────────────────────────────────────────────────────────────────────

pub struct StreamState {
    pub stream_id: Option<String>,
    pub started_at: Option<Instant>,
    pub quality_preset: Option<String>,
    /// Signals the ingest pipeline task to stop gracefully.
    pub shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

pub type AppState = Arc<Mutex<StreamState>>;

// ─────────────────────────────────────────────────────────────────────────────
// DTOs
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct StreamMetrics {
    pub active_nodes: u32,
    pub viewer_count: u32,
    pub bitrate_kbps: u32,
    pub latency_ms: u32,
    pub uptime_s: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Commands
// ─────────────────────────────────────────────────────────────────────────────

/// Start a live stream.
///
/// Wires the full pipeline:
///   1. Load (or generate) the streamer Ed25519 identity from disk.
///   2. Create an `EmbeddedSeedRouter` pointing to seed nodes from
///      `PRISM_SEED_ADDRS` env var (`host:port,host:port,...`).
///   3. Create a `StreamInjector` — derives `stream_id` from identity + timestamp.
///   4. Start the RTMP server on port 1935 (OBS or ffmpeg connects here).
///   5. Spawn a pipeline task that groups RTMP frames into 3-second segments
///      and injects each segment into the P2P network.
///
/// Returns the 64-char hex `stream_id` to display in the UI.
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

    // ── 1. Identity ──────────────────────────────────────────────────────────
    let key_path = std::path::PathBuf::from("prism_studio.key");
    let identity = Arc::new(load_or_create_identity(&key_path).map_err(|e: anyhow::Error| e.to_string())?);
    tracing::info!(
        node_id = %hex::encode(&identity.node_id[..4]),
        "streamer identity ready"
    );

    // ── 2. Seed router ───────────────────────────────────────────────────────
    let router = Arc::new(EmbeddedSeedRouter::from_env());

    // ── 3. Injector ──────────────────────────────────────────────────────────
    let start_ts = now_unix_ms();
    let n_layers = layers_for_preset(&quality_preset);
    let injector = Arc::new(StreamInjector::new(
        Arc::clone(&identity),
        start_ts,
        n_layers,
        router,
    ));
    let stream_id = injector.stream_id().to_string();

    // ── 4 + 5. Ingest pipeline ───────────────────────────────────────────────
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let inj = Arc::clone(&injector);
    tokio::spawn(async move {
        run_ingest_pipeline(inj, shutdown_rx).await;
    });

    // ── Metrics loop ─────────────────────────────────────────────────────────
    let w = window.clone();
    let sid = stream_id.clone();
    tokio::spawn(async move {
        emit_metrics_loop(w, sid).await;
    });

    s.stream_id = Some(stream_id.clone());
    s.started_at = Some(Instant::now());
    s.quality_preset = Some(quality_preset);
    s.shutdown_tx = Some(shutdown_tx);

    tracing::info!(stream_id, "stream started — RTMP listening on :1935");
    Ok(stream_id)
}

/// Stop the running stream and shut down the ingest pipeline.
#[tauri::command]
pub async fn stop_stream(state: State<'_, AppState>) -> Result<(), String> {
    let mut s = state.lock().await;
    if s.stream_id.is_none() {
        return Err("no stream is running".to_string());
    }
    tracing::info!(stream_id = ?s.stream_id, "stopping stream");

    if let Some(tx) = s.shutdown_tx.take() {
        let _ = tx.send(());
    }

    s.stream_id = None;
    s.started_at = None;
    s.quality_preset = None;
    Ok(())
}

/// Return current stream metrics (also emitted via `"metrics-update"` events).
#[tauri::command]
pub async fn get_metrics(state: State<'_, AppState>) -> Result<StreamMetrics, String> {
    let s = state.lock().await;
    let uptime_s = s.started_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
    Ok(StreamMetrics {
        active_nodes: 4,
        viewer_count: 0,
        bitrate_kbps: quality_to_bitrate(s.quality_preset.as_deref().unwrap_or("low")),
        latency_ms: 10_000,
        uptime_s,
    })
}

/// Build the Tauri application with all commands registered.
pub fn app_handle(
    state: AppState,
    onboarding: crate::onboarding::OnboardingState,
) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .manage(state)
        .manage(onboarding)
        .invoke_handler(tauri::generate_handler![
            start_stream,
            stop_stream,
            get_metrics,
            crate::onboarding::onboarding_status,
            crate::onboarding::onboarding_generate_identity,
            crate::onboarding::onboarding_export_backup,
            crate::onboarding::onboarding_run_benchmark,
            crate::onboarding::onboarding_set_source,
            crate::onboarding::onboarding_complete,
        ])
}

// ─────────────────────────────────────────────────────────────────────────────
// Ingest pipeline
// ─────────────────────────────────────────────────────────────────────────────

/// Run until `shutdown` fires or the RTMP server exits.
///
/// Pipeline per frame received from OBS/ffmpeg:
/// ```text
/// RtmpServer (port 1935)
///   → MediaFrame (raw FLV video)
///   → accumulate into 3-second windows
///   → Segment { data, duration_ms, pts_start }
///   → StreamInjector::inject()  (signs + sends to seed nodes)
/// ```
///
/// Note: this MVP wraps raw RTMP FLV data as segment payload. A future
/// iteration will insert `prism-encoder` (SVT-AV1 → fMP4) between the
/// RTMP frame and the injector.
async fn run_ingest_pipeline(
    injector: Arc<StreamInjector>,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    let (frame_tx, mut frame_rx) = tokio::sync::mpsc::channel::<MediaFrame>(64);

    tokio::spawn(async move {
        if let Err(e) = RtmpServer::new(1935).run(frame_tx).await {
            tracing::error!("RTMP server error: {e}");
        }
    });

    let segment_window = Duration::from_secs(3);
    let mut buf: Vec<u8> = Vec::new();
    let mut window_start = Instant::now();
    let mut pts: u64 = 0;
    let mut shutdown = std::pin::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("ingest pipeline: shutdown signal received");
                break;
            }
            frame = frame_rx.recv() => {
                let Some(frame) = frame else { break };
                if !frame.is_video { continue; }

                buf.extend_from_slice(&frame.data);

                if window_start.elapsed() >= segment_window && !buf.is_empty() {
                    let data = std::mem::take(&mut buf);
                    let segment = Segment { data, duration_ms: 3_000, pts_start: pts };
                    pts += 3_000;
                    window_start = Instant::now();

                    let inj = Arc::clone(&injector);
                    tokio::spawn(async move {
                        if let Err(e) = inj.inject(segment).await {
                            tracing::warn!("inject error: {e}");
                        }
                    });
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn load_or_create_identity(path: &std::path::Path) -> anyhow::Result<Identity> {
    if path.exists() {
        Identity::load(path)
    } else {
        let id = Identity::generate();
        id.save(path)?;
        Ok(id)
    }
}

fn layers_for_preset(preset: &str) -> u8 {
    match preset {
        "low"    => 1,
        "medium" => 2,
        "high"   => 3,
        "ultra"  => 4,
        _        => 1,
    }
}

fn quality_to_bitrate(preset: &str) -> u32 {
    match preset {
        "low"    => 400,
        "medium" => 1_000,
        "high"   => 2_500,
        "ultra"  => 5_500,
        _        => 400,
    }
}

fn now_unix_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

