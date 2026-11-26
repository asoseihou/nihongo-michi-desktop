//! SQLite storage (rusqlite, bundled SQLite) with numbered migrations.

use michi_core::agenda::{Aviso, DeskSettings, Event, Kind, Override};
use michi_core::{Card, DayDoc, Profile};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// Migrations run in order; `PRAGMA user_version` records the last one.
const MIGRATIONS: &[&str] = &[include_str!("../migrations/0001_init.sql"), include_str!("../migrations/0002_sync.sql")];

pub type R<T> = Result<T, String>;

pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> R<Db> {
        let mut c = Connection::open(path).map_err(err)?;
        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;").map_err(err)?;
        migrate(&mut c)?;
        Ok(Db(Mutex::new(c)))
    }

    #[cfg(test)]
    pub fn memory() -> Db {
        let mut c = Connection::open_in_memory().unwrap();
        migrate(&mut c).unwrap();
        Db(Mutex::new(c))
    }

    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn migrate(c: &mut Connection) -> R<()> {
    let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let n = i as i64 + 1;
        if n <= v {
            continue;
        }
        let tx = c.transaction().map_err(err)?;
        tx.execute_batch(sql).map_err(|e| format!("migração {n}: {e}"))?;
        tx.execute_batch(&format!("PRAGMA user_version = {n}")).map_err(err)?;
        tx.commit().map_err(err)?;
    }
    Ok(())
}

pub fn now() -> i64 {
    michi_core::time::now_ms() as i64
}

/* ---------------- meta ---------------- */

pub fn meta_get(c: &Connection, key: &str) -> Option<String> {
    c.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional().ok().flatten()
}

pub fn meta_set(c: &Connection, key: &str, value: &str) -> R<()> {
    c.execute(
        "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )
    .map_err(err)?;
    Ok(())
}

/* ---------------- documents ---------------- */

pub fn doc_get(c: &Connection, name: &str) -> Option<Value> {
    let s: Option<String> =
        c.query_row("SELECT data FROM docs WHERE name=?1", [name], |r| r.get(0)).optional().ok().flatten();
    s.and_then(|s| serde_json::from_str(&s).ok())
}

pub fn doc_put(c: &Connection, name: &str, data: &Value) -> R<()> {
    doc_put_at(c, name, data, now())
}

/// Same as `doc_put`, keeping the version of a record that came from another PC.
pub fn doc_put_at(c: &Connection, name: &str, data: &Value, t: i64) -> R<()> {
    c.execute(
        "INSERT INTO docs(name,data,updated_at) VALUES(?1,?2,?3)
         ON CONFLICT(name) DO UPDATE SET data=excluded.data, updated_at=excluded.updated_at",
        params![name, data.to_string(), t],
    )
    .map_err(err)?;
    Ok(())
}

pub fn docs_all(c: &Connection) -> R<BTreeMap<String, Value>> {
    let mut st = c.prepare("SELECT name, data FROM docs").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(err)?;
    let mut out = BTreeMap::new();
    for row in rows.flatten() {
        if let Ok(v) = serde_json::from_str(&row.1) {
            out.insert(row.0, v);
        }
    }
    Ok(out)
}

pub fn profile(c: &Connection) -> Profile {
    doc_get(c, "profile").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

pub fn desk(c: &Connection) -> DeskSettings {
    doc_get(c, "desk").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/* ---------------- study days ---------------- */

pub fn day_get(c: &Connection, key: &str) -> Option<DayDoc> {
    let s: Option<String> =
        c.query_row("SELECT data FROM days WHERE day=?1", [key], |r| r.get(0)).optional().ok().flatten();
    s.and_then(|s| serde_json::from_str(&s).ok())
}

pub fn day_put(c: &Connection, key: &str, data: &Value) -> R<()> {
    day_put_at(c, key, data, now())
}

pub fn day_put_at(c: &Connection, key: &str, data: &Value, t: i64) -> R<()> {
    let typed: DayDoc = serde_json::from_value(data.clone()).unwrap_or_else(|_| DayDoc::new(key));
    c.execute(
        "INSERT INTO days(day,xp,minutes,data,updated_at) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(day) DO UPDATE SET xp=excluded.xp, minutes=excluded.minutes, data=excluded.data, updated_at=excluded.updated_at",
        params![key, typed.xp, typed.study_minutes(), data.to_string(), t],
    )
    .map_err(err)?;
    Ok(())
}

pub fn days_all(c: &Connection) -> R<BTreeMap<String, Value>> {
    let mut st = c.prepare("SELECT day, data FROM days").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(err)?;
    let mut out = BTreeMap::new();
    for row in rows.flatten() {
        if let Ok(v) = serde_json::from_str(&row.1) {
            out.insert(row.0, v);
        }
    }
    Ok(out)
}

/* ---------------- SRS cards ---------------- */

pub fn cards_all(c: &Connection) -> R<Vec<(String, Card)>> {
    let mut st = c.prepare("SELECT id,due,ivl,ease,reps,lapses,state,step FROM cards").map_err(err)?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Card([r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get::<_, i64>(6)? as f64, r.get(7)?]),
            ))
        })
        .map_err(err)?;
    Ok(rows.flatten().collect())
}

