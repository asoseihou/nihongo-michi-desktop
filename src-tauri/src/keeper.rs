//! Background housekeeping: the daily backup and the sync between PCs.
//!
//! The first sync runs before the window reads the data (`load_all` waits
//! for it), so the app opens with the other PC's progress already in. While
//! the window is open, changes from another PC are only announced (applying
//! them under the learner's feet could undo what they are doing); they are
//! applied when the window closes to the tray, or at once when the learner
//! clicks "Atualizar".

use crate::db::Db;
use crate::events;
use crate::pcsync::{self, Mode, SyncState};
use crate::{autobackup, local};
use serde::Serialize;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TICK: Duration = Duration::from_secs(60);
/// The first daily backup waits this long, so it never slows the start.
const BACKUP_DELAY: Duration = Duration::from_secs(45);
/// How long `load_all` waits for the first sync (a slow cloud folder).
pub const FIRST_SYNC_WAIT: Duration = Duration::from_secs(8);

pub const SYNC_PENDING: &str = "michi://sync-pending";
pub const SYNC_STATUS: &str = "michi://sync-status";
pub const RELOAD: &str = "michi://reload";

enum Msg {
    Tick,
    Hidden,
}

#[derive(Default)]
pub struct Ready {
    done: Mutex<bool>,
    cv: Condvar,
}

impl Ready {
    fn set(&self) {
        *self.done.lock().unwrap_or_else(|p| p.into_inner()) = true;
        self.cv.notify_all();
    }

    pub fn wait(&self, max: Duration) {
        let g = self.done.lock().unwrap_or_else(|p| p.into_inner());
        let _ = self.cv.wait_timeout_while(g, max, |done| !*done);
    }
}

pub struct Keeper {
    tx: Mutex<Sender<Msg>>,
    pub ready: Arc<Ready>,
}

impl Keeper {
    /// The main window was closed to the tray.
    pub fn hidden(&self) {
        let _ = self.tx.lock().map(|t| t.send(Msg::Hidden));
    }
    /// Something worth a look now (folder chosen, data restored…).
    pub fn poke(&self) {
        let _ = self.tx.lock().map(|t| t.send(Msg::Tick));
    }
}

#[derive(Clone, Serialize)]
pub struct SyncStatusEvent {
    pub on: bool,
    pub ok: bool,
    pub last_at: f64,
    pub pending: Vec<String>,
}

fn window_open(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
        .unwrap_or(false)
}

pub fn emit_status(app: &AppHandle) {
    let s = pcsync::status(app);
    let _ = app.emit(
        SYNC_STATUS,
        SyncStatusEvent { on: s.on, ok: s.error.is_empty(), last_at: s.last_at, pending: s.pending.clone() },
    );
}

/// After changes from another PC landed in the database.
pub fn after_apply(app: &AppHandle) {
    if let Some(s) = app.try_state::<crate::scheduler::Scheduler>() {
        s.wake();
    }
    let _ = app.emit(crate::agenda_cmd::CHANGED, ());
    events::avisos_changed(app);
    crate::tray::refresh(app);
    let _ = app.emit_to("main", RELOAD, ());
}

/// One sync round; `allow_apply` false while the window is open.
fn run_sync(app: &AppHandle, allow_apply: bool) {
    match pcsync::sync_once(app, allow_apply, Mode::Normal) {
        Ok(r) => {
            if r.received > 0 {
                after_apply(app);
            }
            if !r.pending.is_empty() {
                let _ = app.emit_to("main", SYNC_PENDING, r.pending.clone());
            }
        }
        Err(e) => log::warn!("sync: {e}"),
    }
    emit_status(app);
}

fn sync_wanted(app: &AppHandle) -> bool {
    let db = app.state::<Db>();
    let (l, dirty) = {
        let c = db.lock();
        let pushed: i64 = crate::db::meta_get(&c, "sync_pushed").and_then(|s| s.parse().ok()).unwrap_or(-1);
        (local::get(&c), pcsync::local_max_t(&c) > pushed)
    };
    if !l.sync_on() {
        return false;
    }
    let dir = std::path::PathBuf::from(l.sync_dir.trim());
    dirty || app.state::<SyncState>().remote_changed(&dir, &pcsync::own_file(&dir, &l))
}

pub fn start(app: &AppHandle) {
    let (tx, rx) = channel::<Msg>();
    let ready = Arc::new(Ready::default());
    app.manage(Keeper { tx: Mutex::new(tx), ready: ready.clone() });
    let app = app.clone();
    std::thread::spawn(move || {
        // The window is still loading: the first sync may change anything.
        run_sync(&app, true);
        ready.set();
        crate::scheduler::start(&app);
        let started = Instant::now();
        loop {
            let msg = match rx.recv_timeout(TICK) {
                Ok(m) => m,
                Err(RecvTimeoutError::Timeout) => Msg::Tick,
                Err(RecvTimeoutError::Disconnected) => break,
            };
            if let Msg::Hidden = msg {
                // Let the window save what it still holds before reading it back.
                std::thread::sleep(Duration::from_secs(3));
            }
            if started.elapsed() >= BACKUP_DELAY {
                if let Err(e) = autobackup::run_daily(&app) {
                    log::warn!("backup: {e}");
                }
            }
            if matches!(msg, Msg::Hidden) || sync_wanted(&app) {
                run_sync(&app, !window_open(&app));
            }
        }
    });
}
