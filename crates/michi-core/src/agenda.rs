//! The in-app agenda: events with recurrence, per-occurrence moves and
//! skips, reminders, and the notification records shared by the backend
//! scheduler, the main window and the reminder popup.

use crate::de;
use crate::routine::Block;
use crate::rrule::RRule;
use crate::time::{key_of, parse_hhmm, parse_key};
use crate::trip;
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Rotina,
    Estudo,
    Vlog,
    Viagem,
    Meu,
}

impl Kind {
    pub const ALL: [Kind; 5] = [Kind::Rotina, Kind::Estudo, Kind::Vlog, Kind::Viagem, Kind::Meu];

    pub fn label(&self) -> &'static str {
        match self {
            Kind::Rotina => "Rotina",
            Kind::Estudo => "Estudo",
            Kind::Vlog => "Vlog",
            Kind::Viagem => "Viagem",
            Kind::Meu => "Meus eventos",
        }
    }

    pub fn color(&self) -> &'static str {
        match self {
            Kind::Rotina => "#0a9a8f",
            Kind::Estudo => "#0a72c0",
            Kind::Vlog => "#d6247c",
            Kind::Viagem => "#d7262e",
            Kind::Meu => "#8248a8",
        }
    }

    pub fn css(&self) -> &'static str {
        match self {
            Kind::Rotina => "rotina",
            Kind::Estudo => "estudo",
            Kind::Vlog => "vlog",
            Kind::Viagem => "viagem",
            Kind::Meu => "meu",
        }
    }

    pub fn parse(s: &str) -> Kind {
        match s {
            "rotina" => Kind::Rotina,
            "estudo" => Kind::Estudo,
            "vlog" => Kind::Vlog,
            "viagem" => Kind::Viagem,
            _ => Kind::Meu,
        }
    }
}

/// One occurrence of a series moved to another date/time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Override {
    /// Original date of the occurrence ("YYYY-MM-DD").
    pub occ: String,
    pub date: String,
    #[serde(default)]
    pub time: String,
    pub dur: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    /// Short Japanese label (routine blocks: 復習, 遊び…).
    #[serde(default)]
    pub jp: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default, deserialize_with = "de::bool_any")]
    pub all_day: bool,
    /// First date of the series, "YYYY-MM-DD".
    pub date: String,
    /// "HH:MM" (ignored for all-day events).
    #[serde(default)]
    pub time: String,
    /// Duration in minutes (all-day: days × 1440).
    pub dur: i64,
    #[serde(default)]
    pub rrule: Option<String>,
    /// Skipped occurrences (original dates).
    #[serde(default)]
    pub exdates: Vec<String>,
    /// Minutes before the start to remind (0 = at the start time).
    #[serde(default)]
    pub reminders: Vec<i64>,
    #[serde(default)]
    pub color: Option<String>,
    /// Tab opened by "Abrir" (hoje, revisar, jornada…).
    #[serde(default)]
    pub go: Option<String>,
    /// user | rotina | viagem
    #[serde(default = "user_source")]
    pub source: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub overrides: Vec<Override>,
    /// Routine block switched off: kept for history, hidden and silent.
    #[serde(default, deserialize_with = "de::bool_any")]
    pub off: bool,
}

fn user_source() -> String {
    "user".into()
}

/// Longest event (a year) and earliest reminder (a week before): far from
/// chrono's limits, so no date arithmetic on an event can overflow.
pub const MAX_DUR_MIN: i64 = 366 * 1440;
pub const MAX_REMIND_MIN: i64 = 7 * 1440;
const MAX_LIST: usize = 2000;

/// `#rgb`, `#rrggbb` or `#rrggbbaa` only: colors end up in style attributes.
pub fn safe_color(c: &str) -> bool {
    let h = c.strip_prefix('#').unwrap_or("");
    matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|x| x.is_ascii_hexdigit())
}

fn sane_date(s: &str) -> bool {
    parse_key(s).map(|d| (1970..=2200).contains(&d.year())).unwrap_or(false)
}

