//! Nihongo Michi UI — Leptos (CSR) compiled to wasm32 by Trunk and shown
//! by Tauri. The same bundle draws the main window and the reminder popup.

// Helpers ported ahead of the phase-2 screens are not all used yet.
#![allow(dead_code)]

mod app;
mod aviso;
mod data;
mod docs;
mod jp;
mod logic;
mod store;
mod tauri;
mod tts;
mod util;
mod views;

fn main() {
    console_error_panic_hook::set_once();
    if tauri::window_label() == "aviso" {
        leptos::mount::mount_to_body(aviso::AvisoApp);
    } else {
        leptos::mount::mount_to_body(app::App);
    }
}
