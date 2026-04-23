//! Five-step onboarding flow for first-time streamers.
//!
//! Steps:
//!   1 – Identity:   generate Ed25519 keypair and display pubkey
//!   2 – Backup:     export encrypted backup of the key file
//!   3 – Benchmark:  measure upload bandwidth to suggest quality preset
//!   4 – Source:     select camera or OBS/RTMP as video source
//!   5 – Go Live:    mark onboarding complete, user is ready to stream

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::State;
use tokio::sync::Mutex;

use prism_core::{hash::sha256, Identity};

const STATUS_FILE: &str = "prism_onboarding.json";
const KEY_FILE: &str = "prism_studio.key";

// ─────────────────────────────────────────────────────────────────────────────
// State types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingStatus {
    /// Current step (1–5). 6 = onboarding complete.
    pub current_step: u8,
    pub identity_pubkey: Option<String>,
    pub benchmark_result: Option<String>,
    pub source_type: Option<String>,
    pub completed: bool,
}

impl Default for OnboardingStatus {
    fn default() -> Self {
        Self {
            current_step: 1,
            identity_pubkey: None,
            benchmark_result: None,
            source_type: None,
            completed: false,
        }
    }
}

pub type OnboardingState = Arc<Mutex<OnboardingStatus>>;

pub fn init_onboarding_state() -> OnboardingState {
    Arc::new(Mutex::new(load_status()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Persistence helpers
// ─────────────────────────────────────────────────────────────────────────────

fn load_status() -> OnboardingStatus {
    let path = PathBuf::from(STATUS_FILE);
    if path.exists() {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(s) = serde_json::from_slice::<OnboardingStatus>(&bytes) {
                return s;
            }
        }
    }
    OnboardingStatus::default()
}

fn save_status(status: &OnboardingStatus) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(status)?;
    std::fs::write(STATUS_FILE, json)?;
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Commands
// ─────────────────────────────────────────────────────────────────────────────

/// Returns current onboarding status.
#[tauri::command]
pub async fn onboarding_status(
    state: State<'_, OnboardingState>,
) -> Result<OnboardingStatus, String> {
    Ok(state.lock().await.clone())
}

/// Step 1: generate (or load) Ed25519 identity and return the user-facing display string.
///
/// Returns a string like "pr1a3f7...b92e" derived from the first/last 4 hex chars of
/// the verifying key.
#[tauri::command]
pub async fn onboarding_generate_identity(
    state: State<'_, OnboardingState>,
) -> Result<String, String> {
    let key_path = PathBuf::from(KEY_FILE);
    let identity = if key_path.exists() {
        Identity::load(&key_path).map_err(|e| e.to_string())?
    } else {
        let id = Identity::generate();
        id.save(&key_path).map_err(|e| e.to_string())?;
        id
    };

    let pubkey_hex = hex::encode(identity.verifying_key.as_bytes());
    let pubkey_display = format!(
        "pr1{}...{}",
        &pubkey_hex[..4],
        &pubkey_hex[pubkey_hex.len() - 4..]
    );

    let mut s = state.lock().await;
    s.identity_pubkey = Some(pubkey_display.clone());
    if s.current_step < 2 {
        s.current_step = 2;
    }
    save_status(&s).map_err(|e| e.to_string())?;

    tracing::info!(pubkey = pubkey_display, "identity ready");
    Ok(pubkey_display)
}

/// Step 2: export an encrypted backup of the key file.
///
/// The key bytes are XOR-masked with sha256(password) and written to
/// `prism_studio_backup.json` alongside the verifying key for verification.
/// Returns the backup file path.
#[tauri::command]
pub async fn onboarding_export_backup(
    password: String,
    state: State<'_, OnboardingState>,
) -> Result<String, String> {
    let key_path = PathBuf::from(KEY_FILE);
    let key_bytes = std::fs::read(&key_path)
        .map_err(|e| format!("cannot read key file: {e}"))?;
    if key_bytes.len() != 32 {
        return Err("key file has unexpected size".to_string());
    }

    let mask = sha256(password.as_bytes());
    let encrypted: Vec<u8> = key_bytes
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i])
        .collect();

    let identity = Identity::load(&key_path).map_err(|e| e.to_string())?;
    let pubkey_hex = hex::encode(identity.verifying_key.as_bytes());

    let backup = serde_json::json!({
        "version": 1,
        "pubkey": pubkey_hex,
        "encrypted_key": hex::encode(&encrypted),
        "checksum": hex::encode(&sha256(&encrypted)[..4]),
    });

    let backup_path = "prism_studio_backup.json";
    std::fs::write(backup_path, serde_json::to_string_pretty(&backup).unwrap())
        .map_err(|e| format!("cannot write backup: {e}"))?;

    let mut s = state.lock().await;
    if s.current_step < 3 {
        s.current_step = 3;
    }
    save_status(&s).map_err(|e| e.to_string())?;

    tracing::info!(path = backup_path, "identity backup exported");
    Ok(backup_path.to_string())
}