pub fn card_put(c: &Connection, id: &str, card: Option<&Card>) -> R<()> {
    card_put_at(c, id, card, now())
}

/// Saves (or deletes, leaving a tombstone) one SRS card.
pub fn card_put_at(c: &Connection, id: &str, card: Option<&Card>, t: i64) -> R<()> {
    let clean = card.copied().map(|mut k| (k.sanitize(), k));
    if matches!(clean, Some((false, _))) || id.len() > 300 {
        return Ok(()); // not finite or absurd id: skipped
    }
    let clean = clean.map(|(_, k)| k);
    let card = clean.as_ref();
    match card {
        Some(k) => {
            let a = k.0;
            c.execute(
                "INSERT INTO cards(id,due,ivl,ease,reps,lapses,state,step,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(id) DO UPDATE SET due=excluded.due, ivl=excluded.ivl, ease=excluded.ease, reps=excluded.reps,
                   lapses=excluded.lapses, state=excluded.state, step=excluded.step, updated_at=excluded.updated_at",
                params![id, a[0], a[1], a[2], a[3], a[4], a[5] as i64, a[6], t],
            )
            .map_err(err)?;
            tomb_del(c, "card", id)?;
        }
        None => {
            c.execute("DELETE FROM cards WHERE id=?1", [id]).map_err(err)?;
            tomb_put(c, "card", id, t)?;
        }
    }
    Ok(())
}

/* ---------------- agenda ---------------- */

fn json_vec<T: serde::de::DeserializeOwned>(s: String) -> Vec<T> {
    serde_json::from_str(&s).unwrap_or_default()
}

fn event_of(r: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: r.get("id")?,
        kind: Kind::parse(&r.get::<_, String>("kind")?),
        title: r.get("title")?,
        jp: r.get("jp")?,
        notes: r.get("notes")?,
        all_day: r.get::<_, i64>("all_day")? != 0,
        date: r.get("date")?,
        time: r.get("time")?,
        dur: r.get("dur")?,
        rrule: r.get("rrule")?,
        exdates: json_vec(r.get("exdates")?),
        reminders: json_vec(r.get("reminders")?),
        color: r.get("color")?,
        go: r.get("go")?,
        source: r.get("source")?,
        source_id: r.get("source_id")?,
        overrides: json_vec::<Override>(r.get("overrides")?),
        off: r.get::<_, i64>("off")? != 0,
    })
}

pub fn events_all(c: &Connection) -> R<Vec<Event>> {
    let mut st = c.prepare("SELECT * FROM events ORDER BY date, time").map_err(err)?;
    let rows = st.query_map([], event_of).map_err(err)?;
    Ok(rows.flatten().filter_map(|mut e| e.sanitize().then_some(e)).collect())
}

pub fn event_get(c: &Connection, id: &str) -> Option<Event> {
    let mut e = c.query_row("SELECT * FROM events WHERE id=?1", [id], event_of).optional().ok().flatten()?;
    e.sanitize().then_some(e)
}

pub fn event_put(c: &Connection, e: &Event) -> R<()> {
    event_put_at(c, e, now())
}

/// Stores an event within safe limits; an unusable one (bad date) is skipped.
pub fn event_put_at(c: &Connection, e: &Event, t: i64) -> R<()> {
    let mut clean = e.clone();
    if !clean.sanitize() {
        log::warn!("evento ignorado (dados inválidos): {}", e.id.chars().take(60).collect::<String>());
        return Ok(());
    }
    let e = &clean;
    c.execute(
        "INSERT INTO events(id,kind,title,jp,notes,all_day,date,time,dur,rrule,exdates,reminders,color,go,source,source_id,overrides,off,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?19)
         ON CONFLICT(id) DO UPDATE SET kind=excluded.kind, title=excluded.title, jp=excluded.jp, notes=excluded.notes,
           all_day=excluded.all_day, date=excluded.date, time=excluded.time, dur=excluded.dur, rrule=excluded.rrule,
           exdates=excluded.exdates, reminders=excluded.reminders, color=excluded.color, go=excluded.go,
           source=excluded.source, source_id=excluded.source_id, overrides=excluded.overrides, off=excluded.off,
           updated_at=excluded.updated_at",
        params![
            e.id,
            e.kind.css(),
            e.title,
            e.jp,
            e.notes,
            e.all_day as i64,
            e.date,
            e.time,
            e.dur,
            e.rrule,
            serde_json::to_string(&e.exdates).unwrap_or_else(|_| "[]".into()),
            serde_json::to_string(&e.reminders).unwrap_or_else(|_| "[]".into()),
            e.color,
            e.go,
            e.source,
            e.source_id,
            serde_json::to_string(&e.overrides).unwrap_or_else(|_| "[]".into()),
            e.off as i64,
            t
        ],
    )
    .map_err(err)?;
    tomb_del(c, "event", &e.id)?;
    Ok(())
}

