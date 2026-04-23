//! Studio RTMP server facade.
//!
//! Wraps `prism_ingest::rtmp_server::RtmpServer` and exposes the interface
//! defined in the PRD: `start()`, `stop()`, and `frame_stream()`.
//!
//! The ingest crate owns the low-level RTMP protocol handling; this module
//! provides the lifecycle API the Studio frontend commands use.

use std::sync::Arc;

use futures_core::Stream;
use prism_ingest::rtmp_server::{MediaFrame, RtmpServer as IngestRtmpServer};
use tokio::sync::{mpsc, Mutex};

pub use prism_ingest::rtmp_server::MediaFrame as RawFrame;

// ── Public interface (PRD-specified) ─────────────────────────────────────────

pub struct RtmpServer {
    frame_rx:    Arc<Mutex<mpsc::Receiver<MediaFrame>>>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl RtmpServer {
    /// Start the RTMP server on `port` (default 1935) in a background task.
    pub async fn start(port: u16) -> anyhow::Result<Self> {
        let (frame_tx, frame_rx) = mpsc::channel::<MediaFrame>(256);
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let server = IngestRtmpServer::new(port);
        tokio::spawn(async move {
            tokio::select! {
                res = server.run(frame_tx) => {
                    if let Err(e) = res {
                        tracing::error!("RTMP server error: {e}");
                    }
                }
                _ = &mut shutdown_rx => {
                    tracing::info!("RTMP server shutting down");
                }
            }
        });

        tracing::info!(port, "RTMP server started");
        Ok(Self {
            frame_rx: Arc::new(Mutex::new(frame_rx)),
            shutdown_tx,
        })
    }

    /// Stop the RTMP server. Signals the background task to exit.
    pub async fn stop(self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Async-stream of frames received from OBS/ffmpeg.
    ///
    /// Frames are produced in real time as OBS sends them over RTMP.
    /// The stream ends when the server is stopped or the RTMP connection closes.
    pub fn frame_stream(&self) -> impl Stream<Item = RawFrame> + '_ {
        let rx = Arc::clone(&self.frame_rx);
        async_stream::stream! {
            loop {
                let frame = {
                    let mut guard = rx.lock().await;
                    guard.recv().await
                };
                match frame {
                    Some(f) => yield f,
                    None => break,
                }
            }
        }
    }
}