/// Step 3: measure upload bandwidth and suggest a quality preset.
///
/// Simulates a benchmark by timing how quickly the system can hash a
/// 10 MB buffer (CPU proxy) and reports a suggested quality tier.
/// Returns a human-readable result like "1080p60" or "720p".
#[tauri::command]
pub async fn onboarding_run_benchmark(
    state: State<'_, OnboardingState>,
) -> Result<String, String> {
    let result = tokio::task::spawn_blocking(|| {
        let buf = vec![0xABu8; 10 * 1024 * 1024]; // 10 MB
        let start = Instant::now();
        let _ = sha256(&buf);
        let elapsed_ms = start.elapsed().as_millis();

        // Map hashing latency to a quality tier (proxy for system throughput).
        match elapsed_ms {
            0..=30  => "1080p60",
            31..=60 => "1080p30",
            61..=120 => "720p",
            _ => "480p",
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    tracing::info!(quality = result, "benchmark complete");

    let mut s = state.lock().await;
    s.benchmark_result = Some(result.to_string());
    if s.current_step < 4 {
        s.current_step = 4;
    }
    save_status(&s).map_err(|e| e.to_string())?;

    Ok(result.to_string())
}

/// Step 4: record the chosen video source ("camera" or "rtmp").
#[tauri::command]
pub async fn onboarding_set_source(
    source_type: String,
    state: State<'_, OnboardingState>,
) -> Result<(), String> {
    if source_type != "camera" && source_type != "rtmp" {
        return Err(format!("unknown source type: {source_type}"));
    }

    let mut s = state.lock().await;
    s.source_type = Some(source_type.clone());
    if s.current_step < 5 {
        s.current_step = 5;
    }
    save_status(&s).map_err(|e| e.to_string())?;

    tracing::info!(source = source_type, "video source configured");
    Ok(())
}

/// Step 5: mark onboarding as complete.
#[tauri::command]
pub async fn onboarding_complete(
    state: State<'_, OnboardingState>,
) -> Result<(), String> {
    let mut s = state.lock().await;
    s.completed = true;
    s.current_step = 6;
    save_status(&s).map_err(|e| e.to_string())?;

    tracing::info!("onboarding complete");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_status_starts_at_step_1() {
        let s = OnboardingStatus::default();
        assert_eq!(s.current_step, 1);
        assert!(!s.completed);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let status = OnboardingStatus {
            current_step: 3,
            identity_pubkey: Some("pr1abcd...ef12".into()),
            benchmark_result: Some("720p".into()),
            source_type: None,
            completed: false,
        };
        let json = serde_json::to_string_pretty(&status).unwrap();
        let loaded: OnboardingStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.current_step, 3);
        assert_eq!(loaded.benchmark_result.as_deref(), Some("720p"));
    }
}
