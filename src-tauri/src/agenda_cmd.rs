//! Agenda editing commands: save/delete events, move or skip one
//! occurrence, and the .ics export.

use crate::db::{self, err, Db, R};
use crate::events;
use crate::scheduler::Scheduler;
use chrono::Duration;
use michi_core::agenda::{Event, Override};
use michi_core::time::{key_of, now_ms, parse_key};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub const CHANGED: &str = "michi://agenda-changed";

fn changed(app: &AppHandle) {
    let _ = app.emit(CHANGED, ());
    if let Some(s) = app.try_state::<Scheduler>() {
        s.wake();
    }
    events::avisos_changed(app);
    crate::tray::refresh(app);
}

#[tauri::command]
pub fn event_get(db: State<'_, Db>, id: String) -> Option<Event> {
    db::event_get(&db.lock(), &id)
}

#[tauri::command]
pub fn event_save(app: AppHandle, db: State<'_, Db>, mut event: Event) -> R<Event> {
    if event.title.trim().is_empty() {
        return Err("dê um título ao evento".into());
    }
    if parse_key(&event.date).is_none() {
        return Err("data inválida".into());
    }
    if event.id.is_empty() {
        event.id = format!("ev-{}", now_ms() as u64);
        event.source = "user".into();
    }
    event.dur = event.dur.max(if event.all_day { 1440 } else { 5 });
    event.reminders.sort_unstable();
    event.reminders.dedup();
    db::event_put(&db.lock(), &event)?;
    changed(&app);
    Ok(event)
}

#[tauri::command]
pub fn event_delete(app: AppHandle, db: State<'_, Db>, id: String) -> R<()> {
    {
        let c = db.lock();
        let e = db::event_get(&c, &id).ok_or("evento não encontrado")?;
        if e.source != "user" {
            return Err("eventos da rotina e da viagem não se apagam por aqui".into());
        }
        db::event_delete(&c, &id)?;
        c.execute(
            "UPDATE notifications SET status='dismissed' WHERE event_id=?1 AND status IN ('shown','snoozed')",
            [&id],
        )
        .map_err(err)?;
    }
    changed(&app);
    Ok(())
}

/// Skips one occurrence (EXDATE).
#[tauri::command]
pub fn occ_skip(app: AppHandle, db: State<'_, Db>, event_id: String, occ: String) -> R<()> {
    {
        let c = db.lock();
        let mut e = db::event_get(&c, &event_id).ok_or("evento não encontrado")?;
        if !e.exdates.contains(&occ) {
            e.exdates.push(occ.clone());
        }
        e.overrides.retain(|o| o.occ != occ);
        db::event_put(&c, &e)?;
        db::notif_close_occ(&c, &event_id, &occ, "dismissed")?;
    }
    changed(&app);
    Ok(())
}

/// Moves (or resizes) one occurrence; back to the series' slot removes the override.
#[tauri::command]
pub fn occ_move(
    app: AppHandle,
    db: State<'_, Db>,
    event_id: String,
    occ: String,
    date: String,
    time: String,
    dur: i64,
) -> R<()> {
    if parse_key(&date).is_none() {
        return Err("data inválida".into());
    }
    {
        let c = db.lock();
        let mut e = db::event_get(&c, &event_id).ok_or("evento não encontrado")?;
        e.overrides.retain(|o| o.occ != occ);
        let same = date == occ && (e.all_day || time == e.time) && dur == e.dur;
        if !same {
            e.overrides.push(Override { occ: occ.clone(), date, time, dur: dur.max(5) });
        }
        db::event_put(&c, &e)?;
    }
    changed(&app);
    Ok(())
}

/* ---------------- .ics ---------------- */

fn ics_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n")
}

fn dt(date: &str, time: &str) -> String {
    format!("{}T{}00", date.replace('-', ""), time.replace(':', ""))
}

/// Folds lines longer than 75 octets (RFC 5545).
fn fold(line: &str) -> String {
    let mut out = String::new();
    let mut n = 0;
    for ch in line.chars() {
        let l = ch.len_utf8();
        if n + l > 74 {
            out.push_str("\r\n ");
            n = 1;
        }
        out.push(ch);
        n += l;
    }
    out
}