pub fn event_delete(c: &Connection, id: &str) -> R<()> {
    event_delete_at(c, id, now())
}

pub fn event_delete_at(c: &Connection, id: &str, t: i64) -> R<()> {
    c.execute("DELETE FROM events WHERE id=?1", [id]).map_err(err)?;
    c.execute("DELETE FROM occ_done WHERE event_id=?1", [id]).map_err(err)?;
    tomb_put(c, "event", id, t)?;
    Ok(())
}

pub fn occ_done_set(c: &Connection, event_id: &str, occ: &str, done: bool) -> R<()> {
    occ_done_set_at(c, event_id, occ, done, now())
}

pub fn occ_done_set_at(c: &Connection, event_id: &str, occ: &str, done: bool, t: i64) -> R<()> {
    let key = format!("{event_id}|{occ}");
    if done {
        c.execute("INSERT OR REPLACE INTO occ_done(event_id,occ,done_at) VALUES(?1,?2,?3)", params![event_id, occ, t])
            .map_err(err)?;
        tomb_del(c, "occ", &key)?;
    } else {
        c.execute("DELETE FROM occ_done WHERE event_id=?1 AND occ=?2", params![event_id, occ]).map_err(err)?;
        tomb_put(c, "occ", &key, t)?;
    }
    Ok(())
}

/* ---------------- tombstones (sync) ---------------- */

pub fn tomb_put(c: &Connection, kind: &str, id: &str, at: i64) -> R<()> {
    c.execute(
        "INSERT INTO tombstones(kind,id,at) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET at=excluded.at",
        params![kind, id, at],
    )
    .map_err(err)?;
    Ok(())
}

pub fn tomb_del(c: &Connection, kind: &str, id: &str) -> R<()> {
    c.execute("DELETE FROM tombstones WHERE kind=?1 AND id=?2", params![kind, id]).map_err(err)?;
    Ok(())
}

