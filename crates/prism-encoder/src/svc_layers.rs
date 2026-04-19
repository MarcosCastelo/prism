//! SVC spatial layer definitions and layer-stripping helpers.
//!
//! Prism uses up to 4 spatial layers inside a single fMP4/AV1 bitstream:
//!
//! | Index | Name | Resolution | FPS | Extra bitrate |
//! |-------|------|-----------|-----|---------------|
//! |   0   | L0   | 360p      |  30 | 400 kbps (base, always present) |
//! |   1   | L1   | 480p      |  30 | +600 kbps |
//! |   2   | L2   | 720p      |  60 | +1 500 kbps |
//! |   3   | L3   | 1080p     |  60 | +3 000 kbps |

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};

/// Bandwidth budget for one SVC layer (cumulative up to and including this layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvcLayer {
    /// Zero-based layer index (0 = L0 base, 3 = L3 highest).
    pub index: u8,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Additional bitrate contributed by this layer (kbps).
    pub incremental_bitrate_kbps: u32,
}

/// Static configuration table — one entry per layer, in ascending quality order.
pub const LAYER_CONFIGS: [SvcLayer; 4] = [
    SvcLayer { index: 0, width: 640,  height: 360,  fps: 30, incremental_bitrate_kbps: 400   },
    SvcLayer { index: 1, width: 854,  height: 480,  fps: 30, incremental_bitrate_kbps: 600   },
    SvcLayer { index: 2, width: 1280, height: 720,  fps: 60, incremental_bitrate_kbps: 1_500 },
    SvcLayer { index: 3, width: 1920, height: 1080, fps: 60, incremental_bitrate_kbps: 3_000 },
];

/// Human-readable quality preset → number of layers.
pub struct LayerConfig {
    pub n_layers: u8,
}

impl LayerConfig {
    /// Parse a quality preset string into a `LayerConfig`.
    ///
    /// Recognised presets: `"low"` (L0 only), `"medium"` (L0–L1),
    /// `"high"` (L0–L2), `"ultra"` (L0–L3).
    pub fn from_preset(preset: &str) -> Result<Self> {
        let n_layers = match preset {
            "low"    => 1,
            "medium" => 2,
            "high"   => 3,
            "ultra"  => 4,
            other    => {
                return Err(anyhow!(
                    "unknown quality preset '{}'; valid values: low, medium, high, ultra",
                    other
                ))
            }
        };
        Ok(Self { n_layers })
    }

    /// Total target bitrate for all active layers (kbps).
    pub fn total_bitrate_kbps(&self) -> u32 {
        LAYER_CONFIGS
            .iter()
            .take(self.n_layers as usize)
            .map(|l| l.incremental_bitrate_kbps)
            .sum()
    }

    /// Active layer descriptors (slice up to `n_layers`).
    pub fn active_layers(&self) -> &[SvcLayer] {
        &LAYER_CONFIGS[..self.n_layers as usize]
    }
}

/// Strip all spatial layers above `max_layer` from an fMP4 payload.
///
/// Layer stripping is a bitwise operation on the fMP4 bitstream: OBU
/// (Open Bitstream Unit) NAL units for layers > `max_layer` are removed.
/// The operation does not require decoding.
///
/// Returns the stripped payload bytes and a `Vec` of per-layer SHA-256 hashes
/// for layers 0..=`max_layer`.
///
/// **Important:** The stripped payload for layer `i` must hash to
/// `VideoChunk.layer_hashes[i].payload_hash` to detect tampering.
pub fn strip_to_layer(payload: &[u8], max_layer: u8) -> Result<(Vec<u8>, Vec<[u8; 32]>)> {
    if max_layer > 3 {
        return Err(anyhow!("max_layer must be 0–3, got {max_layer}"));
    }
    if payload.is_empty() {
        return Err(anyhow!("payload is empty"));
    }

    // Walk the fMP4 box structure to locate `mdat` box content, then
    // parse OBU headers and filter by spatial_id.
    //
    // This implementation performs a best-effort parse of the top-level
    // ISO BMFF box structure to locate the mdat box, then applies OBU
    // spatial-layer filtering on the raw AV1 bitstream within it.
    let stripped = strip_obu_layers(payload, max_layer)?;

    // Compute per-layer hashes (hash of the payload stripped to exactly layer i).
    let mut layer_hashes: Vec<[u8; 32]> = Vec::with_capacity((max_layer + 1) as usize);
    for layer in 0..=max_layer {
        let layer_payload = if layer == max_layer {
            stripped.clone()
        } else {
            strip_obu_layers(payload, layer)?
        };
        let hash: [u8; 32] = Sha256::digest(&layer_payload).into();
        layer_hashes.push(hash);
    }

    Ok((stripped, layer_hashes))
}