fn clip(s: &mut String, max: usize) {
    if s.chars().count() > max {
        *s = s.chars().take(max).collect();
    }
}

/// A concrete occurrence, ready for the calendar views.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occ {
    pub event_id: String,
    /// Original date of the occurrence (identity for done/skip/move).
    pub occ: String,
    /// Actual date after a move.
    pub date: String,
    pub time: String,
    pub dur: i64,
    pub all_day: bool,
    pub title: String,
    pub jp: String,
    pub notes: String,
    pub kind: Kind,
    pub color: String,
    pub go: Option<String>,
    pub source: String,
    pub source_id: Option<String>,
    pub recurring: bool,
    pub moved: bool,
    pub reminders: Vec<i64>,
    #[serde(default)]
    pub done: bool,
}

impl Occ {
    pub fn key(&self) -> String {
        format!("{}@{}", self.event_id, self.occ)
    }
    pub fn start(&self) -> NaiveDateTime {
        let d = parse_key(&self.date).unwrap_or_default();
        let t = if self.all_day { NaiveTime::MIN } else { parse_hhmm(&self.time).unwrap_or(NaiveTime::MIN) };
        d.and_time(t)
    }
    pub fn end(&self) -> NaiveDateTime {
        self.start() + Duration::minutes(self.dur.max(1))
    }
    /// Agenda credit key in the day document.
    pub fn ag_key(&self) -> String {
        if self.source == "rotina" {
            format!("rotina:{}", self.source_id.clone().unwrap_or_default())
        } else {
            self.key()
        }
    }
    /// Routine block id, when this occurrence is a routine block.
    pub fn block_id(&self) -> Option<&str> {
        if self.source == "rotina" {
            self.source_id.as_deref()
        } else {
            None
        }
    }
}

impl Event {
    /// Keeps the event inside safe limits. Events also arrive from backups
    /// and from other PCs' sync files, so every stored event goes through
    /// this. Returns false when the event is unusable (no id, bad date).
    pub fn sanitize(&mut self) -> bool {
        if self.id.trim().is_empty() || self.id.len() > 200 || !sane_date(&self.date) {
            return false;
        }
        clip(&mut self.title, 300);
        clip(&mut self.jp, 60);
        clip(&mut self.notes, 5000);
        self.dur = self.dur.clamp(0, MAX_DUR_MIN);
        if !self.time.is_empty() && parse_hhmm(&self.time).is_none() {
            self.time = "09:00".into();
        }
        self.reminders.retain(|m| (0..=MAX_REMIND_MIN).contains(m));
        self.reminders.sort_unstable();
        self.reminders.dedup();
        self.reminders.truncate(12);
        self.exdates.retain(|d| sane_date(d));
        self.exdates.truncate(MAX_LIST);
        self.overrides.retain(|o| sane_date(&o.occ) && sane_date(&o.date));
        for o in &mut self.overrides {
            o.dur = o.dur.clamp(0, MAX_DUR_MIN);
            if !o.time.is_empty() && parse_hhmm(&o.time).is_none() {
                o.time.clear();
            }
        }
        self.overrides.truncate(MAX_LIST);
        if self.rrule.as_deref().map(|r| RRule::parse(r).is_none()).unwrap_or(false) {
            self.rrule = None;
        }
        if self.color.as_deref().map(|c| !safe_color(c)).unwrap_or(false) {
            self.color = None;
        }
        if let Some(g) = &mut self.go {
            clip(g, 40);
        }
        true
    }

    pub fn rule(&self) -> Option<RRule> {
        self.rrule.as_deref().and_then(RRule::parse)
    }

    pub fn start_date(&self) -> NaiveDate {
        parse_key(&self.date).unwrap_or_default()
    }

    pub fn color(&self) -> String {
        self.color.clone().filter(|c| !c.is_empty()).unwrap_or_else(|| self.kind.color().to_string())
    }

