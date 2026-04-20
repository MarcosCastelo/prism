//! A/V capture from camera or screen.
//!
//! Camera capture uses [`nokhwa`] when the `camera` Cargo feature is enabled.
//! Without that feature (or on unsupported platforms) every call that would
//! touch hardware returns a descriptive `Err`.
//!
//! Screen capture is stubbed — it will be implemented in a future sprint using
//! a platform-specific crate (DXGI on Windows, Pipewire/XCB on Linux).
//!
//! All blocking device I/O is wrapped in [`tokio::task::spawn_blocking`] so the
//! async runtime is never stalled.
//!
//! # Feature: `camera`
//!
//! Enable in `Cargo.toml`:
//! ```toml
//! prism-ingest = { path = "...", features = ["camera"] }
//! ```
//!
//! **Windows note:** the `camera` feature depends on `mozjpeg-sys`, which requires
//! MSVC `cl.exe` (not clang-cl) as the C compiler.  Set
//! `CC_x86_64_pc_windows_msvc=cl.exe` before building.

use anyhow::{anyhow, Context, Result};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Selects the capture input source.
#[derive(Debug, Clone)]
pub enum CaptureInput {
    /// Physical camera device.  `index` is the OS device index (0 = first camera).
    Camera { index: u32 },
    /// Desktop / monitor capture.  `monitor_index` is 0-based.
    ///
    /// Currently not implemented — returns `Err` on all platforms.
    Screen { monitor_index: u32 },
}

/// A single raw YUV420 (I420) video frame ready for the SVT-AV1 encoder.
#[derive(Debug, Clone)]
pub struct RawFrame {
    /// Planar YUV420 (I420) data:
    /// * Y plane: `width × height` bytes
    /// * U plane: `(width/2) × (height/2)` bytes
    /// * V plane: `(width/2) × (height/2)` bytes
    pub yuv420: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Wall-clock timestamp in milliseconds (Unix epoch).
    pub pts_ms: u64,
}

/// Synchronous capture handle.
///
/// Open the device with [`CaptureSource::open`], then call
/// [`CaptureSource::capture_frame`] in a loop.  Use [`run_blocking_capture_loop`]
/// for the full async-compatible pattern.
pub struct CaptureSource {
    input: CaptureInput,
    #[allow(dead_code)]
    target_width: u32,
    #[allow(dead_code)]
    target_height: u32,
    #[allow(dead_code)]
    target_fps: u32,
    #[cfg(feature = "camera")]
    camera: Option<nokhwa::Camera>,
}

impl CaptureSource {
    /// Create a new capture source without opening the device.
    pub fn new(input: CaptureInput, width: u32, height: u32, fps: u32) -> Result<Self> {
        Ok(Self {
            input,
            target_width: width,
            target_height: height,
            target_fps: fps,
            #[cfg(feature = "camera")]
            camera: None,
        })
    }

    /// Open and start the capture device.
    pub fn open(&mut self) -> Result<()> {
        match &self.input {
            CaptureInput::Camera { index } => {
                #[cfg(feature = "camera")]
                {
                    use nokhwa::{
                        pixel_format::RgbFormat,
                        utils::{CameraIndex, RequestedFormat, RequestedFormatType},
                        Camera,
                    };

                    let cam_index = CameraIndex::Index(*index);
                    let requested = RequestedFormat::new::<RgbFormat>(
                        RequestedFormatType::AbsoluteHighestFrameRate,
                    );
                    let mut cam = Camera::new(cam_index, requested)
                        .map_err(|e| anyhow!("failed to open camera {index}: {e}"))?;

                    cam.open_stream()
                        .map_err(|e| anyhow!("failed to start camera stream: {e}"))?;

                    tracing::info!(
                        index,
                        width = self.target_width,
                        height = self.target_height,
                        fps = self.target_fps,
                        "camera capture started"
                    );
                    self.camera = Some(cam);
                    Ok(())
                }
                #[cfg(not(feature = "camera"))]
                {
                    let _ = index;
                    Err(anyhow!(
                        "camera capture requires the 'camera' feature — rebuild with \
                         `features = [\"camera\"]` and ensure MSVC cl.exe is available"
                    ))
                }
            }
            CaptureInput::Screen { monitor_index } => Err(anyhow!(
                "screen capture is not yet implemented (monitor_index={monitor_index})"
            )),
        }
    }

