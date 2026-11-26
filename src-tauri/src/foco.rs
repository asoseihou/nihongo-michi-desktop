//! Focus mode: a timer for a routine block, an agenda item or free study,
//! shown in a small always-on-top window ("foco"). When it ends, the focused
//! minutes count for the day (Today board, goal, heat map) and give XP, so a
//! long session also keeps the streak. Reminders wait until it ends.
//!
//! The session lives in `meta` (this PC only), so it survives the window
//! being closed; a session left running when the app quit is closed on the
//! next start with the time it had really run.

use crate::db::{self, err, Db, R};
use crate::events;
use michi_core::agenda::Kind;
pub use michi_core::focus::{Focus, Spec};
use michi_core::ops::{focus_xp, Op};
use michi_core::time::{now_ms, today_key};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "foco";
pub const EVENT: &str = "michi://foco";
const KEY: &str = "focus";
const WIDTH: f64 = 320.0;
const MARGIN: f64 = 14.0;
/// The running session is saved this often (to know how long it really ran
/// if the app is closed in the middle).
const BEAT_MS: f64 = 30_000.0;

pub struct FocusTimer(Mutex<Sender<()>>);

impl FocusTimer {
    fn wake(&self) {
        let _ = self.0.lock().map(|s| s.send(()));
    }
}

fn load(c: &rusqlite::Connection) -> Option<Focus> {
    db::meta_get(c, KEY).and_then(|s| serde_json::from_str(&s).ok())
}

fn save(c: &rusqlite::Connection, f: Option<&Focus>) -> R<()> {
    match f {
        Some(f) => db::meta_set(c, KEY, &serde_json::to_string(f).map_err(err)?),
        None => {
            c.execute("DELETE FROM meta WHERE key=?1", [KEY]).map_err(err)?;
            Ok(())
        }
    }
}

pub fn current(app: &AppHandle) -> Option<Focus> {
    let db = app.state::<Db>();
    let c = db.lock();
    load(&c)
}

/// A session is counting (running or paused): reminders and updates wait.
pub fn running(app: &AppHandle) -> bool {
    current(app).map(|f| f.active()).unwrap_or(false)
}

fn changed(app: &AppHandle) {
    let _ = app.emit(EVENT, current(app));
    crate::tray::refresh(app);
    if let Some(t) = app.try_state::<FocusTimer>() {
        t.wake();
    }
}

/* ---------------- window ---------------- */

/// Opens the focus window (top-right corner), or tells it to redraw.
pub fn show(app: &AppHandle) {
    let handle = app.clone();
    // Same as the reminder popup: never build a webview on the main thread.
    std::thread::spawn(move || {
        if handle.get_webview_window(LABEL).is_none() {
            let built = WebviewWindowBuilder::new(&handle, LABEL, WebviewUrl::App("index.html".into()))
                .title("Nihongo Michi · foco")
                .inner_size(WIDTH, 170.0)
                .resizable(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .shadow(true)
                .build();
            if let Err(e) = built {
                log::warn!("foco: {e}");
            }
            return;
        }
        let _ = handle.emit_to(LABEL, EVENT, current(&handle));
    });
}

/// Called by the focus window after drawing (`height` in CSS px; 0 hides).
pub fn fit(app: &AppHandle, height: f64, place: bool) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    if height <= 0.0 {
        let _ = w.hide();
        return;
    }
    let monitor = app.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten());
    let Some(m) = monitor else {
        let _ = w.show();
        return;
    };
    let sf = m.scale_factor();
    let (pw, ph) = ((WIDTH * sf).round() as u32, (height.min(400.0) * sf).round() as u32);
    let _ = w.set_size(PhysicalSize::new(pw, ph));
    // Placed in the top-right corner when it first appears; after that it
    // stays wherever the learner dragged it.
    if place {
        let area = m.work_area();
        let x = area.position.x + area.size.width as i32 - pw as i32 - (MARGIN * sf) as i32;
        let y = area.position.y + (MARGIN * sf) as i32;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
    let _ = w.set_always_on_top(true);
    let _ = w.show();
}

pub fn hide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

/* ---------------- session ---------------- */