    fn span_days(&self) -> i64 {
        (self.dur.max(1) + 1439) / 1440
    }

    /// Occurrences overlapping `[from, to]` (inclusive dates).
    pub fn expand(&self, from: NaiveDate, to: NaiveDate) -> Vec<Occ> {
        let start = self.start_date();
        let span = self.span_days();
        // Wider window: moved occurrences can come from (or go to) nearby dates.
        let wfrom = from - Duration::days(span + 31);
        let wto = to + Duration::days(31);
        let dates: Vec<NaiveDate> = match self.rule() {
            Some(r) => r.dates(start, wfrom, wto),
            None => {
                if start >= wfrom && start <= wto {
                    vec![start]
                } else {
                    vec![]
                }
            }
        };
        let recurring = self.rule().is_some();
        let mut out = Vec::new();
        for d in dates {
            let occ = key_of(d);
            if self.exdates.iter().any(|x| x == &occ) {
                continue;
            }
            let ov = self.overrides.iter().find(|o| o.occ == occ);
            let (date, time, dur) = match ov {
                Some(o) => (o.date.clone(), o.time.clone(), o.dur),
                None => (occ.clone(), self.time.clone(), self.dur),
            };
            let Some(ad) = parse_key(&date) else { continue };
            let last = ad + Duration::days(if self.all_day { (dur.max(1) + 1439) / 1440 - 1 } else { 0 });
            if last < from || ad > to {
                continue;
            }
            out.push(Occ {
                event_id: self.id.clone(),
                occ,
                date,
                time,
                dur,
                all_day: self.all_day,
                title: self.title.clone(),
                jp: self.jp.clone(),
                notes: self.notes.clone(),
                kind: self.kind,
                color: self.color(),
                go: self.go.clone(),
                source: self.source.clone(),
                source_id: self.source_id.clone(),
                recurring,
                moved: ov.is_some(),
                reminders: self.reminders.clone(),
                done: false,
            });
        }
        out
    }
}

/// Recurring event of a routine block. `anchor` is the first date of the
/// series (kept from the existing event so history stays put).
pub fn routine_event(b: &Block, anchor: &str, remind_before: i64) -> Event {
    Event {
        id: format!("rotina-{}", b.id),
        kind: if b.id == "vlog" { Kind::Vlog } else { Kind::Rotina },
        title: b.name.clone(),
        jp: b.jp.clone(),
        notes: b.what.clone(),
        all_day: false,
        date: anchor.to_string(),
        time: b.time.clone(),
        dur: (b.min.round() as i64).max(5),
        rrule: Some(RRule::for_js_days(&b.days_of()).to_string()),
        exdates: vec![],
        reminders: vec![remind_before],
        color: None,
        go: Some(if b.go.is_empty() { "hoje".into() } else { b.go.clone() }),
        source: "rotina".into(),
        source_id: Some(b.id.clone()),
        overrides: vec![],
        off: b.off,
    }
}

/// All-day events of the trip itinerary, starting on `trip_start`, limited
/// to the trip length.
pub fn trip_events(trip_start: &str, trip_end: &str) -> Vec<Event> {
    let Some(s) = parse_key(trip_start) else { return vec![] };
    let len = parse_key(trip_end).map(|e| (e - s).num_days() + 1).unwrap_or(29).max(1);
    trip::itinerary()
        .into_iter()
        .filter(|d| (d.day as i64) <= len)
        .map(|d| Event {
            id: format!("viagem-d{:02}", d.day),
            kind: Kind::Viagem,
            title: format!("Dia {} · {} — {}", d.day, d.city, d.title),
            jp: trip::city_jp(&d.city).to_string(),
            notes: if d.st.is_empty() {
                String::new()
            } else {
                format!("Treinar na Linha Viagem: {}", d.st.join(", "))
            },
            all_day: true,
            date: key_of(s + Duration::days(d.day as i64 - 1)),
            time: String::new(),
            dur: 1440,
            rrule: None,
            exdates: vec![],
            reminders: vec![],
            color: Some(trip::city_color(&d.city).to_string()),
            go: Some("jornada".into()),
            source: "viagem".into(),
            source_id: Some(format!("d{:02}", d.day)),
            overrides: vec![],
            off: false,
        })
        .collect()
}

