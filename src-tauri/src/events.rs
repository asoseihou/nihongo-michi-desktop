//! Events sent from the backend to the windows.

use michi_core::ops::Op;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub const PATCH: &str = "michi://patch";
pub const AVISOS: &str = "michi://avisos";
pub const NAVIGATE: &str = "michi://navigate";
/// The `desk` document changed outside the UI (tray pause).
pub const RELOAD_DESK: &str = "michi://reload-desk";

#[derive(Clone, Serialize)]
pub struct Navigate {
    pub route: String,
    /// Routine block that asked for it (the UI picks the same intent as a
    /// click on the Today board).
    pub block: Option<String>,
}

/// The main window replays these on its in-memory documents.
pub fn patch(app: &AppHandle, ops: &[Op]) {
    if !ops.is_empty() {
        let _ = app.emit_to("main", PATCH, ops);
    }
}

pub fn avisos_changed(app: &AppHandle) {
    let _ = app.emit(AVISOS, ());
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn navigate(app: &AppHandle, route: &str, block: Option<String>) {
    show_main(app);
    let _ = app.emit_to("main", NAVIGATE, Navigate { route: route.to_string(), block });
}