    /// Capture one YUV420 frame.  Blocks until a frame is available.
    ///
    /// Call [`open`] first.
    pub fn capture_frame(&mut self) -> Result<RawFrame> {
        match self.input {
            CaptureInput::Camera { .. } => {
                #[cfg(feature = "camera")]
                {
                    use nokhwa::pixel_format::RgbFormat;

                    let pts_ms = current_ms();
                    let cam = self
                        .camera
                        .as_mut()
                        .ok_or_else(|| anyhow!("camera not opened — call open() first"))?;

                    let buffer = cam
                        .frame()
                        .map_err(|e| anyhow!("camera frame capture failed: {e}"))?;

                    let resolution = buffer.resolution();
                    let width = resolution.width();
                    let height = resolution.height();

                    let rgb_img = buffer
                        .decode_image::<RgbFormat>()
                        .map_err(|e| anyhow!("frame decode to RGB failed: {e}"))?;

                    let yuv420 = rgb_to_yuv420(rgb_img.as_raw(), width, height);
                    Ok(RawFrame { yuv420, width, height, pts_ms })
                }
                #[cfg(not(feature = "camera"))]
                Err(anyhow!("camera capture requires the 'camera' feature"))
            }
            CaptureInput::Screen { .. } => {
                Err(anyhow!("screen capture is not yet implemented"))
            }
        }
    }

    /// Stop the device and free resources.
    pub fn close(&mut self) {
        #[cfg(feature = "camera")]
        if let Some(mut cam) = self.camera.take() {
            if let Err(e) = cam.stop_stream() {
                tracing::warn!(error = %e, "error stopping camera stream");
            }
        }
    }
}

impl Drop for CaptureSource {
    fn drop(&mut self) {
        self.close();
    }
}

// ---------------------------------------------------------------------------
// Async capture loop
// ---------------------------------------------------------------------------

/// Run the full open + capture loop on a blocking thread, sending frames via
/// `frame_tx`.
///
/// Returns when `frame_tx` is closed (consumer dropped) or a fatal error occurs.
pub async fn run_blocking_capture_loop(
    mut source: CaptureSource,
    frame_tx: mpsc::Sender<RawFrame>,
) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        source.open()?;

        loop {
            let frame = source.capture_frame()?;
            if frame_tx.blocking_send(frame).is_err() {
                tracing::info!("capture loop stopped: frame receiver dropped");
                break;
            }
        }

        source.close();
        Ok(())
    })
    .await
    .context("capture blocking task panicked")?
}

// ---------------------------------------------------------------------------
// RGB → YUV420 (I420) conversion — pure Rust, no optional deps
// ---------------------------------------------------------------------------

/// Convert packed RGB24 to planar YUV420 (I420) using BT.601 studio-swing.
///
/// `rgb` must be `width × height × 3` bytes (R, G, B interleaved).
///
/// Output layout:
/// ```text
/// [Y plane: w×h] [U plane: w/2 × h/2] [V plane: w/2 × h/2]
/// ```
pub fn rgb_to_yuv420(rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = (w / 2) * (h / 2);
    let mut yuv = vec![0u8; y_size + uv_size * 2];

    let (y_plane, uv_planes) = yuv.split_at_mut(y_size);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_size);

    for row in 0..h {
        for col in 0..w {
            let px = &rgb[(row * w + col) * 3..];
            let r = px[0] as i32;
            let g = px[1] as i32;
            let b = px[2] as i32;

            // BT.601 studio swing (integer approximation, 8-bit fixed point)
            let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_plane[row * w + col] = y.clamp(16, 235) as u8;

            // U and V: 2×2 chroma subsampling (average not needed for real-time)
            if row % 2 == 0 && col % 2 == 0 {
                let uv_idx = (row / 2) * (w / 2) + (col / 2);
                let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
                let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
                u_plane[uv_idx] = u.clamp(16, 240) as u8;
                v_plane[uv_idx] = v.clamp(16, 240) as u8;
            }
        }
    }

    yuv
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

