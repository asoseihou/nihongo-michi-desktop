//! Keeps derived agenda events in step with the profile: one recurring
//! event per routine block and one all-day event per trip day.

use crate::db::{self, R};
use michi_core::agenda::{routine_event, trip_events, Event};
use michi_core::routine::routine_of;
use michi_core::time::today_key;
use rusqlite::Connection;
use std::collections::HashSet;

/// Returns true when something changed.
pub fn sync_events(c: &Connection) -> R<bool> {
    let p = db::profile(c);
    let existing = db::events_all(c)?;
    let mut changed = false;
    let today = today_key();

    // Routine blocks → recurring events. Skips and moves survive edits.
    let mut keep: HashSet<String> = HashSet::new();
    for b in routine_of(&p.routine) {
        if b.id.is_empty() {
            continue;
        }
        let id = format!("rotina-{}", b.id);
        keep.insert(id.clone());
        let old = existing.iter().find(|e| e.id == id);
        let anchor = old.map(|e| e.date.clone()).unwrap_or_else(|| today.clone());
        let mut ev = routine_event(&b, &anchor, p.remind_before());
        if let Some(o) = old {
            ev.exdates = o.exdates.clone();
            ev.overrides = o.overrides.clone();
            if &ev == o {
                continue;
            }
        }
        db::event_put(c, &ev)?;
        changed = true;
    }
    for e in existing.iter().filter(|e| e.source == "rotina" && !keep.contains(&e.id)) {
        db::event_delete(c, &e.id)?;
        changed = true;
    }

    // Trip itinerary → all-day events (dates follow Ajustes → viagem).
    let (s, en) = p.trip_dates();
    let mut keep_trip: HashSet<String> = HashSet::new();
    for mut ev in trip_events(&s, &en) {
        keep_trip.insert(ev.id.clone());
        if let Some(o) = existing.iter().find(|e| e.id == ev.id) {
            keep_user_edits(&mut ev, o);
            if &ev == o {
                continue;
            }
        }
        db::event_put(c, &ev)?;
        changed = true;
    }
    for e in existing.iter().filter(|e| e.source == "viagem" && !keep_trip.contains(&e.id)) {
        db::event_delete(c, &e.id)?;
        changed = true;
    }
    Ok(changed)
}

/// Fields the learner may edit on a trip day without them being reset.
fn keep_user_edits(ev: &mut Event, old: &Event) {
    if !old.notes.is_empty() {
        ev.notes = old.notes.clone();
    }
    ev.reminders = old.reminders.clone();
    ev.exdates = old.exdates.clone();
    ev.overrides = old.overrides.clone();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use serde_json::json;

    #[test]
    fn routine_and_trip_events_follow_profile() {
        let db = Db::memory();
        let c = db.lock();
        assert!(sync_events(&c).unwrap());
        let all = db::events_all(&c).unwrap();
        assert_eq!(all.iter().filter(|e| e.source == "rotina").count(), 6);
        assert_eq!(all.iter().filter(|e| e.source == "viagem").count(), 29);
        assert!(!sync_events(&c).unwrap(), "second sync is a no-op");

        // Skip one occurrence, then change the block: the skip stays.
        let mut srs = db::event_get(&c, "rotina-srs").unwrap();
        srs.exdates.push("2030-01-01".into());
        db::event_put(&c, &srs).unwrap();
        db::doc_put(
            &c,
            "profile",
            &json!({"routine":[{"id":"srs","time":"08:15","min":20,"jp":"復習","name":"Revisões","go":"revisar","days":[1,2,3,4,5]}],
                    "tripStart":"2028-12-02","tripEnd":"2028-12-11"}),
        )
        .unwrap();
        assert!(sync_events(&c).unwrap());
        let srs = db::event_get(&c, "rotina-srs").unwrap();
        assert_eq!(srs.time, "08:15");
        assert_eq!(srs.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"));
        assert_eq!(srs.exdates, vec!["2030-01-01".to_string()]);
        let all = db::events_all(&c).unwrap();
        assert_eq!(all.iter().filter(|e| e.source == "rotina").count(), 1);
        assert_eq!(all.iter().filter(|e| e.source == "viagem").count(), 10);
        assert_eq!(db::event_get(&c, "viagem-d01").unwrap().date, "2028-12-02");
    }
}
