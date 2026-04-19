// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;

use std::sync::Arc;
use tokio::sync::Mutex;

use commands::{app_handle, AppState, StreamState};

fn main() {
    tracing_subscriber::fmt().init();

    let state: AppState = Arc::new(Mutex::new(StreamState {
        stream_id: None,
        started_at: None,
        quality_preset: None,
    }));

    app_handle(state)
        .run(tauri::generate_context!())
        .expect("error while running Prism Studio");
}
