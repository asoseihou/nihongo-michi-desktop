//! Daily automatic backup, in the same JSON format as "💾 Exportar backup"
//! (so any of them restores here, on another PC or in the web version), and
//! a safety copy before anything that replaces the progress (restore, reset,
//! taking another PC's progress).

use crate::backup;
use crate::db::{self, err, Db, R};
use crate::local::{self, Local};
use michi_core::time::{now_ms, today_key};
use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub const AUTO_PREFIX: &str = "nihongo-michi-auto-";
pub const SAFE_PREFIX: &str = "nihongo-michi-antes-de-";
/// Safety copies kept (on top of the daily ones).
const SAFE_KEEP: usize = 10;

/// `Documentos\Nihongo Michi\Backups` (or the data folder when there is no
/// Documents folder).
pub fn default_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .document_dir()
        .map(|d| d.join("Nihongo Michi").join("Backups"))
        .or_else(|_| app.path().app_data_dir().map(|d| d.join("backups")))
        .unwrap_or_else(|_| PathBuf::from("backups"))
}

pub fn dir_of(app: &AppHandle, l: &Local) -> PathBuf {
    if l.backup_dir.trim().is_empty() {
        default_dir(app)
    } else {
        PathBuf::from(l.backup_dir.trim())
    }
}

/// Writes next to the target and renames over it, so a half-written file
/// never replaces a good one.
pub fn write_atomic(path: &Path, text: &str) -> R<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("não consegui criar a pasta {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("não consegui gravar em {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("não consegui gravar {}: {e}", path.display())
    })
}

fn export_text(c: &Connection) -> R<String> {
    serde_json::to_string(&backup::export(c)?).map_err(err)
}

/// Keeps the newest `keep` files that start with `prefix`.
fn prune(dir: &Path, prefix: &str, keep: usize) {
    let mut files: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            (name.starts_with(prefix) && name.ends_with(".json")).then(|| (name, e.path()))
        })
        .collect();
    // The names carry the date (and time), so name order is age order.
    files.sort();
    let extra = files.len().saturating_sub(keep);
    for (_, p) in files.into_iter().take(extra) {
        let _ = std::fs::remove_file(p);
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Status {
    pub on: bool,
    pub dir: String,
    pub default_dir: String,
    pub keep: usize,
    pub last_at: f64,
    pub last_file: String,
    pub error: String,
    pub files: Vec<FileInfo>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    /// auto | safety | manual
    pub kind: String,
    pub at: f64,
    pub size: u64,
}

fn modified_ms(m: &std::fs::Metadata) -> f64 {
    m.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// Backups in the folder, newest first.
pub fn list(dir: &Path) -> Vec<FileInfo> {
    let mut out: Vec<FileInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with("nihongo-michi-") || !name.ends_with(".json") {
                return None;
            }
            let meta = e.metadata().ok()?;
            let kind = if name.starts_with(AUTO_PREFIX) {
                "auto"
            } else if name.starts_with(SAFE_PREFIX) {
                "safety"
            } else {
                "manual"
            };
            Some(FileInfo {
                path: e.path().display().to_string(),
                name,
                kind: kind.into(),
                at: modified_ms(&meta),
                size: meta.len(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.at.partial_cmp(&a.at).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn status(app: &AppHandle, c: &Connection) -> Status {
    let l = local::get(c);
    let dir = dir_of(app, &l);
    Status {
        on: !l.backup_off,
        dir: dir.display().to_string(),
        default_dir: default_dir(app).display().to_string(),
        keep: l.keep(),
        last_at: db::meta_get(c, "backup_last_at").and_then(|s| s.parse().ok()).unwrap_or(0.0),
        last_file: db::meta_get(c, "backup_last_file").unwrap_or_default(),
        error: db::meta_get(c, "backup_error").unwrap_or_default(),
        files: list(&dir).into_iter().take(12).collect(),
    }
}

/// Makes today's backup now. Returns the file written.
pub fn backup_now(app: &AppHandle, c: &Connection) -> R<PathBuf> {
    let l = local::get(c);
    let dir = dir_of(app, &l);
    let path = dir.join(format!("{AUTO_PREFIX}{}.json", today_key()));
    let res = export_text(c).and_then(|text| write_atomic(&path, &text));
    match &res {
        Ok(()) => {
            prune(&dir, AUTO_PREFIX, l.keep());
            db::meta_set(c, "backup_last_day", &today_key())?;
            db::meta_set(c, "backup_last_at", &format!("{}", now_ms()))?;
            db::meta_set(c, "backup_last_file", &path.display().to_string())?;
            db::meta_set(c, "backup_error", "")?;
        }
        Err(e) => {
            db::meta_set(c, "backup_error", e)?;
        }
    }
    res.map(|_| path)
}

/// The daily backup, when it is on and today's is still missing.
pub fn run_daily(app: &AppHandle) -> R<bool> {
    let db = app.state::<Db>();
    let c = db.lock();
    let l = local::get(&c);
    if l.backup_off || db::meta_get(&c, "backup_last_day").as_deref() == Some(today_key().as_str()) {
        return Ok(false);
    }
    backup_now(app, &c).map(|_| true)
}

/// Safety copy before replacing the progress (`what`: "restaurar",
/// "zerar", "sincronizar"). Falls back to the data folder when the backup
/// folder is unavailable (a removed USB drive, say); fails only if neither
/// can be written, and then the caller must not go on.
pub fn safety(app: &AppHandle, c: &Connection, what: &str) -> R<PathBuf> {
    let text = export_text(c)?;
    let stamp = michi_core::time::local(now_ms()).format("%Y-%m-%d-%H%M%S").to_string();
    let name = format!("{SAFE_PREFIX}{what}-{stamp}.json");
    let l = local::get(c);
    let mut dirs = vec![dir_of(app, &l)];
    if let Ok(d) = app.path().app_data_dir() {
        dirs.push(d.join("backups"));
    }
    let mut last_err = String::new();
    for dir in dirs {
        let path = dir.join(&name);
        match write_atomic(&path, &text) {
            Ok(()) => {
                prune(&dir, SAFE_PREFIX, SAFE_KEEP);
                return Ok(path);
            }
            Err(e) => last_err = e,
        }
    }
    Err(format!("não consegui guardar uma cópia de segurança antes de {what}: {last_err}"))
}

/// Peeks into a backup file (for the restore confirmation).
pub fn inspect(path: &str) -> R<backup::BackupInfo> {
    backup::inspect(path).map(|(_, i)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_keeps_newest_by_name() {
        let dir = std::env::temp_dir().join(format!("michi-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for d in ["2026-11-20", "2026-11-21", "2026-11-22", "2026-11-23"] {
            std::fs::write(dir.join(format!("{AUTO_PREFIX}{d}.json")), "{}").unwrap();
        }
        std::fs::write(dir.join("nihongo-michi-backup-2026-01-01.json"), "{}").unwrap();
        prune(&dir, AUTO_PREFIX, 2);
        let mut names: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                format!("{AUTO_PREFIX}2026-11-22.json"),
                format!("{AUTO_PREFIX}2026-11-23.json"),
                "nihongo-michi-backup-2026-01-01.json".to_string()
            ],
            "manual exports are never pruned"
        );
        let l = list(&dir);
        assert_eq!(l.len(), 3);
        assert!(l.iter().any(|f| f.kind == "manual"));
        write_atomic(&dir.join("x.json"), "{\"a\":1}").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("x.json")).unwrap(), "{\"a\":1}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