/// Compute per-layer SHA-256 hashes for a full (un-stripped) chunk payload.
///
/// Called by the injector when building `VideoChunk.layer_hashes`.
pub fn compute_layer_hashes(payload: &[u8], n_layers: u8) -> Result<Vec<[u8; 32]>> {
    if n_layers == 0 || n_layers > 4 {
        return Err(anyhow!("n_layers must be 1–4, got {n_layers}"));
    }
    let mut hashes = Vec::with_capacity(n_layers as usize);
    for layer in 0..n_layers {
        let (stripped, _) = strip_to_layer(payload, layer)?;
        let hash: [u8; 32] = Sha256::digest(&stripped).into();
        hashes.push(hash);
    }
    Ok(hashes)
}

/// Verify that a stripped payload matches the expected layer hash from a `VideoChunk`.
///
/// Returns `Ok(())` on match, `Err` on mismatch (tampering detected).
pub fn verify_layer_hash(stripped_payload: &[u8], expected_hash: &[u8]) -> Result<()> {
    let actual: [u8; 32] = Sha256::digest(stripped_payload).into();
    if actual.as_ref() == expected_hash {
        Ok(())
    } else {
        Err(anyhow!("layer hash mismatch: stripped payload does not match expected hash"))
    }
}

// ---------------------------------------------------------------------------
// Internal OBU parsing helpers
// ---------------------------------------------------------------------------

/// Strip OBU NAL units for spatial layers above `max_layer` from the payload.
///
/// Parses the fMP4 box structure, locates `mdat`, filters AV1 OBU temporal
/// units by `spatial_id`, and reconstructs the box with filtered content.
fn strip_obu_layers(payload: &[u8], max_layer: u8) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(payload.len());
    let mut pos = 0usize;

    while pos + 8 <= payload.len() {
        let box_size = u32::from_be_bytes(payload[pos..pos + 4].try_into().unwrap()) as usize;
        let box_type = &payload[pos + 4..pos + 8];

        if box_size < 8 {
            // Minimal box is 8 bytes (size + type).
            return Err(anyhow!("malformed fMP4: box size {box_size} < 8 at offset {pos}"));
        }
        if pos + box_size > payload.len() {
            return Err(anyhow!("malformed fMP4: box extends past end of payload"));
        }

        if box_type == b"mdat" {
            // Filter the raw AV1 bitstream within mdat.
            let mdat_content = &payload[pos + 8..pos + box_size];
            let filtered = filter_obu_by_spatial_id(mdat_content, max_layer)?;
            let new_box_size = (8 + filtered.len()) as u32;
            out.extend_from_slice(&new_box_size.to_be_bytes());
            out.extend_from_slice(b"mdat");
            out.extend_from_slice(&filtered);
        } else {
            // Copy all other boxes (moov, moof, etc.) verbatim.
            out.extend_from_slice(&payload[pos..pos + box_size]);
        }

        pos += box_size;
    }

    // Append any trailing bytes (unusual but tolerate).
    if pos < payload.len() {
        out.extend_from_slice(&payload[pos..]);
    }

    Ok(out)
}

