//! Commands for Ajustes → backups and sync between PCs.

use crate::autobackup;
use crate::backup::BackupInfo;
use crate::db::{err, Db, R};
use crate::keeper::{self, Keeper};
use crate::local;
use crate::pcsync::{self, Mode};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

fn poke(app: &AppHandle) {
    if let Some(k) = app.try_state::<Keeper>() {
        k.poke();
    }
}

/* ---------------- automatic backup ---------------- */

#[tauri::command]
pub fn backup_auto_status(app: AppHandle, db: State<'_, Db>) -> autobackup::Status {
    autobackup::status(&app, &db.lock())
}

#[tauri::command]
pub fn backup_auto_set(
    app: AppHandle,
    db: State<'_, Db>,
    on: Option<bool>,
    keep: Option<u32>,
    default_dir: Option<bool>,
) -> R<autobackup::Status> {
    let c = db.lock();
    local::update(&c, |l| {
        if let Some(on) = on {
            l.backup_off = !on;
        }
        if let Some(k) = keep {
            l.backup_keep = k;
        }
        if default_dir == Some(true) {
            l.backup_dir.clear();
        }
    })?;
    Ok(autobackup::status(&app, &c))
}

#[tauri::command]
pub async fn backup_auto_pick_dir(app: AppHandle, db: State<'_, Db>) -> R<Option<autobackup::Status>> {
    let Some(dir) = app.dialog().file().set_title("Pasta dos backups automáticos").blocking_pick_folder() else {
        return Ok(None);
    };
    let dir = dir.into_path().map_err(err)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("não consegui usar essa pasta: {e}"))?;
    let c = db.lock();
    local::update(&c, |l| l.backup_dir = dir.display().to_string())?;
    Ok(Some(autobackup::status(&app, &c)))
}

#[tauri::command]
pub async fn backup_auto_now(app: AppHandle, db: State<'_, Db>) -> R<autobackup::Status> {
    let c = db.lock();
    autobackup::backup_now(&app, &c)?;
    Ok(autobackup::status(&app, &c))
}

/// Restore confirmation for a backup picked from the list.
#[tauri::command]
pub fn backup_inspect(path: String) -> R<BackupInfo> {
    autobackup::inspect(&path)
}

/// Opens the backup folder or the sync folder in Explorer.
#[tauri::command]
pub fn open_folder(app: AppHandle, db: State<'_, Db>, which: String) -> R<()> {
    let dir: PathBuf = {
        let c = db.lock();
        let l = local::get(&c);
        match which.as_str() {
            "sync" if l.sync_on() => PathBuf::from(l.sync_dir.trim()),
            _ => autobackup::dir_of(&app, &l),
        }
    };
    std::fs::create_dir_all(&dir).map_err(err)?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(err)
}

/* ---------------- sync between PCs ---------------- */

#[tauri::command]
pub fn sync_status(app: AppHandle) -> pcsync::Status {
    pcsync::status(&app)
}

#[tauri::command]
pub async fn sync_pick_dir(app: AppHandle) -> R<Option<pcsync::Preview>> {
    let Some(dir) = app.dialog().file().set_title("Pasta compartilhada entre os PCs").blocking_pick_folder() else {
        return Ok(None);
    };
    let dir = dir.into_path().map_err(err)?;
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || pcsync::preview(&a, &dir).map(Some)).await.map_err(err)?
}

/// `how`: "merge", "adopt" or "mine" (see `pcsync::join`).
#[tauri::command]
pub async fn sync_join(app: AppHandle, dir: String, how: String) -> R<pcsync::Report> {
    let a = app.clone();
    let r =
        tauri::async_runtime::spawn_blocking(move || pcsync::join(&a, &PathBuf::from(dir), &how)).await.map_err(err)?;
    if matches!(&r, Ok(x) if x.received > 0) {
        keeper::after_apply(&app);
    }
    keeper::emit_status(&app);
    r
}

/// "Sincronizar agora" / "Atualizar": the UI saved everything first, so the
/// other PCs' changes can be applied right away.
#[tauri::command]
pub async fn sync_now(app: AppHandle) -> R<pcsync::Report> {
    let a = app.clone();
    let r =
        tauri::async_runtime::spawn_blocking(move || pcsync::sync_once(&a, true, Mode::Normal)).await.map_err(err)?;
    if matches!(&r, Ok(x) if x.received > 0) {
        keeper::after_apply(&app);
    }
    keeper::emit_status(&app);
    r
}

#[tauri::command]
pub fn sync_off(app: AppHandle) -> R<pcsync::Status> {
    pcsync::turn_off(&app)?;
    keeper::emit_status(&app);
    poke(&app);
    Ok(pcsync::status(&app))
}
