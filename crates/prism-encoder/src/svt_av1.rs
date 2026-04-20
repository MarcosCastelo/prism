//! FFI wrapper for libsvtav1 (C library).
//!
//! When `libsvtav1-dev` is installed, build.rs generates bindings and sets
//! `cfg(svtav1_available)`.  Without the library the public API compiles but
//! every call returns `Err("SVT-AV1 library not available …")`.

#[cfg(svtav1_available)]
#[allow(non_upper_case_globals, non_camel_case_types, non_snake_case, dead_code, improper_ctypes)]
mod ffi {
    include!(concat!(env!("OUT_DIR"), "/svt_av1_bindings.rs"));
}

use anyhow::{anyhow, Result};

/// Configuration for a single-resolution SVT-AV1 encode session.
pub struct EncodeConfig {
    pub width: u32,
    pub height: u32,
    /// Target frame rate (30 or 60).
    pub fps: u32,
    /// Total target bitrate across all SVC layers (kbps).
    pub bitrate_kbps: u32,
    /// Number of SVC spatial layers: 1–4 (L0 only … L0–L3).
    pub svc_layers: u8,
    /// `true` for live streaming (enables low-delay mode).
    pub low_delay: bool,
    /// Encoder speed preset: 9–12.  Higher values encode faster with lower quality.
    pub preset: i8,
}

/// Handle to an active SVT-AV1 encode session.
///
/// Call [`encode_frame`] repeatedly, then [`flush`] once when done.
pub struct SvtAv1Encoder {
    #[cfg(svtav1_available)]
    handle: *mut ffi::EbComponentType,
    /// Cached config for validation inside encode calls.
    config: EncodeConfig,
}

// SAFETY: The SVT-AV1 C handle is not thread-safe; callers must ensure
// single-threaded access (which tokio::task::spawn_blocking guarantees).
#[cfg(svtav1_available)]
unsafe impl Send for SvtAv1Encoder {}

impl SvtAv1Encoder {
    /// Create and initialise a new encoder session.
    pub fn new(config: EncodeConfig) -> Result<Self> {
        #[cfg(svtav1_available)]
        {
            use std::ptr;

            let mut handle: *mut ffi::EbComponentType = ptr::null_mut();
            let mut enc_cfg: ffi::EbSvtAv1EncConfiguration = unsafe { std::mem::zeroed() };

            // Initialise handle.
            let ret = unsafe { ffi::svt_av1_enc_init_handle(&mut handle, ptr::null_mut(), &mut enc_cfg) };
            if ret != 0 {
                return Err(anyhow!("svt_av1_enc_init_handle failed: error {ret}"));
            }

            // Set parameters.
            enc_cfg.source_width = config.width;
            enc_cfg.source_height = config.height;
            enc_cfg.frame_rate_numerator = config.fps;
            enc_cfg.frame_rate_denominator = 1;
            enc_cfg.target_bit_rate = config.bitrate_kbps * 1_000;
            enc_cfg.enc_mode = config.preset;
            enc_cfg.low_delay_mode = if config.low_delay { 1 } else { 0 };
            enc_cfg.hierarchical_levels = config.svc_layers.saturating_sub(1) as u32;

            let ret = unsafe { ffi::svt_av1_enc_set_parameter(handle, &mut enc_cfg) };
            if ret != 0 {
                unsafe { ffi::svt_av1_enc_deinit_handle(handle) };
                return Err(anyhow!("svt_av1_enc_set_parameter failed: error {ret}"));
            }

            let ret = unsafe { ffi::svt_av1_enc_init(handle) };
            if ret != 0 {
                unsafe { ffi::svt_av1_enc_deinit_handle(handle) };
                return Err(anyhow!("svt_av1_enc_init failed: error {ret}"));
            }

            tracing::info!(
                width = config.width,
                height = config.height,
                fps = config.fps,
                bitrate_kbps = config.bitrate_kbps,
                preset = config.preset,
                "SVT-AV1 encoder initialised"
            );

            return Ok(Self { handle, config });
        }

        #[cfg(not(svtav1_available))]
        {
            let _ = config;
            tracing::warn!(
                "SVT-AV1 library not available at build time — encoder is a no-op stub"
            );
            Err(anyhow!(
                "SVT-AV1 library not available: install libsvtav1-dev and rebuild"
            ))
        }
    }