/// A reminder record (notification centre + popup).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aviso {
    pub id: i64,
    pub event_id: String,
    pub occ: String,
    pub title: String,
    pub jp: String,
    pub body: String,
    pub kind: Kind,
    pub color: String,
    pub go: Option<String>,
    /// "HH:MM" of the occurrence (empty for all-day / summaries).
    pub at: String,
    pub dur: i64,
    /// When it fired (ms).
    pub fire_at: f64,
    /// shown | done | snoozed | dismissed | missed | silenced | opened
    pub status: String,
    pub snooze_until: Option<f64>,
    pub read: bool,
}

/// Desktop-only settings (document `desk`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeskSettings {
    #[serde(deserialize_with = "de::bool_any")]
    pub reminders_on: bool,
    #[serde(deserialize_with = "de::bool_any")]
    pub quiet_on: bool,
    #[serde(deserialize_with = "de::string_any")]
    pub quiet_start: String,
    #[serde(deserialize_with = "de::string_any")]
    pub quiet_end: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub snooze_min: f64,
    #[serde(deserialize_with = "de::bool_any")]
    pub sound: bool,
    #[serde(deserialize_with = "de::f64_any")]
    pub volume: f64,
    /// Reminders paused until this instant (ms), from the tray.
    #[serde(deserialize_with = "de::f64_any")]
    pub pause_until: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<f64>,
}

impl Default for DeskSettings {
    fn default() -> Self {
        DeskSettings {
            reminders_on: true,
            quiet_on: true,
            quiet_start: "23:00".into(),
            quiet_end: "07:00".into(),
            snooze_min: 10.0,
            sound: true,
            volume: 0.7,
            pause_until: 0.0,
            u: None,
        }
    }
}

