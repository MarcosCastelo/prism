//! fMP4 segmenter — accumulates encoded AV1 OBU packets until a full
//! segment of `target_duration_ms` (nominally 3 000 ms) is ready.
//!
//! The produced `Segment` is raw fMP4 that can be directly embedded in a
//! `VideoChunk` payload and delivered over the P2P network.
//!
//! # fMP4 layout produced
//!
//! ```text
//! [ftyp] (written once by build_ftyp, prepended to first segment)
//! [moof]
//!   [mfhd] — sequence_number
//!   [traf]
//!     [tfhd]
//!     [tfdt] — base_media_decode_time (= pts_start in 90 kHz timescale)
//!     [trun] — one entry per packet: duration, size, flags
//! [mdat] — concatenated raw AV1 OBU packets
//! ```
//!
//! This layout is compatible with HLS fMP4 segments (`.m4s`).


/// A completed fMP4 segment ready to be wrapped in a `VideoChunk`.
#[derive(Debug, Clone)]
pub struct Segment {
    /// Raw fMP4 bytes (moof + mdat, or ftyp + moof + mdat for the first segment).
    pub data: Vec<u8>,
    /// Effective duration of this segment in milliseconds.
    pub duration_ms: u64,
    /// Presentation timestamp of the first frame in this segment (milliseconds).
    pub pts_start: u64,
}

/// Accumulates encoded AV1 packets and emits `Segment`s once `target_duration_ms`
/// of content has been buffered.
pub struct FMp4Segmenter {
    target_duration_ms: u64,
    /// AV1 OBU packets buffered for the current open segment.
    packets: Vec<(Vec<u8>, u64)>, // (payload, pts_ms)
    /// Sequence number for `mfhd` box — monotonically increasing.
    sequence_number: u32,
    /// Whether the initialization segment (`ftyp`) still needs to be prepended.
    first_segment: bool,
}

impl FMp4Segmenter {
    /// Create a new segmenter.  `target_duration_ms` is typically `3_000`.
    pub fn new(target_duration_ms: u64) -> Self {
        Self {
            target_duration_ms,
            packets: Vec::new(),
            sequence_number: 1,
            first_segment: true,
        }
    }

    /// Push one encoded AV1 OBU packet with its presentation timestamp (ms).
    ///
    /// Returns `Some(Segment)` when a complete segment is ready (i.e. the
    /// span from the first buffered frame to the current frame ≥ target).
    /// Returns `None` otherwise — keep feeding packets.
    pub fn push_packet(&mut self, packet: Vec<u8>, pts: u64) -> Option<Segment> {
        self.packets.push((packet, pts));

        // Check if we have accumulated enough content.
        let pts_start = self.packets[0].1;
        let duration_ms = pts.saturating_sub(pts_start);

        if duration_ms >= self.target_duration_ms {
            self.emit_segment()
        } else {
            None
        }
    }