    /// Feed one raw YUV420 frame.  Returns zero or more encoded OBU packets.
    ///
    /// `frame` must be exactly `width * height * 3 / 2` bytes (planar YUV420).
    /// `pts`   is the presentation timestamp in the stream's time base.
    pub fn encode_frame(&mut self, frame: &[u8], pts: u64) -> Result<Vec<Vec<u8>>> {
        let expected = (self.config.width * self.config.height * 3 / 2) as usize;
        if frame.len() != expected {
            return Err(anyhow!(
                "frame size mismatch: expected {} bytes, got {}",
                expected,
                frame.len()
            ));
        }

        #[cfg(svtav1_available)]
        {
            use std::ptr;

            // Build input buffer header.
            let mut io_fmt: ffi::EbSvtIOFormat = unsafe { std::mem::zeroed() };
            io_fmt.luma = frame.as_ptr() as *mut u8;
            io_fmt.cb = unsafe { frame.as_ptr().add((self.config.width * self.config.height) as usize) as *mut u8 };
            io_fmt.cr = unsafe {
                frame.as_ptr().add((self.config.width * self.config.height + self.config.width * self.config.height / 4) as usize) as *mut u8
            };
            io_fmt.y_stride = self.config.width;
            io_fmt.cb_stride = self.config.width / 2;
            io_fmt.cr_stride = self.config.width / 2;

            let mut in_hdr: ffi::EbBufferHeaderType = unsafe { std::mem::zeroed() };
            in_hdr.size = std::mem::size_of::<ffi::EbBufferHeaderType>() as u32;
            in_hdr.p_buffer = &mut io_fmt as *mut _ as *mut u8;
            in_hdr.n_filled_len = frame.len() as u32;
            in_hdr.pts = pts as i64;
            in_hdr.pic_type = ffi::EB_AV1_INVALID_PICTURE as u32;
            in_hdr.flags = 0;

            let ret = unsafe { ffi::svt_av1_enc_send_picture(self.handle, &mut in_hdr) };
            if ret != 0 {
                return Err(anyhow!("svt_av1_enc_send_picture failed: error {ret}"));
            }

            return self.drain_output();
        }

        #[cfg(not(svtav1_available))]
        {
            let _ = pts;
            Err(anyhow!("SVT-AV1 library not available"))
        }
    }

    /// Signal end-of-stream and drain remaining encoded packets.
    pub fn flush(&mut self) -> Result<Vec<Vec<u8>>> {
        #[cfg(svtav1_available)]
        {
            use std::ptr;

            // Send EOS flag.
            let mut in_hdr: ffi::EbBufferHeaderType = unsafe { std::mem::zeroed() };
            in_hdr.size = std::mem::size_of::<ffi::EbBufferHeaderType>() as u32;
            in_hdr.flags = ffi::EB_BUFFERFLAG_EOS;
            unsafe { ffi::svt_av1_enc_send_picture(self.handle, &mut in_hdr) };

            return self.drain_output();
        }

        #[cfg(not(svtav1_available))]
        Err(anyhow!("SVT-AV1 library not available"))
    }

    #[cfg(svtav1_available)]
    fn drain_output(&self) -> Result<Vec<Vec<u8>>> {
        let mut packets: Vec<Vec<u8>> = Vec::new();

        loop {
            let mut out_hdr: *mut ffi::EbBufferHeaderType = std::ptr::null_mut();
            let ret = unsafe { ffi::svt_av1_enc_get_packet(self.handle, &mut out_hdr, 0) };

            // No more output packets available right now.
            if ret != 0 || out_hdr.is_null() {
                break;
            }

            let hdr = unsafe { &*out_hdr };
            if hdr.n_filled_len > 0 && !hdr.p_buffer.is_null() {
                let slice = unsafe {
                    std::slice::from_raw_parts(hdr.p_buffer, hdr.n_filled_len as usize)
                };
                packets.push(slice.to_vec());
            }

            unsafe { ffi::svt_av1_enc_release_out_buffer(&mut out_hdr) };

            // EOS reached — no more packets will be produced.
            if hdr.flags & ffi::EB_BUFFERFLAG_EOS != 0 {
                break;
            }
        }

        Ok(packets)
    }
}

#[cfg(svtav1_available)]
impl Drop for SvtAv1Encoder {
    fn drop(&mut self) {
        unsafe {
            ffi::svt_av1_enc_deinit(self.handle);
            ffi::svt_av1_enc_deinit_handle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoder_new_fails_gracefully_without_library() {
        // On systems without libsvtav1 the constructor returns Err, never panics.
        let config = EncodeConfig {
            width: 1280,
            height: 720,
            fps: 30,
            bitrate_kbps: 2_500,
            svc_layers: 3,
            low_delay: true,
            preset: 10,
        };
        // We do not assert Ok/Err here because the outcome depends on whether
        // libsvtav1-dev is installed on the CI host. We only assert no panic.
        let _ = SvtAv1Encoder::new(config);
    }

    #[test]
    fn encode_config_fields_are_sane() {
        let config = EncodeConfig {
            width: 1920,
            height: 1080,
            fps: 60,
            bitrate_kbps: 5_500,
            svc_layers: 4,
            low_delay: true,
            preset: 12,
        };
        assert!(config.preset >= 9 && config.preset <= 12);
        assert!(config.svc_layers >= 1 && config.svc_layers <= 4);
        assert_eq!(config.width * config.height * 3 / 2, 1920 * 1080 * 3 / 2);
    }
}
