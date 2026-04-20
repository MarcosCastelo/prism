//! RTMP server — listens on TCP port 1935 and receives a live stream from OBS
//! (or any RTMP-capable encoder).
//!
//! # Protocol flow
//!
//! ```text
//! TCP accept → RTMP handshake (Handshake state machine)
//!            → ServerSession (handles connect/publish commands)
//!            → VideoDataReceived / AudioDataReceived events
//!            → MediaFrame sent via mpsc channel
//! ```
//!
//! One `tokio::spawn` per accepted connection; the channel carries frames to
//! whichever consumer (encoder pipeline, injector, etc.) owns the receiver.

use anyhow::{anyhow, Context, Result};
use rml_rtmp::handshake::{Handshake, HandshakeProcessResult, PeerType};
use rml_rtmp::sessions::{
    ServerSession, ServerSessionConfig, ServerSessionEvent, ServerSessionResult,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single video or audio frame received from OBS over RTMP.
///
/// For video frames the `data` bytes are in FLV video tag format (the first
/// byte encodes frame type + codec ID, followed by codec-specific data).
/// For audio frames the `data` bytes are in FLV audio tag format.
#[derive(Debug, Clone)]
pub struct MediaFrame {
    /// Raw FLV-encapsulated payload as delivered by rml_rtmp.
    pub data: Vec<u8>,
    /// Wall-clock presentation timestamp in milliseconds (RTMP stream time).
    pub timestamp_ms: u32,
    /// `true` for video frames, `false` for audio frames.
    pub is_video: bool,
    /// `true` when this is a keyframe (IDR). Always `false` for audio.
    ///
    /// Detected from the FLV video tag header: bits 7–4 of the first byte
    /// equal `0x1` for keyframe, `0x2` for inter-frame.
    pub is_keyframe: bool,
    /// Stream key supplied by the publisher (e.g. OBS stream key field).
    pub stream_key: String,
}

/// RTMP server that accepts one publisher and forwards media frames via a channel.
pub struct RtmpServer {
    /// TCP port to listen on (default: 1935).
    port: u16,
}

impl RtmpServer {
    /// Create a new server bound to `port`.
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    /// Start accepting connections.  Each accepted connection is handled in its
    /// own `tokio::spawn` task.  Frames are sent via `frame_tx`.
    ///
    /// This function runs indefinitely until an unrecoverable error occurs
    /// (e.g. the TCP bind fails).  Individual connection errors are logged and
    /// do not stop the server.
    pub async fn run(self, frame_tx: mpsc::Sender<MediaFrame>) -> Result<()> {
        let listener = TcpListener::bind(("0.0.0.0", self.port))
            .await
            .with_context(|| format!("failed to bind RTMP server on port {}", self.port))?;

        tracing::info!(port = self.port, "RTMP server listening");

        loop {
            let (stream, peer_addr) = listener
                .accept()
                .await
                .context("failed to accept TCP connection")?;

            tracing::info!(%peer_addr, "RTMP client connected");

            let tx = frame_tx.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, tx).await {
                    tracing::warn!(%peer_addr, error = %e, "RTMP connection closed with error");
                } else {
                    tracing::info!(%peer_addr, "RTMP connection closed");
                }
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Per-connection handler
// ---------------------------------------------------------------------------

/// Handle one RTMP connection from TCP accept to stream finish.
async fn handle_connection(mut stream: TcpStream, frame_tx: mpsc::Sender<MediaFrame>) -> Result<()> {
    // ---- Phase 1: RTMP handshake ----
    let remaining = rtmp_handshake(&mut stream).await?;

    // ---- Phase 2: RTMP session ----
    let config = ServerSessionConfig::new();
    let (mut session, initial_results) =
        ServerSession::new(config).map_err(|e| anyhow!("ServerSession::new failed: {e:?}"))?;

    // Process initial protocol messages (e.g. window-ack) produced by the session.
    let mut write_buf: Vec<u8> = Vec::new();
    let mut pending_frames: Vec<MediaFrame> = Vec::new();

    process_results(initial_results, &mut session, &mut pending_frames, &mut write_buf)?;
    flush_and_send(&mut stream, &frame_tx, &mut write_buf, &mut pending_frames).await?;

    // Process remaining bytes left over after the handshake.
    if !remaining.is_empty() {
        let results = session
            .handle_input(&remaining)
            .map_err(|e| anyhow!("handle_input (remaining) failed: {e:?}"))?;
        process_results(results, &mut session, &mut pending_frames, &mut write_buf)?;
        flush_and_send(&mut stream, &frame_tx, &mut write_buf, &mut pending_frames).await?;
    }

    // Main read loop.
    let mut buf = vec![0u8; 65_536];
    loop {
        let n = stream.read(&mut buf).await.context("TCP read error")?;
        if n == 0 {
            tracing::debug!("RTMP peer closed connection");
            break;
        }

        let results = session
            .handle_input(&buf[..n])
            .map_err(|e| anyhow!("handle_input failed: {e:?}"))?;

        process_results(results, &mut session, &mut pending_frames, &mut write_buf)?;
        flush_and_send(&mut stream, &frame_tx, &mut write_buf, &mut pending_frames).await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// RTMP handshake
// ---------------------------------------------------------------------------

/// Perform the three-way RTMP handshake (C0+C1 → S0+S1+S2 → C2).
///
/// Returns any bytes received after the handshake completed (part of the first
/// RTMP chunk, not consumed by the handshake state machine).
async fn rtmp_handshake(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut handshake = Handshake::new(PeerType::Server);
    let mut buf = vec![0u8; 4_096];

    loop {
        let n = stream.read(&mut buf).await.context("handshake TCP read")?;
        if n == 0 {
            return Err(anyhow!("connection closed during RTMP handshake"));
        }

        match handshake
            .process_bytes(&buf[..n])
            .map_err(|e| anyhow!("RTMP handshake error: {e:?}"))?
        {
            HandshakeProcessResult::InProgress { response_bytes } => {
                stream
                    .write_all(&response_bytes)
                    .await
                    .context("handshake write failed")?;
            }
            HandshakeProcessResult::Completed {
                response_bytes,
                remaining_bytes,
            } => {
                stream
                    .write_all(&response_bytes)
                    .await
                    .context("handshake write failed")?;
                return Ok(remaining_bytes);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Result processing (synchronous — no await)
// ---------------------------------------------------------------------------

/// Walk a `Vec<ServerSessionResult>`, collecting outbound bytes and media frames.
///
/// Events that require a server response (ConnectionRequested, PublishStreamRequested)
/// are acknowledged immediately via `session.accept_request`; the resulting
/// responses are appended to `outbound`.
fn process_results(
    results: Vec<ServerSessionResult>,
    session: &mut ServerSession,
    pending_frames: &mut Vec<MediaFrame>,
    outbound: &mut Vec<u8>,
) -> Result<()> {
    for result in results {
        match result {
            ServerSessionResult::OutboundResponse(packet) => {
                outbound.extend_from_slice(&packet.bytes);
            }

            ServerSessionResult::RaisedEvent(event) => {
                handle_event(event, session, pending_frames, outbound)?;
            }

            ServerSessionResult::UnhandleableMessageReceived(msg) => {
                tracing::debug!("unhandleable RTMP message: {:?}", msg);
            }
        }
    }
    Ok(())
}

/// Dispatch a single `ServerSessionEvent`.
fn handle_event(
    event: ServerSessionEvent,
    session: &mut ServerSession,
    pending_frames: &mut Vec<MediaFrame>,
    outbound: &mut Vec<u8>,
) -> Result<()> {
    match event {
        ServerSessionEvent::ConnectionRequested {
            request_id,
            app_name,
        } => {
            tracing::info!(app = %app_name, "RTMP connection requested — accepting");
            let more = session
                .accept_request(request_id)
                .map_err(|e| anyhow!("accept connection failed: {e:?}"))?;
            process_results(more, session, pending_frames, outbound)?;
        }

        ServerSessionEvent::PublishStreamRequested {
            request_id,
            app_name,
            stream_key,
            ..
        } => {
            tracing::info!(app = %app_name, key = %stream_key, "RTMP publish requested — accepting");
            let more = session
                .accept_request(request_id)
                .map_err(|e| anyhow!("accept publish failed: {e:?}"))?;
            process_results(more, session, pending_frames, outbound)?;
        }

        ServerSessionEvent::VideoDataReceived {
            data,
            timestamp,
            stream_key,
            ..
        } => {
            // FLV video tag: first byte = (frame_type << 4) | codec_id
            // frame_type 1 = keyframe, 2 = inter-frame
            let is_keyframe = data.first().map(|&b| (b >> 4) == 1).unwrap_or(false);

            pending_frames.push(MediaFrame {
                data: data.to_vec(),
                timestamp_ms: timestamp.value,
                is_video: true,
                is_keyframe,
                stream_key,
            });
        }

        ServerSessionEvent::AudioDataReceived {
            data,
            timestamp,
            stream_key,
            ..
        } => {
            pending_frames.push(MediaFrame {
                data: data.to_vec(),
                timestamp_ms: timestamp.value,
                is_video: false,
                is_keyframe: false,
                stream_key,
            });
        }

        ServerSessionEvent::PublishStreamFinished {
            app_name,
            stream_key,
        } => {
            tracing::info!(app = %app_name, key = %stream_key, "RTMP stream finished");
        }

        // Ignore playback, metadata, and other events not relevant to the
        // ingest pipeline.
        _ => {}
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Async flush helper
// ---------------------------------------------------------------------------

/// Write accumulated outbound bytes to the TCP stream and forward pending
/// media frames to the consumer channel.
async fn flush_and_send(
    stream: &mut TcpStream,
    frame_tx: &mpsc::Sender<MediaFrame>,
    outbound: &mut Vec<u8>,
    pending_frames: &mut Vec<MediaFrame>,
) -> Result<()> {
    if !outbound.is_empty() {
        stream
            .write_all(outbound)
            .await
            .context("TCP write failed")?;
        outbound.clear();
    }

    for frame in pending_frames.drain(..) {
        if frame_tx.send(frame).await.is_err() {
            // Consumer dropped — stop the connection gracefully.
            tracing::warn!("frame channel closed — stopping RTMP connection");
            return Err(anyhow!("frame channel closed"));
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtmp_server_new() {
        let srv = RtmpServer::new(1935);
        assert_eq!(srv.port, 1935);
    }

    #[test]
    fn media_frame_audio_is_not_keyframe() {
        let frame = MediaFrame {
            data: vec![0xAF, 0x01, 0x00],
            timestamp_ms: 100,
            is_video: false,
            is_keyframe: false,
            stream_key: "live".to_string(),
        };
        assert!(!frame.is_keyframe);
        assert!(!frame.is_video);
    }

    #[test]
    fn keyframe_detection_from_flv_byte() {
        // FLV video tag: first byte 0x17 → frame_type=1 (keyframe), codec=7 (AVC)
        let keyframe_byte: u8 = 0x17;
        let is_keyframe = (keyframe_byte >> 4) == 1;
        assert!(is_keyframe);

        // First byte 0x27 → frame_type=2 (inter-frame), codec=7
        let inter_byte: u8 = 0x27;
        let is_inter = (inter_byte >> 4) == 2;
        assert!(is_inter);
        assert_ne!((inter_byte >> 4), 1);
    }

    #[test]
    fn keyframe_detection_zero_data() {
        // Empty data should not be a keyframe.
        let is_keyframe = [].first().map(|&b: &u8| (b >> 4) == 1).unwrap_or(false);
        assert!(!is_keyframe);
    }

    #[tokio::test]
    async fn server_binds_to_available_port() {
        // Port 0 asks the OS for any available port.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port > 0);
        drop(listener);

        // RtmpServer::new just stores the port — no actual bind yet.
        let srv = RtmpServer::new(port);
        assert_eq!(srv.port, port);
    }

    #[test]
    fn media_frame_video_keyframe_fields() {
        let frame = MediaFrame {
            data: vec![0x17, 0x00],
            timestamp_ms: 3_000,
            is_video: true,
            is_keyframe: true,
            stream_key: "mystream".to_string(),
        };
        assert!(frame.is_video);
        assert!(frame.is_keyframe);
        assert_eq!(frame.timestamp_ms, 3_000);
        assert_eq!(frame.stream_key, "mystream");
    }
}