pub fn ics(events: &[Event]) -> String {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let mut l: Vec<String> = vec![
        "BEGIN:VCALENDAR".into(),
        "VERSION:2.0".into(),
        "PRODID:-//Nihongo Michi//PT-BR".into(),
        "CALSCALE:GREGORIAN".into(),
    ];
    for e in events.iter().filter(|e| !e.off) {
        let uid = format!("UID:{}@nihongo-michi", e.id);
        let summary = format!(
            "SUMMARY:{}",
            ics_text(&if e.jp.is_empty() { e.title.clone() } else { format!("{} {}", e.jp, e.title) })
        );
        let start_lines = |date: &str, time: &str, dur: i64| -> Vec<String> {
            if e.all_day {
                let end = parse_key(date)
                    .map(|d| key_of(d + Duration::days(((dur.max(1440) + 1439) / 1440).max(1))))
                    .unwrap_or_default();
                vec![
                    format!("DTSTART;VALUE=DATE:{}", date.replace('-', "")),
                    format!("DTEND;VALUE=DATE:{}", end.replace('-', "")),
                ]
            } else {
                vec![format!("DTSTART:{}", dt(date, time)), format!("DURATION:PT{}M", dur.max(1))]
            }
        };
        let alarms: Vec<String> = e
            .reminders
            .iter()
            .flat_map(|m| {
                vec![
                    "BEGIN:VALARM".into(),
                    if e.all_day { format!("TRIGGER:PT{}M", 9 * 60 - m) } else { format!("TRIGGER:-PT{m}M") },
                    "ACTION:DISPLAY".into(),
                    format!("DESCRIPTION:{}", ics_text(&e.title)),
                    "END:VALARM".into(),
                ]
            })
            .collect();
        l.push("BEGIN:VEVENT".into());
        l.push(uid.clone());
        l.push(format!("DTSTAMP:{stamp}"));
        l.extend(start_lines(&e.date, &e.time, e.dur));
        if let Some(r) = e.rule() {
            l.push(format!("RRULE:{r}"));
        }
        for x in &e.exdates {
            l.push(if e.all_day {
                format!("EXDATE;VALUE=DATE:{}", x.replace('-', ""))
            } else {
                format!("EXDATE:{}", dt(x, &e.time))
            });
        }
        l.push(summary.clone());
        if !e.notes.is_empty() {
            l.push(format!("DESCRIPTION:{}", ics_text(&e.notes)));
        }
        l.extend(alarms.clone());
        l.push("END:VEVENT".into());
        for o in &e.overrides {
            l.push("BEGIN:VEVENT".into());
            l.push(uid.clone());
            l.push(format!("DTSTAMP:{stamp}"));
            l.push(if e.all_day {
                format!("RECURRENCE-ID;VALUE=DATE:{}", o.occ.replace('-', ""))
            } else {
                format!("RECURRENCE-ID:{}", dt(&o.occ, &e.time))
            });
            l.extend(start_lines(&o.date, &o.time, o.dur));
            l.push(summary.clone());
            l.extend(alarms.clone());
            l.push("END:VEVENT".into());
        }
    }
    l.push("END:VCALENDAR".into());
    l.into_iter().map(|x| fold(&x)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

#[tauri::command]
pub async fn agenda_export_ics(app: AppHandle, db: State<'_, Db>) -> R<Option<String>> {
    let text = ics(&db::events_all(&db.lock())?);
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("Calendário", &["ics"])
        .set_file_name("nihongo-michi-agenda.ics")
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(err)?;
    std::fs::write(&path, text).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::sync_events;

    #[test]
    fn ics_has_series_skips_and_moves() {
        let db = Db::memory();
        let c = db.lock();
        sync_events(&c).unwrap();
        let mut e = db::event_get(&c, "rotina-srs").unwrap();
        e.exdates.push(e.date.clone());
        e.overrides.push(Override {
            occ: "2030-01-02".into(),
            date: "2030-01-02".into(),
            time: "09:00".into(),
            dur: 20,
        });
        db::event_put(&c, &e).unwrap();
        let t = ics(&db::events_all(&c).unwrap());
        assert!(t.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(t.contains("RRULE:FREQ=DAILY"));
        assert!(t.contains("EXDATE:"));
        assert!(t.contains("RECURRENCE-ID:20300102T073000"));
        assert!(t.contains("DTSTART;VALUE=DATE:20281201"));
        assert!(t.lines().all(|l| l.len() <= 76));
    }
}