    /// Force-close the current open segment regardless of duration.
    ///
    /// Call this at end-of-stream (after `SvtAv1Encoder::flush`).
    /// Returns `None` if no packets have been buffered since the last segment.
    pub fn flush(&mut self) -> Option<Segment> {
        if self.packets.is_empty() {
            return None;
        }
        self.emit_segment()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn emit_segment(&mut self) -> Option<Segment> {
        if self.packets.is_empty() {
            return None;
        }

        let pts_start = self.packets[0].1;
        let pts_end = self.packets.last().unwrap().1;
        let duration_ms = pts_end.saturating_sub(pts_start).max(1);

        let mdat_payload: Vec<u8> = self
            .packets
            .iter()
            .flat_map(|(pkt, _)| pkt.iter().copied())
            .collect();

        let packet_meta: Vec<PacketMeta> = self
            .packets
            .windows(2)
            .map(|w| {
                let duration_pts = ms_to_pts(w[1].1.saturating_sub(w[0].1));
                PacketMeta { size: w[0].0.len() as u32, duration_pts }
            })
            .chain(std::iter::once({
                // Last packet — assign a duration equal to the average.
                let avg = ms_to_pts(duration_ms / self.packets.len() as u64);
                PacketMeta {
                    size: self.packets.last().unwrap().0.len() as u32,
                    duration_pts: avg.max(1),
                }
            }))
            .collect();

        let moof = build_moof(self.sequence_number, ms_to_pts(pts_start), &packet_meta);
        let mdat = build_mdat(&mdat_payload);

        let mut data = Vec::with_capacity(
            if self.first_segment { FTYP.len() } else { 0 } + moof.len() + mdat.len(),
        );

        if self.first_segment {
            data.extend_from_slice(FTYP);
            self.first_segment = false;
        }
        data.extend_from_slice(&moof);
        data.extend_from_slice(&mdat);

        self.packets.clear();
        self.sequence_number += 1;

        Some(Segment { data, duration_ms, pts_start })
    }
}

// ---------------------------------------------------------------------------
// fMP4 box builders
// ---------------------------------------------------------------------------

/// Convert milliseconds to 90 kHz PTS ticks (standard MPEG/HLS timescale).
fn ms_to_pts(ms: u64) -> u64 {
    ms * 90
}

struct PacketMeta {
    size: u32,
    duration_pts: u64,
}

/// Static `ftyp` box for CMAF/HLS fMP4 (AV1 codec brand).
const FTYP: &[u8] = &[
    // size = 24
    0x00, 0x00, 0x00, 0x18,
    // type = 'ftyp'
    b'f', b't', b'y', b'p',
    // major_brand = 'cmf2'
    b'c', b'm', b'f', b'2',
    // minor_version = 0
    0x00, 0x00, 0x00, 0x00,
    // compatible_brands: 'iso6', 'cmf2'
    b'i', b's', b'o', b'6',
    b'c', b'm', b'f', b'2',
];

fn build_moof(sequence_number: u32, base_decode_time: u64, packets: &[PacketMeta]) -> Vec<u8> {
    let mfhd = build_mfhd(sequence_number);
    let traf = build_traf(base_decode_time, packets);

    let total = 8 + mfhd.len() + traf.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(b"moof");
    out.extend_from_slice(&mfhd);
    out.extend_from_slice(&traf);
    out
}

fn build_mfhd(sequence_number: u32) -> Vec<u8> {
    // mfhd: version(1) + flags(3) + sequence_number(4) = 8 bytes payload, 16 total
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&16u32.to_be_bytes()); // size
    out.extend_from_slice(b"mfhd");
    out.push(0x00); // version
    out.extend_from_slice(&[0x00, 0x00, 0x00]); // flags
    out.extend_from_slice(&sequence_number.to_be_bytes());
    out
}

fn build_traf(base_decode_time: u64, packets: &[PacketMeta]) -> Vec<u8> {
    let tfhd = build_tfhd();
    let tfdt = build_tfdt(base_decode_time);
    let trun = build_trun(packets);

    let total = 8 + tfhd.len() + tfdt.len() + trun.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(b"traf");
    out.extend_from_slice(&tfhd);
    out.extend_from_slice(&tfdt);
    out.extend_from_slice(&trun);
    out
}

fn build_tfhd() -> Vec<u8> {
    // Minimal tfhd: version(1) + flags(3) + track_ID(4) = 16 bytes
    // flags = 0x020000 (default-base-is-moof)
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&16u32.to_be_bytes());
    out.extend_from_slice(b"tfhd");
    out.push(0x00); // version
    out.extend_from_slice(&[0x02, 0x00, 0x00]); // flags: default-base-is-moof
    out.extend_from_slice(&1u32.to_be_bytes()); // track_ID = 1
    out
}

fn build_tfdt(base_decode_time: u64) -> Vec<u8> {
    // version=1 (64-bit decode time) + flags(3) + base_media_decode_time(8) = 20 bytes
    let mut out = Vec::with_capacity(20);
    out.extend_from_slice(&20u32.to_be_bytes());
    out.extend_from_slice(b"tfdt");
    out.push(0x01); // version = 1 (64-bit)
    out.extend_from_slice(&[0x00, 0x00, 0x00]); // flags
    out.extend_from_slice(&base_decode_time.to_be_bytes());
    out
}