pub fn start(app: &AppHandle, spec: Spec) -> R<Focus> {
    if running(app) {
        finish(app, false)?;
    }
    let now = now_ms();
    let minutes = spec.minutes.clamp(1.0, 240.0);
    let id = format!("{}", now as u64);
    let key = match (&spec.block, &spec.event_id, &spec.occ) {
        (Some(b), _, _) => format!("rotina:{b}"),
        (None, Some(e), Some(o)) => format!("{e}@{o}"),
        _ => format!("livre:{id}"),
    };
    let f = Focus {
        id,
        title: if spec.title.trim().is_empty() { "Estudo livre".into() } else { spec.title.trim().to_string() },
        jp: if spec.jp.is_empty() && spec.block.is_none() && spec.event_id.is_none() {
            "集中".into()
        } else {
            spec.jp
        },
        color: if spec.color.is_empty() { Kind::Estudo.color().into() } else { spec.color },
        key,
        block: spec.block,
        event_id: spec.event_id,
        occ: spec.occ,
        day: today_key(),
        planned_ms: minutes * 60_000.0,
        started_at: now,
        beat_at: now,
        state: "running".into(),
        ..Default::default()
    };
    {
        let db = app.state::<Db>();
        save(&db.lock(), Some(&f))?;
    }
    show(app);
    changed(app);
    Ok(f)
}

fn update(app: &AppHandle, f: impl FnOnce(&mut Focus)) -> R<Option<Focus>> {
    let out = {
        let db = app.state::<Db>();
        let c = db.lock();
        let Some(mut s) = load(&c) else { return Ok(None) };
        f(&mut s);
        save(&c, Some(&s))?;
        s
    };
    changed(app);
    Ok(Some(out))
}

pub fn pause(app: &AppHandle) -> R<Option<Focus>> {
    let now = now_ms();
    update(app, |f| {
        if f.state == "running" {
            f.state = "paused".into();
            f.paused_at = Some(now);
        }
    })
}

pub fn resume(app: &AppHandle) -> R<Option<Focus>> {
    let now = now_ms();
    update(app, |f| {
        if f.state == "paused" {
            f.paused_ms += now - f.paused_at.unwrap_or(now);
            f.paused_at = None;
            f.state = "running".into();
            f.beat_at = now;
        }
    })
}

pub fn add(app: &AppHandle, minutes: f64) -> R<Option<Focus>> {
    let m = minutes.clamp(1.0, 120.0);
    update(app, |f| {
        if f.state != "done" {
            f.planned_ms += m * 60_000.0;
        }
    })
}

/// Ends the session and credits its minutes. `completed`: the time ran out.
pub fn finish(app: &AppHandle, completed: bool) -> R<Option<Focus>> {
    let now = now_ms();
    let (f, ops) = {
        let db = app.state::<Db>();
        let c = db.lock();
        let Some(mut f) = load(&c) else { return Ok(None) };
        if f.state == "done" {
            return Ok(Some(f));
        }
        let elapsed = f.elapsed_ms(now);
        let minutes = (elapsed / 60_000.0).round();
        f.state = "done".into();
        f.finished_at = f.started_at + f.paused_ms + elapsed;
        f.paused_at = None;
        f.completed = completed;
        let mut ops = Vec::new();
        if minutes >= 1.0 {
            f.credited_min = minutes;
            f.xp = focus_xp(minutes);
            ops.push(Op::Fo { day: f.day.clone(), key: f.key.clone(), min: minutes });
            if f.xp > 0.0 {
                ops.push(Op::Xp { day: f.day.clone(), n: f.xp });
            }
            crate::agenda::apply_ops(&c, &ops)?;
        }
        save(&c, Some(&f))?;
        (f, ops)
    };
    events::patch(app, &ops);
    if completed {
        let desk = db::desk(&app.state::<Db>().lock());
        if desk.sound {
            crate::sound::chime(desk.volume);
        }
    }
    show(app);
    changed(app);
    // Reminders held during the session show up now.
    let waiting = { !db::notif_active(&app.state::<Db>().lock()).unwrap_or_default().is_empty() };
    if waiting {
        crate::aviso::show(app);
    }
    Ok(Some(f))
}

