//! Nihongo Michi — desktop shell (Tauri 2). The UI is a Leptos (wasm) app;
//! this side owns the SQLite database, the tray, the reminder scheduler and
//! the reminder popup.

mod agenda;
mod agenda_cmd;
mod autobackup;
mod aviso;
mod backup;
mod commands;
mod datacmd;
mod db;
mod events;
mod files;
mod foco;
mod keeper;
mod local;
mod pcsync;
mod scheduler;
mod sound;
mod stt;
mod sync;
mod tray;
mod updater;

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| events::show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("michi.db")).map_err(std::io::Error::other)?;
            sync::sync_events(&db.lock()).map_err(std::io::Error::other)?;
            app.manage(db);
            app.manage(stt::Stt::default());
            app.manage(pcsync::SyncState::default());
            app.manage(datacmd::Picked::default());
            // Right after an update, the new version says what changed; one
            // installed while the window was closed starts in the tray.
            let just = updater::take_marker(app.handle());
            let hidden = just.as_ref().map(|j| j.hidden).unwrap_or(false);
            updater::init(app.handle(), just);
            tray::build(app.handle())?;
            // Started with Windows (`--hidden`): stay in the tray.
            if !hidden && !std::env::args().any(|a| a == "--hidden") {
                events::show_main(app.handle());
            }
            // First sync, then the reminder scheduler, then backups and sync
            // in the background.
            keeper::start(app.handle());
            foco::start_timer(app.handle());
            Ok(())
        })
        .on_window_event(|w, e| {
            // Closing hides: the app keeps reminding from the tray. "Sair" quits.
            if let WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = w.hide();
                if w.label() == "main" {
                    if let Some(k) = w.app_handle().try_state::<keeper::Keeper>() {
                        k.hidden();
                    }
                }
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
            agenda_cmd::event_get,
            agenda_cmd::event_save,
            agenda_cmd::event_delete,
            agenda_cmd::occ_skip,
            agenda_cmd::occ_move,
            agenda_cmd::agenda_export_ics,
            datacmd::backup_auto_status,
            datacmd::backup_auto_set,
            datacmd::backup_auto_pick_dir,
            datacmd::backup_auto_now,
            datacmd::backup_inspect,
            datacmd::open_folder,
            datacmd::sync_status,
            datacmd::sync_pick_dir,
            datacmd::sync_join,
            datacmd::sync_now,
            datacmd::sync_off,
            datacmd::update_status,
            datacmd::update_check,
            datacmd::update_install,
            datacmd::update_set,
            datacmd::update_seen,
            foco::foco_state,
            foco::foco_start,
            foco::foco_pause,
            foco::foco_resume,
            foco::foco_add,
            foco::foco_stop,
            foco::foco_mark,
            foco::foco_close,
            foco::foco_show,
            foco::foco_fit,
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
            files::save_text,
            files::open_text,
            files::anki_connect,
            stt::stt_status,
            stt::stt_start,
            stt::stt_stop,
            stt::stt_cancel,
            stt::stt_download,
        ])
        .build(tauri::generate_context!())
        .expect("erro ao iniciar o Nihongo Michi")
        .run(|app, ev| {
            if let RunEvent::Exit = ev {
                // Leave this PC's latest progress in the shared folder.
                if let Err(e) = pcsync::push_only(app) {
                    log::warn!("sync ao sair: {e}");
                }
            }
        });
}
