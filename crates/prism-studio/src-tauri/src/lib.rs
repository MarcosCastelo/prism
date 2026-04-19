mod commands;
mod events;

use std::sync::Arc;
use tokio::sync::Mutex;

use commands::{app_handle, AppState, StreamState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().init();

    let state: AppState = Arc::new(Mutex::new(StreamState {
        stream_id: None,
        started_at: None,
        quality_preset: None,
        shutdown_tx: None,
    }));

    app_handle(state)
        .run(tauri::generate_context!())
        .expect("error while running Prism Studio");
}
