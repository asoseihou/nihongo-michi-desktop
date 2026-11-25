//! The app's own reminder popup: a small always-on-top window in the
//! bottom-right corner, drawn by the UI in the "departure board" style.
//! The backend only creates it and asks it to refresh; the UI reports its
//! height (`aviso_fit`) and the backend places and shows it.

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "aviso";
const WIDTH: f64 = 440.0;
const MARGIN: f64 = 14.0;

/// Makes sure the popup exists and tells it to reload its list.
///
/// Runs on its own thread: on Windows, building a webview from the main
/// thread (sync commands, tray/menu handlers) deadlocks (tauri#4121).
pub fn show(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        if handle.get_webview_window(LABEL).is_none() {
            let built = WebviewWindowBuilder::new(&handle, LABEL, WebviewUrl::App("index.html".into()))
                .title("Nihongo Michi · aviso")
                .inner_size(WIDTH, 180.0)
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .shadow(true)
                .build();
            if let Err(e) = built {
                log::warn!("aviso: {e}");
            }
            // A fresh window loads its list by itself on mount.
            return;
        }
        let _ = handle.emit_to(LABEL, crate::events::AVISOS, ());
    });
}

/// Called by the popup after drawing: `height` in CSS pixels, 0 = nothing to show.
pub fn fit(app: &AppHandle, height: f64) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    if height <= 0.0 {
        let _ = w.hide();
        return;
    }
    // Always the primary monitor's corner (the hidden popup may "belong" to
    // any screen of a multi-monitor desk).
    let monitor = app.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten());
    let Some(m) = monitor else {
        let _ = w.show();
        return;
    };
    let sf = m.scale_factor();
    let (pw, ph) = ((WIDTH * sf).round() as u32, (height.min(560.0) * sf).round() as u32);
    let area = m.work_area();
    let x = area.position.x + area.size.width as i32 - pw as i32 - (MARGIN * sf) as i32;
    let y = area.position.y + area.size.height as i32 - ph as i32 - (MARGIN * sf) as i32;
    let _ = w.set_size(PhysicalSize::new(pw, ph));
    let _ = w.set_position(PhysicalPosition::new(x, y));
    let _ = w.set_always_on_top(true);
    let _ = w.show();
}