/// Filter AV1 OBU bitstream, keeping only OBUs belonging to spatial layers ≤ `max_layer`.
///
/// AV1 OBU header format (simplified):
///   - bit 7: forbidden_bit (must be 0)
///   - bits 6–4: obu_type (3 bits)
///   - bit 3: obu_extension_flag
///   - bit 2: obu_has_size_field
///   - bit 1: reserved
///
/// When `obu_extension_flag` is set, the next byte carries:
///   - bits 7–5: temporal_id
///   - bits 4–3: spatial_id
///   - bits 2–0: quality_id / reserved
fn filter_obu_by_spatial_id(data: &[u8], max_layer: u8) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(data.len());
    let mut pos = 0usize;

    while pos < data.len() {
        let header_byte = data[pos];

        // forbidden_bit must be 0.
        if header_byte & 0x80 != 0 {
            return Err(anyhow!("invalid OBU header at offset {pos}: forbidden_bit set"));
        }

        let obu_type = (header_byte >> 4) & 0x07;
        let extension_flag = (header_byte >> 3) & 0x01 != 0;
        let has_size_field = (header_byte >> 2) & 0x01 != 0;

        let header_size = if extension_flag { 2 } else { 1 };

        if pos + header_size > data.len() {
            break;
        }

        let spatial_id = if extension_flag {
            (data[pos + 1] >> 3) & 0x03
        } else {
            // No extension — OBU applies to all layers (e.g. sequence header).
            0
        };

        // Determine OBU payload size.
        let obu_payload_size = if has_size_field {
            let (sz, sz_bytes) = read_leb128(&data[pos + header_size..])?;
            // OBU length includes the size bytes.
            let obu_total = header_size + sz_bytes + sz as usize;
            if pos + obu_total > data.len() {
                break;
            }
            obu_total
        } else {
            // No size field — OBU extends to end of data.
            data.len() - pos
        };

        let _ = obu_type; // keep for future per-type logic

        // Include this OBU only if it belongs to a layer ≤ max_layer,
        // or if it has no extension (applies globally, e.g. sequence header).
        if !extension_flag || spatial_id <= max_layer {
            out.extend_from_slice(&data[pos..pos + obu_payload_size]);
        }

        pos += obu_payload_size;
    }

    Ok(out)
}

/// Decode an unsigned LEB128 integer.  Returns `(value, bytes_consumed)`.
fn read_leb128(data: &[u8]) -> Result<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut i = 0usize;

    loop {
        if i >= data.len() {
            return Err(anyhow!("truncated LEB128 at byte {i}"));
        }
        let byte = data[i];
        i += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err(anyhow!("LEB128 overflow"));
        }
    }

    Ok((value, i))
}

// ---------------------------------------------------------------------------
// sha2 re-export dependency — added to Cargo.toml below
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_config_preset_parsing() {
        assert_eq!(LayerConfig::from_preset("low").unwrap().n_layers, 1);
        assert_eq!(LayerConfig::from_preset("medium").unwrap().n_layers, 2);
        assert_eq!(LayerConfig::from_preset("high").unwrap().n_layers, 3);
        assert_eq!(LayerConfig::from_preset("ultra").unwrap().n_layers, 4);
        assert!(LayerConfig::from_preset("unknown").is_err());
    }

    #[test]
    fn layer_config_total_bitrate() {
        // low: L0 only → 400 kbps
        assert_eq!(LayerConfig::from_preset("low").unwrap().total_bitrate_kbps(), 400);
        // ultra: L0+L1+L2+L3 → 400+600+1500+3000 = 5500 kbps
        assert_eq!(LayerConfig::from_preset("ultra").unwrap().total_bitrate_kbps(), 5_500);
    }

    #[test]
    fn layer_config_active_layers_count() {
        let cfg = LayerConfig::from_preset("high").unwrap();
        assert_eq!(cfg.active_layers().len(), 3);
        assert_eq!(cfg.active_layers()[2].index, 2);
    }

    #[test]
    fn layer_constants_are_ordered() {
        for (i, layer) in LAYER_CONFIGS.iter().enumerate() {
            assert_eq!(layer.index as usize, i);
        }
    }

    #[test]
    fn strip_to_layer_invalid_max_layer() {
        let dummy_payload = vec![0u8; 64];
        assert!(strip_to_layer(&dummy_payload, 4).is_err());
    }

    #[test]
    fn strip_to_layer_empty_payload() {
        assert!(strip_to_layer(&[], 0).is_err());
    }

    #[test]
    fn verify_layer_hash_correct() {
        let data = b"test payload";
        let hash: [u8; 32] = sha2::Sha256::digest(data).into();
        assert!(verify_layer_hash(data, &hash).is_ok());
    }

    #[test]
    fn verify_layer_hash_mismatch() {
        let data = b"test payload";
        let wrong_hash = [0u8; 32];
        assert!(verify_layer_hash(data, &wrong_hash).is_err());
    }

    #[test]
    fn leb128_decodes_single_byte() {
        let (val, n) = read_leb128(&[0x05]).unwrap();
        assert_eq!(val, 5);
        assert_eq!(n, 1);
    }

    #[test]
    fn leb128_decodes_multi_byte() {
        // 300 in LEB128 = [0xAC, 0x02]
        let (val, n) = read_leb128(&[0xAC, 0x02]).unwrap();
        assert_eq!(val, 300);
        assert_eq!(n, 2);
    }
}
