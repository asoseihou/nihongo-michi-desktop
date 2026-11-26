//! Automatic updates from the GitHub releases. The installer is signed in
//! the release workflow and checked here against the public key in
//! `tauri.conf.json`, so only builds signed with the project's key install.
//!
//! The app looks for a new version a little after it starts and every few
//! hours, downloads it in the background and installs it when the window is
//! closed (the app comes back in the tray), or at once when the learner
//! clicks "Reiniciar e atualizar".

use crate::db::{err, Db, R};
use crate::local;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const EVENT: &str = "michi://update";
/// Left by the version being replaced, read by the new one on start.
const MARKER: &str = "atualizacao.json";
/// First check after the start, then every `INTERVAL`.
pub const FIRST_CHECK: Duration = Duration::from_secs(40);
pub const INTERVAL: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct JustUpdated {
    pub from: String,
    pub to: String,
    pub notes: String,
    /// The window was closed when it installed: start in the tray.
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub current: String,
    /// idle | checking | latest | downloading | ready | installing | error
    pub state: String,
    pub version: String,
    pub notes: String,
    pub progress: f64,
    pub error: String,
    pub checked_at: f64,
    pub auto_check: bool,
    pub auto_install: bool,
    pub just_updated: Option<JustUpdated>,
}

#[derive(Default)]
pub struct UpdState {
    status: Mutex<Status>,
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    checking: AtomicBool,
    last_check: Mutex<Option<Instant>>,
}

/// Clears the "checking" flag however the check ends.
struct Checking<'a>(&'a AtomicBool);

impl Drop for Checking<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn marker_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(MARKER))
}

/// Reads (and removes) what the previous version left before installing.
pub fn take_marker(app: &AppHandle) -> Option<JustUpdated> {
    let p = marker_path(app)?;
    let text = std::fs::read_to_string(&p).ok()?;
    let _ = std::fs::remove_file(&p);
    serde_json::from_str(&text).ok()
}

pub fn init(app: &AppHandle, just: Option<JustUpdated>) {
    let st = UpdState::default();
    {
        let mut s = lock(&st.status);
        s.current = app.package_info().version.to_string();
        s.state = "idle".into();
        s.just_updated = just;
    }
    app.manage(st);
}

pub fn status(app: &AppHandle) -> Status {
    let mut s = lock(&app.state::<UpdState>().status).clone();
    let db = app.state::<Db>();
    let l = local::get(&db.lock());
    s.auto_check = !l.update_check_off;
    s.auto_install = !l.update_install_ask;
    s
}

fn set(app: &AppHandle, f: impl FnOnce(&mut Status)) {
    f(&mut lock(&app.state::<UpdState>().status));
    let _ = app.emit(EVENT, status(app));
}

pub fn due(app: &AppHandle, started: Instant) -> bool {
    let db = app.state::<Db>();
    if local::get(&db.lock()).update_check_off {
        return false;
    }
    let st = app.state::<UpdState>();
    if lock(&st.ready).is_some() {
        return false;
    }
    let last = *lock(&st.last_check);
    match last {
        None => started.elapsed() >= FIRST_CHECK,
        Some(t) => t.elapsed() >= INTERVAL,
    }
}

/// Looks for a new version and downloads it. `manual`: from the button.
pub async fn check(app: &AppHandle, manual: bool) -> R<Status> {
    let st = app.state::<UpdState>();
    if st.checking.swap(true, Ordering::SeqCst) {
        return Ok(status(app)); // a check is already running
    }
    let _one = Checking(&st.checking);
    *lock(&st.last_check) = Some(Instant::now());
    if lock(&st.ready).is_some() {
        return Ok(status(app));
    }
    set(app, |s| {
        s.state = "checking".into();
        s.error.clear();
    });
    let handle = app.clone();
    let updater = app
        .updater_builder()
        .on_before_exit(move || {
            // Leave the latest progress in the shared folder, then let Tauri
            // tidy up (tray icon) before the installer replaces the app.
            let _ = crate::pcsync::push_only(&handle);
            handle.cleanup_before_exit();
        })
        .build()
        .map_err(err)?;
    let found = match updater.check().await {
        Ok(u) => u,
        Err(e) => {
            let msg = e.to_string();
            set(app, |s| {
                s.state = "error".into();
                s.error = msg.clone();
                s.checked_at = michi_core::time::now_ms();
            });
            return if manual { Err(msg) } else { Ok(status(app)) };
        }
    };
    let Some(update) = found else {
        set(app, |s| {
            s.state = "latest".into();
            s.checked_at = michi_core::time::now_ms();
        });
        return Ok(status(app));
    };
    let (version, notes) = (update.version.clone(), update.body.clone().unwrap_or_default());
    set(app, |s| {
        s.state = "downloading".into();
        s.version = version.clone();
        s.notes = notes.clone();
        s.progress = 0.0;
        s.checked_at = michi_core::time::now_ms();
    });
    let mut got = 0usize;
    let mut last_emit = Instant::now();
    let a2 = app.clone();
    let bytes = update
        .download(
            |chunk, total| {
                got += chunk;
                if let Some(t) = total.filter(|t| *t > 0) {
                    if last_emit.elapsed() > Duration::from_millis(400) {
                        last_emit = Instant::now();
                        let p = got as f64 / t as f64;
                        set(&a2, |s| s.progress = p.min(1.0));
                    }
                }
            },
            || {},
        )
        .await;
    match bytes {
        Ok(b) => {
            *lock(&st.ready) = Some((update, b));
            set(app, |s| {
                s.state = "ready".into();
                s.progress = 1.0;
            });
            Ok(status(app))
        }
        Err(e) => {
            let msg = e.to_string();
            set(app, |s| {
                s.state = "error".into();
                s.error = msg.clone();
            });
            if manual {
                Err(msg)
            } else {
                Ok(status(app))
            }
        }
    }
}

/// Installs the downloaded version: the app closes, the installer runs and
/// opens the new version (in the tray when `hidden`).
pub fn install(app: &AppHandle, hidden: bool) -> R<()> {
    let st = app.state::<UpdState>();
    let Some((update, bytes)) = lock(&st.ready).take() else {
        return Err("nenhuma atualização baixada".into());
    };
    let just = JustUpdated {
        from: app.package_info().version.to_string(),
        to: update.version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        hidden,
    };
    if let Some(p) = marker_path(app) {
        let _ = std::fs::write(&p, serde_json::to_string(&just).map_err(err)?);
    }
    set(app, |s| s.state = "installing".into());
    // On Windows this does not return: the process exits for the installer.
    if let Err(e) = update.install(&bytes) {
        if let Some(p) = marker_path(app) {
            let _ = std::fs::remove_file(p);
        }
        let msg = e.to_string();
        *lock(&st.ready) = Some((update, bytes));
        set(app, |s| {
            s.state = "error".into();
            s.error = msg.clone();
        });
        return Err(msg);
    }
    Ok(())
}

/// Installs by itself when allowed, the version is ready and the window is closed.
pub fn maybe_install(app: &AppHandle, window_open: bool) {
    if window_open || lock(&app.state::<UpdState>().ready).is_none() {
        return;
    }
    let db = app.state::<Db>();
    if local::get(&db.lock()).update_install_ask {
        return;
    }
    if let Err(e) = install(app, true) {
        log::warn!("atualização: {e}");
    }
}

pub fn seen(app: &AppHandle) {
    lock(&app.state::<UpdState>().status).just_updated = None;
}