#[cfg_attr(not(feature = "camera"), allow(dead_code))]
fn current_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- YUV conversion ----

    #[test]
    fn yuv420_output_size_is_correct() {
        let rgb = vec![0u8; 4 * 4 * 3];
        let yuv = rgb_to_yuv420(&rgb, 4, 4);
        // Y(16) + U(4) + V(4) = 24
        assert_eq!(yuv.len(), 4 * 4 + 4 + 4);
    }

    #[test]
    fn yuv420_pure_white_y_is_studio_white() {
        let rgb = vec![255u8; 8 * 8 * 3];
        let yuv = rgb_to_yuv420(&rgb, 8, 8);
        // Studio-swing white: Y should be near 235
        let y = yuv[0];
        assert!(y >= 230 && y <= 235, "white Y={y}");
    }

    #[test]
    fn yuv420_pure_black_y_is_studio_black() {
        let rgb = vec![0u8; 8 * 8 * 3];
        let yuv = rgb_to_yuv420(&rgb, 8, 8);
        // Studio-swing black: Y should be near 16
        let y = yuv[0];
        assert!(y >= 16 && y <= 20, "black Y={y}");
    }

    #[test]
    fn yuv420_pure_red_v_is_elevated() {
        // Red (255, 0, 0) → high V, Y~81
        let mut rgb = vec![0u8; 8 * 8 * 3];
        for i in 0..64 {
            rgb[i * 3] = 255;
        }
        let yuv = rgb_to_yuv420(&rgb, 8, 8);
        let y = yuv[0];
        let v_offset = 8 * 8 + 4 * 4; // after Y + U planes
        let v = yuv[v_offset];
        assert!(y >= 75 && y <= 90, "red Y={y}");
        assert!(v > 150, "red V={v}");
    }

    #[test]
    fn yuv420_u_v_neutral_on_grey() {
        // Grey (128, 128, 128) → U and V should be near 128
        let rgb = vec![128u8; 8 * 8 * 3];
        let yuv = rgb_to_yuv420(&rgb, 8, 8);
        let u = yuv[8 * 8];
        let v = yuv[8 * 8 + 4 * 4];
        assert!(u >= 120 && u <= 136, "grey U={u}");
        assert!(v >= 120 && v <= 136, "grey V={v}");
    }

    // ---- CaptureSource interface ----

    #[test]
    fn capture_source_creation_succeeds() {
        let src = CaptureSource::new(CaptureInput::Camera { index: 0 }, 1280, 720, 30);
        assert!(src.is_ok());
    }

    #[test]
    fn screen_capture_open_returns_err() {
        let mut src = CaptureSource::new(
            CaptureInput::Screen { monitor_index: 0 },
            1920, 1080, 30,
        )
        .unwrap();
        assert!(src.open().is_err());
    }

    #[test]
    fn camera_open_without_feature_returns_err() {
        // Without the 'camera' feature, opening a camera returns a useful error.
        #[cfg(not(feature = "camera"))]
        {
            let mut src = CaptureSource::new(
                CaptureInput::Camera { index: 0 },
                1280, 720, 30,
            )
            .unwrap();
            let err = src.open().unwrap_err();
            assert!(
                err.to_string().contains("camera"),
                "error should mention camera: {err}"
            );
        }
        #[cfg(feature = "camera")]
        {
            // With the camera feature, opening may succeed or fail depending on hardware.
            // Just assert the function is callable.
            let _ = CaptureSource::new(CaptureInput::Camera { index: 0 }, 1280, 720, 30);
        }
    }

    #[test]
    fn capture_frame_without_open_returns_err() {
        // Calling capture_frame before open() must never panic.
        #[cfg(not(feature = "camera"))]
        {
            let mut src = CaptureSource::new(
                CaptureInput::Camera { index: 0 },
                640, 480, 30,
            )
            .unwrap();
            assert!(src.capture_frame().is_err());
        }
    }

    #[test]
    fn capture_input_debug_format() {
        let _ = format!("{:?}", CaptureInput::Camera { index: 2 });
        let _ = format!("{:?}", CaptureInput::Screen { monitor_index: 1 });
    }
}
