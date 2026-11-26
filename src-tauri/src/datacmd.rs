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

/// Paths the learner picked in a file dialog during this run. Commands that
/// take a path from the window only accept these (or the app's own backup
/// folders), so a page script could never point them anywhere else.
#[derive(Default)]
pub struct Picked(std::sync::Mutex<std::collections::HashSet<PathBuf>>);

fn norm(p: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

pub fn allow_path(app: &AppHandle, p: &std::path::Path) {
    if let Some(st) = app.try_state::<Picked>() {
        st.0.lock().unwrap_or_else(|e| e.into_inner()).insert(norm(p));
    }
}

fn picked(app: &AppHandle, p: &std::path::Path) -> bool {
    app.try_state::<Picked>()
        .map(|st| st.0.lock().unwrap_or_else(|e| e.into_inner()).contains(&norm(p)))
        .unwrap_or(false)
}

/// A backup to inspect or restore: picked in the dialog, or inside one of
/// the backup folders (automatic backups and safety copies).
pub fn check_backup_path(app: &AppHandle, path: &str) -> R<()> {
    let p = std::path::Path::new(path);
    if picked(app, p) {
        return Ok(());
    }
    let file = norm(p);
    let db = app.state::<Db>();
    let l = local::get(&db.lock());
    let mut dirs = vec![autobackup::dir_of(app, &l)];
    if let Ok(d) = app.path().app_data_dir() {
        dirs.push(d.join("backups"));
    }
    let inside = dirs.iter().any(|d| file.parent().map(|f| f == norm(d)).unwrap_or(false));
    if inside && file.extension().map(|e| e == "json").unwrap_or(false) {
        Ok(())
    } else {
        Err("esse arquivo não foi escolhido por você".into())
    }
}

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
pub fn backup_inspect(app: AppHandle, path: String) -> R<BackupInfo> {
    check_backup_path(&app, &path)?;
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
    allow_path(&app, &dir);
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || pcsync::preview(&a, &dir).map(Some)).await.map_err(err)?
}

/// `how`: "merge", "adopt" or "mine" (see `pcsync::join`).
#[tauri::command]
pub async fn sync_join(app: AppHandle, dir: String, how: String) -> R<pcsync::Report> {
    if !picked(&app, std::path::Path::new(&dir)) {
        return Err("escolha a pasta pelo botão Escolher pasta".into());
    }
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

/* ---------------- updates ---------------- */

#[tauri::command]
pub fn update_status(app: AppHandle) -> crate::updater::Status {
    crate::updater::status(&app)
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> R<crate::updater::Status> {
    crate::updater::check(&app, true).await
}

/// "Reiniciar e atualizar" (the UI saved everything first).
#[tauri::command]
pub fn update_install(app: AppHandle) -> R<()> {
    crate::updater::install(&app, false)
}

#[tauri::command]
pub fn update_set(
    app: AppHandle,
    db: State<'_, Db>,
    auto_check: Option<bool>,
    auto_install: Option<bool>,
) -> R<crate::updater::Status> {
    {
        let c = db.lock();
        local::update(&c, |l| {
            if let Some(v) = auto_check {
                l.update_check_off = !v;
            }
            if let Some(v) = auto_install {
                l.update_install_ask = !v;
            }
        })?;
    }
    Ok(crate::updater::status(&app))
}

/// The "what's new" note after an update was shown.
#[tauri::command]
pub fn update_seen(app: AppHandle) {
    crate::updater::seen(&app);
}
