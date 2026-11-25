//! Nihongo Michi — desktop shell (Tauri 2). The UI is a Leptos (wasm) app;
//! this side owns the SQLite database, the tray, the reminder scheduler and
//! the reminder popup.

mod agenda;
mod aviso;
mod backup;
mod commands;
mod db;
mod events;
mod scheduler;
mod sound;
mod sync;
mod tray;

use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| events::show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("michi.db")).map_err(std::io::Error::other)?;
            sync::sync_events(&db.lock()).map_err(std::io::Error::other)?;
            app.manage(db);
            tray::build(app.handle())?;
            // Started with Windows (`--hidden`): stay in the tray.
            if !std::env::args().any(|a| a == "--hidden") {
                events::show_main(app.handle());
            }
            scheduler::start(app.handle());
            Ok(())
        })
        .on_window_event(|w, e| {
            // Closing hides: the app keeps reminding from the tray. "Sair" quits.
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::load_all,
            commands::doc_get,
            commands::save_docs,
            commands::save_cards,
            commands::backup_export,
            commands::backup_pick,
            commands::backup_apply,
            commands::reset_progress,
            commands::agenda_range,
            commands::occ_mark,
            commands::aviso_state,
            commands::aviso_fit,
            commands::aviso_action,
            commands::aviso_missed_ok,
            commands::aviso_test,
            commands::sound_test,
            commands::avisos_list,
            commands::avisos_unread,
            commands::avisos_read_all,
            commands::avisos_clear,
            commands::autostart_get,
            commands::autostart_set,
            commands::open_external,
            commands::app_info,
            commands::reveal_data_dir,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Nihongo Michi");
}
