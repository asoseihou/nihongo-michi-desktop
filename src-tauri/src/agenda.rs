//! Agenda queries and "done" marking shared by the calendar, the Today
//! board and the reminder popup.

use crate::db::{self, R};
use chrono::{Duration, NaiveDate};
use michi_core::agenda::Occ;
use michi_core::ops::{mark_ops, Op};
use michi_core::time::{key_of, parse_key, study_date_of};
use michi_core::DayDoc;
use rusqlite::Connection;
use std::collections::BTreeMap;

/// Study-day key an occurrence counts for (timed items before 04:00 belong
/// to the previous study day, like everything else in the app).
pub fn day_of(o: &Occ) -> String {
    if o.all_day {
        o.date.clone()
    } else {
        key_of(study_date_of(o.start()))
    }
}

/// Occurrences between two dates with their done flags.
pub fn occurrences(c: &Connection, from: NaiveDate, to: NaiveDate) -> R<Vec<Occ>> {
    let events = db::events_all(c)?;
    let done = db::occ_done_between(c, &key_of(from - Duration::days(40)), &key_of(to + Duration::days(40)))?;
    let mut days: BTreeMap<String, Option<DayDoc>> = BTreeMap::new();
    let mut out = Vec::new();
    for e in events.iter().filter(|e| !e.off) {
        for mut o in e.expand(from, to) {
            o.done = match o.block_id() {
                Some(b) => {
                    let k = day_of(&o);
                    let d = days.entry(k.clone()).or_insert_with(|| db::day_get(c, &k));
                    d.as_ref().map(|d| d.block_done(b)).unwrap_or(false)
                }
                None => done.contains(&(o.event_id.clone(), o.occ.clone())),
            };
            out.push(o);
        }
    }
    out.sort_by(|a, b| {
        (a.date.as_str(), !a.all_day, a.time.as_str()).cmp(&(b.date.as_str(), !b.all_day, b.time.as_str()))
    });
    Ok(out)
}

/// One occurrence by identity.
pub fn find_occ(c: &Connection, event_id: &str, occ: &str) -> R<Option<Occ>> {
    let Some(e) = db::event_get(c, event_id) else { return Ok(None) };
    let Some(d) = parse_key(occ) else { return Ok(None) };
    // A moved occurrence can sit up to a month away from its original date.
    Ok(e.expand(d - Duration::days(35), d + Duration::days(35)).into_iter().find(|o| o.occ == occ))
}

/// Applies operations to the stored documents.
pub fn apply_ops(c: &Connection, ops: &[Op]) -> R<()> {
    let mut days: BTreeMap<String, DayDoc> = BTreeMap::new();
    let mut profile = None;
    for op in ops {
        let d = days
            .entry(op.day().to_string())
            .or_insert_with(|| db::day_get(c, op.day()).unwrap_or_else(|| DayDoc::new(op.day())));
        op.apply_day(d);
        if op.touches_profile() {
            let p = profile.get_or_insert_with(|| db::profile(c));
            op.apply_profile(p);
        }
    }
    let now = michi_core::time::now_ms();
    for (k, mut d) in days {
        d.u = Some(now);
        db::day_put(c, &k, &serde_json::to_value(&d).map_err(db::err)?)?;
    }
    if let Some(mut p) = profile {
        p.u = Some(now);
        db::doc_put(c, "profile", &serde_json::to_value(&p).map_err(db::err)?)?;
    }
    Ok(())
}

/// Marks an occurrence done / not done: day document (Today board,
/// streak, heat map), the occurrence table and open reminders.
pub fn mark(c: &Connection, o: &Occ, done: bool) -> R<Vec<Op>> {
    let day = day_of(o);
    let minutes = if o.all_day { 0.0 } else { o.dur as f64 };
    let ops = mark_ops(&day, o.block_id(), &o.ag_key(), minutes, done);
    apply_ops(c, &ops)?;
    if o.block_id().is_none() {
        db::occ_done_set(c, &o.event_id, &o.occ, done)?;
    }
    if done {
        db::notif_close_occ(c, &o.event_id, &o.occ, "done")?;
    }
    Ok(ops)
}

/// After the UI saved a day document: routine reminders whose block is now
/// done (checked on the board, or done automatically) leave the popup.
pub fn close_done_routine(c: &Connection, day: &str) -> R<bool> {
    let Some(d) = db::day_get(c, day) else { return Ok(false) };
    let Some(date) = parse_key(day) else { return Ok(false) };
    let mut closed = false;
    for a in db::notif_active(c)?.into_iter().chain(db::notif_snooze_due(c, i64::MAX)?) {
        let Some(block) = a.event_id.strip_prefix("rotina-") else { continue };
        let Some(o) = find_occ(c, &a.event_id, &a.occ)? else { continue };
        if day_of(&o) == key_of(date) && d.block_done(block) {
            closed |= db::notif_close_occ(c, &a.event_id, &a.occ, "done")? > 0;
        }
    }
    Ok(closed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::sync::sync_events;
    use michi_core::time::today_key;

    #[test]
    fn marking_routine_feeds_day_and_profile() {
        let db = Db::memory();
        let c = db.lock();
        sync_events(&c).unwrap();
        let today = parse_key(&today_key()).unwrap();
        let occs = occurrences(&c, today, today).unwrap();
        let vlog = occs.iter().find(|o| o.event_id == "rotina-vlog").unwrap().clone();
        assert!(!vlog.done);
        mark(&c, &vlog, true).unwrap();
        let d = db::day_get(&c, &day_of(&vlog)).unwrap();
        assert!(d.block_done("vlog"));
        assert_eq!(d.xp, 10.0);
        assert_eq!(d.study_minutes(), 15.0);
        assert_eq!(db::profile(&c).xp, 10.0);
        let again = occurrences(&c, today, today).unwrap();
        assert!(again.iter().find(|o| o.event_id == "rotina-vlog").unwrap().done);
    }
}
