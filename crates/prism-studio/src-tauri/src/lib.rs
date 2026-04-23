mod commands;
mod events;
mod onboarding;
pub mod rtmp_server;

use std::sync::Arc;
use tokio::sync::Mutex;

use commands::{app_handle, AppState, StreamState};
use onboarding::init_onboarding_state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().init();

    let state: AppState = Arc::new(Mutex::new(StreamState {
        stream_id: None,
        started_at: None,
        quality_preset: None,
        shutdown_tx: None,
    }));

    let onboarding = init_onboarding_state();

    app_handle(state, onboarding)
        .run(tauri::generate_context!())
        .expect("error while running Prism Studio");
}
