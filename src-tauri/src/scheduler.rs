//! The reminder scheduler. Runs in its own thread while the app sits in the
//! tray; every tick it looks for reminders whose time came since the last
//! tick, records them (notification centre) and opens the popup with sound.
//! On start it also records the reminders missed while the app was closed.

use crate::agenda::day_of;
use crate::db::{self, Db, R};
use crate::{aviso, events};
use chrono::{Duration, NaiveTime};
use michi_core::agenda::{Aviso, Occ};
use michi_core::routine::routine_of;
use michi_core::time::{local, ms_of, now_ms};
use rusqlite::Connection;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

const TICK_SECS: u64 = 15;
/// How far back missed reminders are recovered on start.
const MISSED_WINDOW_MS: f64 = 36.0 * 3_600_000.0;
/// A reminder this recent at start-up still pops up normally.
const GRACE_MS: f64 = 5.0 * 60_000.0;
/// All-day items remind relative to 09:00 of their day.
const ALL_DAY_HOUR: u32 = 9;

pub struct Scheduler(Mutex<Sender<()>>);

impl Scheduler {
    /// Wakes the scheduler now (data changed, snooze chosen…).
    pub fn wake(&self) {
        let _ = self.0.lock().map(|s| s.send(()));
    }
}

pub fn start(app: &AppHandle) {
    let (tx, rx) = channel::<()>();
    app.manage(Scheduler(Mutex::new(tx)));
    let app = app.clone();
    std::thread::spawn(move || {
        let mut startup = true;
        loop {
            if let Err(e) = tick(&app, startup) {
                log::warn!("scheduler: {e}");
            }
            startup = false;
            match rx.recv_timeout(std::time::Duration::from_secs(TICK_SECS)) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// When a reminder of an occurrence fires.
pub fn fire_time(o: &Occ, minutes_before: i64) -> chrono::NaiveDateTime {
    let start = if o.all_day {
        o.start().date().and_time(NaiveTime::from_hms_opt(ALL_DAY_HOUR, 0, 0).unwrap())
    } else {
        o.start()
    };
    start - Duration::minutes(minutes_before)
}

/// Is the occurrence already done (so the reminder is pointless)?
fn already_done(c: &Connection, o: &Occ) -> bool {
    match o.block_id() {
        Some(b) => db::day_get(c, &day_of(o)).map(|d| d.block_done(b)).unwrap_or(false),
        None => db::occ_done_between(c, &o.occ, &o.occ)
            .map(|s| s.contains(&(o.event_id.clone(), o.occ.clone())))
            .unwrap_or(false),
    }
}

pub struct Fired {
    pub shown: usize,
    pub missed: usize,
}

/// Records every reminder due in `(last, now]`. Pure apart from the DB, so
/// it is unit-tested without a window.
pub fn collect(c: &Connection, last: f64, now: f64, startup: bool) -> R<Fired> {
    let desk = db::desk(c);
    let p = db::profile(c);
    let blocks = routine_of(&p.routine);
    let mut fired = Fired { shown: 0, missed: 0 };
    if !desk.reminders_on {
        return Ok(fired);
    }
    let from = local(last).date() - Duration::days(1);
    let to = local(now).date() + Duration::days(2);
    for e in db::events_all(c)?.iter().filter(|e| !e.off && !e.reminders.is_empty()) {
        for o in e.expand(from, to) {
            if let Some(b) = o.block_id() {
                // Minimal day keeps only the essential blocks.
                if p.minimal_day && !blocks.iter().any(|x| x.id == b && x.core) {
                    continue;
                }
            }
            for &before in &o.reminders {
                let fire = ms_of(fire_time(&o, before));
                if fire <= last || fire > now {
                    continue;
                }
                let key = format!("{}|{}|{}", o.event_id, o.occ, before);
                if db::notif_key_exists(c, &key) || already_done(c, &o) {
                    continue;
                }
                let fire_local = local(fire).time();
                let status = if startup && now - fire > GRACE_MS {
                    "missed"
                } else if desk.pause_until > now || desk.is_quiet(fire_local) {
                    "silenced"
                } else {
                    "shown"
                };
                let a = Aviso {
                    id: 0,
                    event_id: o.event_id.clone(),
                    occ: o.occ.clone(),
                    title: o.title.clone(),
                    jp: o.jp.clone(),
                    body: body_of(&o, before),
                    kind: o.kind,
                    color: o.color.clone(),
                    go: o.go.clone(),
                    at: if o.all_day { String::new() } else { o.time.clone() },
                    dur: o.dur,
                    fire_at: fire,
                    status: status.into(),
                    snooze_until: None,
                    read: false,
                };
                if db::notif_insert(c, &key, &a)?.is_some() {
                    match status {
                        "shown" => fired.shown += 1,
                        "missed" => fired.missed += 1,
                        _ => {}
                    }
                }
            }
        }
    }
    // Snoozed reminders come back (also during quiet hours: the learner chose the time).
    for a in db::notif_snooze_due(c, now as i64)? {
        db::notif_set_status(c, a.id, "shown", None)?;
        fired.shown += 1;
    }
    Ok(fired)
}

fn body_of(o: &Occ, before: i64) -> String {
    let when = if o.all_day {
        "hoje".to_string()
    } else if before > 0 {
        format!("em {before} min · {} · {} min", o.time, o.dur)
    } else {
        format!("agora · {} min", o.dur)
    };
    if o.notes.is_empty() {
        when
    } else {
        format!("{when} — {}", o.notes)
    }
}

fn tick(app: &AppHandle, startup: bool) -> R<()> {
    let db = app.state::<Db>();
    let now = now_ms();
    let fired = {
        let c = db.lock();
        let last = db::meta_get(&c, "last_tick")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(now)
            .max(now - MISSED_WINDOW_MS)
            .min(now);
        let f = collect(&c, last, now, startup)?;
        db::meta_set(&c, "last_tick", &format!("{now}"))?;
        f
    };
    // During a focus session the reminders wait in the centre; the popup
    // opens when the session ends.
    if (fired.shown > 0 || fired.missed > 0) && !crate::foco::running(app) {
        aviso::show(app);
        if fired.shown > 0 {
            let desk = db::desk(&db.lock());
            if desk.sound {
                crate::sound::chime(desk.volume);
            }
        }
        events::avisos_changed(app);
    }
    crate::tray::refresh(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::sync_events;
    use chrono::NaiveDate;
    use serde_json::json;

    fn at(d: NaiveDate, h: u32, m: u32) -> f64 {
        ms_of(d.and_hms_opt(h, m, 0).unwrap())
    }

    #[test]
    fn fires_once_skips_done_and_handles_quiet_and_missed() {
        let db = Db::memory();
        let c = db.lock();
        db::doc_put(&c, "desk", &json!({"quietOn":true,"quietStart":"07:00","quietEnd":"08:00"})).unwrap();
        sync_events(&c).unwrap();
        let today = local(now_ms()).date();
        let srs_series = db::event_get(&c, "rotina-srs").unwrap();
        let d = NaiveDate::parse_from_str(&srs_series.date, "%Y-%m-%d").unwrap().max(today);

        // 07:30 (srs) is inside the quiet hours → silenced, no popup.
        let f = collect(&c, at(d, 7, 0), at(d, 7, 31), false).unwrap();
        assert_eq!(f.shown, 0);
        // 12:30 (jogo) → shown once.
        let f = collect(&c, at(d, 12, 0), at(d, 12, 31), false).unwrap();
        assert_eq!(f.shown, 1);
        let f = collect(&c, at(d, 12, 0), at(d, 12, 31), false).unwrap();
        assert_eq!(f.shown, 0, "same key never fires twice");

        // vlog already done on the board → no reminder.
        let k = michi_core::time::key_of(d);
        db::day_put(&c, &k, &json!({"k":k,"done":{"vlog":true}})).unwrap();
        // 20:00 estação, 20:30 wagotabi, 20:50 frases are missed at start-up (older than 5 min).
        let f = collect(&c, at(d, 19, 0), at(d, 21, 40), true).unwrap();
        assert_eq!((f.shown, f.missed), (0, 3));
        let st: Vec<String> = db::notif_list(&c, 50).unwrap().into_iter().map(|a| a.status).collect();
        assert!(st.contains(&"silenced".to_string()));
    }
}