/// "Marcar feito" in the summary: the block / agenda item counts as done.
pub fn mark_done(app: &AppHandle) -> R<Option<Focus>> {
    let ops = {
        let db = app.state::<Db>();
        let c = db.lock();
        let Some(f) = load(&c) else { return Ok(None) };
        if f.marked || f.state != "done" {
            return Ok(Some(f));
        }
        let ops = match (&f.block, &f.event_id, &f.occ) {
            (Some(b), _, _) => {
                let occ = crate::agenda::find_occ(&c, &format!("rotina-{b}"), &f.day)?;
                match occ {
                    Some(o) => crate::agenda::mark(&c, &o, true)?,
                    None => {
                        let ops = michi_core::ops::mark_ops(&f.day, Some(b), &f.key, 0.0, true);
                        crate::agenda::apply_ops(&c, &ops)?;
                        ops
                    }
                }
            }
            (None, Some(e), Some(o)) => match crate::agenda::find_occ(&c, e, o)? {
                Some(occ) => crate::agenda::mark(&c, &occ, true)?,
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        crate::agenda::close_done_routine(&c, &f.day)?;
        let mut f = f;
        f.marked = true;
        save(&c, Some(&f))?;
        ops
    };
    events::patch(app, &ops);
    events::avisos_changed(app);
    changed(app);
    Ok(current(app))
}

/// Closes the summary (or abandons a session without counting it).
pub fn close(app: &AppHandle) -> R<()> {
    {
        let db = app.state::<Db>();
        let c = db.lock();
        if load(&c).map(|f| f.state == "done").unwrap_or(true) {
            save(&c, None)?;
        }
    }
    hide(app);
    changed(app);
    Ok(())
}

/* ---------------- timer ---------------- */

/// A session still "running" from before the app closed ends with the time
/// it had really run (up to the last beat).
fn recover(app: &AppHandle) {
    enum Then {
        Nothing,
        Finish,
        Show,
    }
    let then = {
        let db = app.state::<Db>();
        let c = db.lock();
        match load(&c) {
            // A summary nobody closed: drop it.
            Some(f) if f.state == "done" => {
                let _ = save(&c, None);
                Then::Nothing
            }
            // The app was closed mid-session: it counted until the last beat.
            Some(mut f) if f.state == "running" && now_ms() - f.beat_at > 2.0 * BEAT_MS => {
                f.state = "paused".into();
                f.paused_at = Some(f.beat_at);
                let _ = save(&c, Some(&f));
                Then::Finish
            }
            Some(_) => Then::Show,
            None => Then::Nothing,
        }
    };
    match then {
        Then::Finish => {
            if let Err(e) = finish(app, false) {
                log::warn!("foco: {e}");
            }
        }
        Then::Show => show(app),
        Then::Nothing => {}
    }
}

pub fn start_timer(app: &AppHandle) {
    let (tx, rx) = channel::<()>();
    app.manage(FocusTimer(Mutex::new(tx)));
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recover(&app)));
        loop {
            // A bug in one round must not stop the timer for good.
            let step = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| next_wait(&app)));
            let wait = step.unwrap_or(60_000.0);
            match rx.recv_timeout(Duration::from_millis(wait.max(50.0) as u64)) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// One timer round: ends a session whose time ran out, saves the heartbeat,
/// and says how long to sleep (ms).
fn next_wait(app: &AppHandle) -> f64 {
    loop {
        let wait = match current(app) {
            Some(f) if f.state == "running" => {
                let now = now_ms();
                let left = f.remaining_ms(now);
                if left <= 0.0 {
                    if let Err(e) = finish(app, true) {
                        log::warn!("foco: {e}");
                    }
                    continue;
                }
                if now - f.beat_at >= BEAT_MS {
                    let _ = update_quiet(app, now);
                }
                left.min(BEAT_MS)
            }
            _ => 3_600_000.0,
        };
        return wait;
    }
}

/// Saves the heartbeat without telling the windows.
fn update_quiet(app: &AppHandle, now: f64) -> R<()> {
    let db = app.state::<Db>();
    let c = db.lock();
    if let Some(mut f) = load(&c) {
        f.beat_at = now;
        save(&c, Some(&f))?;
    }
    Ok(())
}

/* ---------------- commands ---------------- */

#[tauri::command]
pub fn foco_state(app: AppHandle) -> Option<Focus> {
    current(&app)
}

#[tauri::command]
pub fn foco_start(app: AppHandle, spec: Spec) -> R<Focus> {
    start(&app, spec)
}

#[tauri::command]
pub fn foco_pause(app: AppHandle) -> R<Option<Focus>> {
    pause(&app)
}

#[tauri::command]
pub fn foco_resume(app: AppHandle) -> R<Option<Focus>> {
    resume(&app)
}

#[tauri::command]
pub fn foco_add(app: AppHandle, minutes: f64) -> R<Option<Focus>> {
    add(&app, minutes)
}

/// "Encerrar": ends now and counts the minutes so far.
#[tauri::command]
pub fn foco_stop(app: AppHandle) -> R<Option<Focus>> {
    finish(&app, false)
}

#[tauri::command]
pub fn foco_mark(app: AppHandle) -> R<Option<Focus>> {
    mark_done(&app)
}

#[tauri::command]
pub fn foco_close(app: AppHandle) -> R<()> {
    close(&app)
}

#[tauri::command]
pub fn foco_show(app: AppHandle) {
    show(&app);
}

#[tauri::command]
pub fn foco_fit(app: AppHandle, height: f64, place: bool) {
    fit(&app, height, place);
}