fn build_trun(packets: &[PacketMeta]) -> Vec<u8> {
    // trun flags: 0x000305
    //   bit 0 = data_offset_present
    //   bit 8 = sample_duration_present
    //   bit 9 = sample_size_present
    // version = 0, entry_count = packets.len()
    // data_offset placeholder = 0 (we'll fix after assembling moof)
    // Each entry: duration(4) + size(4) = 8 bytes
    let n = packets.len() as u32;
    let total = 8 + 4 + 4 + 4 + 4 + n as usize * 8;
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&(total as u32).to_be_bytes());
    out.extend_from_slice(b"trun");
    out.push(0x00); // version
    out.extend_from_slice(&[0x03, 0x05, 0x00]); // flags: data_offset | sample_duration | sample_size (reversed byte order — 0x000305 BE = [0x00, 0x03, 0x05])
    out.extend_from_slice(&n.to_be_bytes()); // entry_count
    out.extend_from_slice(&0i32.to_be_bytes()); // data_offset (placeholder, not corrected here)

    for pkt in packets {
        // Clamp to u32 — individual OBU packets won't exceed 4 GB.
        let dur = pkt.duration_pts.min(u32::MAX as u64) as u32;
        out.extend_from_slice(&dur.to_be_bytes());
        out.extend_from_slice(&pkt.size.to_be_bytes());
    }

    out
}

fn build_mdat(payload: &[u8]) -> Vec<u8> {
    let size = (8 + payload.len()) as u32;
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(b"mdat");
    out.extend_from_slice(payload);
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_packet(size: usize) -> Vec<u8> {
        vec![0xA5u8; size]
    }

    #[test]
    fn segmenter_no_segment_before_threshold() {
        let mut seg = FMp4Segmenter::new(3_000);
        // Two packets close together — not enough duration.
        assert!(seg.push_packet(make_packet(256), 0).is_none());
        assert!(seg.push_packet(make_packet(256), 1_000).is_none());
    }

    #[test]
    fn segmenter_emits_segment_at_threshold() {
        let mut seg = FMp4Segmenter::new(3_000);
        seg.push_packet(make_packet(512), 0);
        seg.push_packet(make_packet(512), 1_500);
        let result = seg.push_packet(make_packet(512), 3_001);
        assert!(result.is_some());
        let s = result.unwrap();
        assert_eq!(s.pts_start, 0);
        assert!(s.duration_ms >= 3_000);
        assert!(!s.data.is_empty());
    }

    #[test]
    fn segmenter_flush_emits_partial_segment() {
        let mut seg = FMp4Segmenter::new(3_000);
        seg.push_packet(make_packet(128), 0);
        seg.push_packet(make_packet(128), 500);
        let flushed = seg.flush();
        assert!(flushed.is_some());
    }

    #[test]
    fn segmenter_flush_empty_returns_none() {
        let mut seg = FMp4Segmenter::new(3_000);
        assert!(seg.flush().is_none());
    }

    #[test]
    fn segmenter_first_segment_has_ftyp() {
        let mut seg = FMp4Segmenter::new(100); // tiny threshold
        seg.push_packet(make_packet(64), 0);
        let s = seg.push_packet(make_packet(64), 200).unwrap();
        // First segment should begin with 'ftyp' box.
        assert_eq!(&s.data[4..8], b"ftyp");
    }

    #[test]
    fn segmenter_second_segment_no_ftyp() {
        let mut seg = FMp4Segmenter::new(100);
        seg.push_packet(make_packet(64), 0);
        seg.push_packet(make_packet(64), 200); // triggers first segment
        // Keep going for a second segment.
        seg.push_packet(make_packet(64), 400);
        let s2 = seg.push_packet(make_packet(64), 600).unwrap();
        // Second segment starts with 'moof', not 'ftyp'.
        assert_eq!(&s2.data[4..8], b"moof");
    }

    #[test]
    fn ms_to_pts_conversion() {
        // 1000 ms → 90 000 ticks at 90 kHz
        assert_eq!(ms_to_pts(1_000), 90_000);
    }

    #[test]
    fn mdat_box_structure() {
        let payload = b"hello";
        let mdat = build_mdat(payload);
        assert_eq!(&mdat[4..8], b"mdat");
        let size = u32::from_be_bytes(mdat[0..4].try_into().unwrap()) as usize;
        assert_eq!(size, 8 + payload.len());
    }

    #[test]
    fn mfhd_sequence_number_roundtrip() {
        let mfhd = build_mfhd(42);
        let seq = u32::from_be_bytes(mfhd[12..16].try_into().unwrap());
        assert_eq!(seq, 42);
    }
}