impl DeskSettings {
    /// Is `t` inside the quiet hours? The span may cross midnight.
    pub fn is_quiet(&self, t: NaiveTime) -> bool {
        if !self.quiet_on {
            return false;
        }
        let (Some(a), Some(b)) = (parse_hhmm(&self.quiet_start), parse_hhmm(&self.quiet_end)) else {
            return false;
        };
        if a == b {
            return false;
        }
        if a < b {
            t >= a && t < b
        } else {
            t >= a || t < b
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routine::routine_default;

    fn d(s: &str) -> NaiveDate {
        parse_key(s).unwrap()
    }

    #[test]
    fn routine_series_with_skip_and_move() {
        let blocks = routine_default();
        let mut e = routine_event(&blocks[0], "2026-11-23", 5);
        assert_eq!(e.rrule.as_deref(), Some("FREQ=DAILY"));
        e.exdates.push("2026-11-25".into());
        e.overrides.push(Override {
            occ: "2026-11-26".into(),
            date: "2026-11-26".into(),
            time: "09:00".into(),
            dur: 20,
        });
        let v = e.expand(d("2026-11-24"), d("2026-11-27"));
        assert_eq!(v.iter().map(|o| o.date.as_str()).collect::<Vec<_>>(), ["2026-11-24", "2026-11-26", "2026-11-27"]);
        assert_eq!(v[1].time, "09:00");
        assert!(v[1].moved && !v[0].moved);
        assert_eq!(v[0].ag_key(), "rotina:srs");
        assert_eq!(v[0].block_id(), Some("srs"));
    }

    #[test]
    fn moved_into_window_from_outside() {
        let mut e = routine_event(&routine_default()[1], "2026-11-01", 0);
        e.overrides.push(Override {
            occ: "2026-11-30".into(),
            date: "2026-12-02".into(),
            time: "13:00".into(),
            dur: 10,
        });
        let v = e.expand(d("2026-12-02"), d("2026-12-02"));
        assert_eq!(v.len(), 2);
        assert!(v.iter().any(|o| o.occ == "2026-11-30" && o.time == "13:00"));
    }

    #[test]
    fn trip_days_follow_dates() {
        let v = trip_events("2028-12-01", "2028-12-29");
        assert_eq!(v.len(), 29);
        assert_eq!(v[28].date, "2028-12-29");
        assert!(v[0].title.contains("Tokyo"));
        let short = trip_events("2028-12-01", "2028-12-10");
        assert_eq!(short.len(), 10);
        let occ = v[0].expand(d("2028-11-27"), d("2028-12-03"));
        assert_eq!(occ.len(), 1);
        assert!(occ[0].all_day);
    }

    #[test]
    fn quiet_hours_cross_midnight() {
        let s = DeskSettings::default();
        assert!(s.is_quiet(NaiveTime::from_hms_opt(23, 30, 0).unwrap()));
        assert!(s.is_quiet(NaiveTime::from_hms_opt(6, 59, 0).unwrap()));
        assert!(!s.is_quiet(NaiveTime::from_hms_opt(7, 0, 0).unwrap()));
        assert!(!s.is_quiet(NaiveTime::from_hms_opt(21, 30, 0).unwrap()));
    }
}

#[cfg(test)]
mod limits_tests {
    use super::*;
    use crate::Card;

    #[test]
    fn absurd_events_are_tamed_not_fatal() {
        let j = r##"{"id":"x","kind":"meu","title":"t","date":"2026-11-26","time":"25:99","dur":1000000000000,
            "rrule":"FREQ=DAILY;INTERVAL=4000000000;COUNT=99999999","reminders":[-5,10,999999999999],
            "exdates":["99999-01-01","2026-11-27"],"color":"red;background:url(https://x.y/z)",
            "overrides":[{"occ":"2026-11-26","date":"2026-11-26","time":"09:00","dur":9000000000000}]}"##;
        let mut e: Event = serde_json::from_str(j).unwrap();
        assert!(e.sanitize());
        assert_eq!(e.dur, MAX_DUR_MIN);
        assert_eq!(e.time, "09:00");
        assert_eq!(e.reminders, vec![10]);
        assert_eq!(e.exdates, vec!["2026-11-27".to_string()]);
        assert!(e.color.is_none(), "only #hex colors reach style attributes");
        assert_eq!(e.overrides[0].dur, MAX_DUR_MIN);
        let r = e.rule().unwrap();
        assert_eq!((r.interval, r.count), (1000, Some(10_000)));
        // Expanding over a wide range neither panics nor runs away.
        let from = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2030, 12, 31).unwrap();
        let _ = e.expand(from, to);

        let mut bad: Event =
            serde_json::from_str(r#"{"id":"y","kind":"meu","title":"t","date":"30000-01-01","dur":5}"#).unwrap();
        assert!(!bad.sanitize(), "a date outside 1970–2200 is refused");
        assert!(safe_color("#0a9a8f") && safe_color("#fff") && !safe_color("url(x)") && !safe_color("#12345"));
    }

    #[test]
    fn absurd_cards_are_tamed() {
        let mut c = Card([1e300, 1e15, 1.0, -4.0, 7.0, 9.0, 1e9]);
        assert!(c.sanitize());
        assert_eq!(c.0, [7.3e12, 36_500.0, 1300.0, 0.0, 7.0, 3.0, 10.0]);
        assert!(!Card([f64::NAN, 0.0, 2500.0, 0.0, 0.0, 0.0, 0.0]).sanitize());
        // Scheduling a card with a huge interval does not overflow the date.
        let next = crate::srs::schedule(Some(&Card([0.0, 36_500.0, 2500.0, 3.0, 0.0, 2.0, 0.0])), 1, 1.7e12, 0.5);
        assert!(next.due().is_finite());
        assert_eq!(crate::time::key_add("2026-11-26", i64::MAX), crate::time::key_add("2026-11-26", 100_000));
    }
}