pub fn tombs(c: &Connection, kind: &str) -> R<Vec<(String, i64)>> {
    let mut st = c.prepare("SELECT id, at FROM tombstones WHERE kind=?1").map_err(err)?;
    let rows = st.query_map([kind], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(err)?;
    Ok(rows.flatten().collect())
}

pub fn occ_done_between(c: &Connection, from: &str, to: &str) -> R<HashSet<(String, String)>> {
    let mut st = c.prepare("SELECT event_id, occ FROM occ_done WHERE occ>=?1 AND occ<=?2").map_err(err)?;
    let rows =
        st.query_map(params![from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(err)?;
    Ok(rows.flatten().collect())
}

/* ---------------- notifications ---------------- */

fn aviso_of(r: &Row) -> rusqlite::Result<Aviso> {
    Ok(Aviso {
        id: r.get("id")?,
        event_id: r.get("event_id")?,
        occ: r.get("occ")?,
        title: r.get("title")?,
        jp: r.get("jp")?,
        body: r.get("body")?,
        kind: Kind::parse(&r.get::<_, String>("kind")?),
        color: r.get("color")?,
        go: r.get("go")?,
        at: r.get("at")?,
        dur: r.get("dur")?,
        fire_at: r.get::<_, i64>("fire_at")? as f64,
        status: r.get("status")?,
        snooze_until: r.get::<_, Option<i64>>("snooze_until")?.map(|x| x as f64),
        read: r.get::<_, i64>("read")? != 0,
    })
}

/// Inserts a reminder unless its key already fired. Returns the new id.
pub fn notif_insert(c: &Connection, key: &str, a: &Aviso) -> R<Option<i64>> {
    let n = c
        .execute(
            "INSERT OR IGNORE INTO notifications(key,event_id,occ,title,jp,body,kind,color,go,at,dur,fire_at,status,read)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0)",
            params![
                key,
                a.event_id,
                a.occ,
                a.title,
                a.jp,
                a.body,
                a.kind.css(),
                a.color,
                a.go,
                a.at,
                a.dur,
                a.fire_at as i64,
                a.status
            ],
        )
        .map_err(err)?;
    Ok(if n == 1 { Some(c.last_insert_rowid()) } else { None })
}

pub fn notif_key_exists(c: &Connection, key: &str) -> bool {
    c.query_row("SELECT 1 FROM notifications WHERE key=?1", [key], |_| Ok(())).optional().ok().flatten().is_some()
}

pub fn notif_get(c: &Connection, id: i64) -> Option<Aviso> {
    c.query_row("SELECT * FROM notifications WHERE id=?1", [id], aviso_of).optional().ok().flatten()
}

pub fn notif_set_status(c: &Connection, id: i64, status: &str, snooze_until: Option<i64>) -> R<()> {
    c.execute(
        "UPDATE notifications SET status=?2, snooze_until=?3, acted_at=?4 WHERE id=?1",
        params![id, status, snooze_until, now()],
    )
    .map_err(err)?;
    Ok(())
}

/// Reminders waiting in the popup.
pub fn notif_active(c: &Connection) -> R<Vec<Aviso>> {
    let mut st = c
        .prepare("SELECT * FROM notifications WHERE status='shown' ORDER BY fire_at DESC, id DESC LIMIT 20")
        .map_err(err)?;
    let rows = st.query_map([], aviso_of).map_err(err)?;
    Ok(rows.flatten().collect())
}

/// Snoozed reminders whose time has come.
pub fn notif_snooze_due(c: &Connection, now_ms: i64) -> R<Vec<Aviso>> {
    let mut st = c.prepare("SELECT * FROM notifications WHERE status='snoozed' AND snooze_until<=?1").map_err(err)?;
    let rows = st.query_map([now_ms], aviso_of).map_err(err)?;
    Ok(rows.flatten().collect())
}

pub fn notif_list(c: &Connection, limit: i64) -> R<Vec<Aviso>> {
    let mut st = c.prepare("SELECT * FROM notifications ORDER BY fire_at DESC, id DESC LIMIT ?1").map_err(err)?;
    let rows = st.query_map([limit], aviso_of).map_err(err)?;
    Ok(rows.flatten().collect())
}

pub fn notif_unread(c: &Connection) -> i64 {
    c.query_row("SELECT COUNT(*) FROM notifications WHERE read=0", [], |r| r.get(0)).unwrap_or(0)
}

pub fn notif_mark_all_read(c: &Connection) -> R<()> {
    c.execute("UPDATE notifications SET read=1 WHERE read=0", []).map_err(err)?;
    Ok(())
}

/// Clears history older than today (keeps snoozed/active ones).
pub fn notif_clear(c: &Connection) -> R<()> {
    c.execute("DELETE FROM notifications WHERE status NOT IN ('shown','snoozed')", []).map_err(err)?;
    Ok(())
}

/// Closes every active reminder of one occurrence (done from elsewhere).
pub fn notif_close_occ(c: &Connection, event_id: &str, occ: &str, status: &str) -> R<usize> {
    c.execute(
        "UPDATE notifications SET status=?3, acted_at=?4 WHERE event_id=?1 AND occ=?2 AND status IN ('shown','snoozed')",
        params![event_id, occ, status, now()],
    )
    .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn docs_days_cards_roundtrip() {
        let db = Db::memory();
        let c = db.lock();
        doc_put(&c, "profile", &json!({"name":"Mirage","xp":50})).unwrap();
        assert_eq!(profile(&c).xp, 50.0);
        day_put(&c, "2026-11-25", &json!({"k":"2026-11-25","xp":30,"sec":{"srs":1200}})).unwrap();
        let m: f64 = c.query_row("SELECT minutes FROM days WHERE day='2026-11-25'", [], |r| r.get(0)).unwrap();
        assert_eq!(m, 20.0);
        card_put(&c, "k:日", Some(&Card([1.0, 2.5, 2500.0, 3.0, 0.0, 2.0, 0.0]))).unwrap();
        assert_eq!(cards_all(&c).unwrap()[0].1.ivl(), 2.5);
        card_put(&c, "k:日", None).unwrap();
        assert!(cards_all(&c).unwrap().is_empty());
    }

    #[test]
    fn notification_fires_once() {
        let db = Db::memory();
        let c = db.lock();
        let a = Aviso {
            id: 0,
            event_id: "e".into(),
            occ: "2026-11-25".into(),
            title: "t".into(),
            jp: String::new(),
            body: String::new(),
            kind: Kind::Rotina,
            color: "#000".into(),
            go: None,
            at: "07:30".into(),
            dur: 15,
            fire_at: 1.0,
            status: "shown".into(),
            snooze_until: None,
            read: false,
        };
        assert!(notif_insert(&c, "e|2026-11-25|0", &a).unwrap().is_some());
        assert!(notif_insert(&c, "e|2026-11-25|0", &a).unwrap().is_none());
        assert_eq!(notif_active(&c).unwrap().len(), 1);
        assert_eq!(notif_close_occ(&c, "e", "2026-11-25", "done").unwrap(), 1);
        assert!(notif_active(&c).unwrap().is_empty());
    }
}
