//! Study-day clock: a study day starts at 04:00 local time (port of
//! `dayKey`, `dayStart`, `addDaysStart`, `keyAdd`… from `01-core.js`).

use chrono::{Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike};

pub const CUTOFF_H: i64 = 4;
pub const DAY_MS: f64 = 86_400_000.0;
const HOUR_MS: f64 = 3_600_000.0;

pub const WD_JP: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];
pub const WD_PT: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
pub const MONTH_PT: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
pub const MONTH_PT_LONG: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// Milliseconds since the epoch, like `Date.now()`.
pub fn now_ms() -> f64 {
    Local::now().timestamp_millis() as f64
}

/// Local wall-clock time of an instant.
pub fn local(ms: f64) -> NaiveDateTime {
    Local.timestamp_millis_opt(ms as i64).earliest().map(|d| d.naive_local()).unwrap_or_default()
}

/// Instant of a local wall-clock time (DST gaps move forward an hour).
pub fn ms_of(dt: NaiveDateTime) -> f64 {
    match Local.from_local_datetime(&dt).earliest() {
        Some(t) => t.timestamp_millis() as f64,
        None => Local
            .from_local_datetime(&(dt + Duration::hours(1)))
            .earliest()
            .map(|t| t.timestamp_millis() as f64)
            .unwrap_or(0.0),
    }
}

pub fn now_local() -> NaiveDateTime {
    Local::now().naive_local()
}

/// Study date of an instant (the day changes at 04:00).
pub fn study_date(ms: f64) -> NaiveDate {
    local(ms - CUTOFF_H as f64 * HOUR_MS).date()
}

/// Study date of a local wall-clock time.
pub fn study_date_of(dt: NaiveDateTime) -> NaiveDate {
    (dt - Duration::hours(CUTOFF_H)).date()
}

pub fn key_of(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn parse_key(k: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(k.get(..10).unwrap_or(k), "%Y-%m-%d").ok()
}

/// `dayKey(t)`.
pub fn day_key(ms: f64) -> String {
    key_of(study_date(ms))
}

pub fn today_key() -> String {
    day_key(now_ms())
}

/// `dayStart(t)`: 04:00 of the study day containing `t`.
pub fn day_start(ms: f64) -> f64 {
    ms_of(study_date(ms).and_time(cutoff_time()))
}

fn cutoff_time() -> NaiveTime {
    NaiveTime::from_hms_opt(CUTOFF_H as u32, 0, 0).unwrap()
}

/// `addDaysStart(n, t)`.
pub fn add_days_start(n: i64, ms: f64) -> f64 {
    // A card imported with an absurd interval must not overflow the date.
    let d = study_date(ms);
    let d = d.checked_add_signed(Duration::days(n.clamp(-100_000, 100_000))).unwrap_or(d);
    ms_of(d.and_time(cutoff_time()))
}

/// `keyAdd(k, n)`.
pub fn key_add(k: &str, n: i64) -> String {
    match parse_key(k) {
        Some(d) => key_of(d.checked_add_signed(Duration::days(n.clamp(-100_000, 100_000))).unwrap_or(d)),
        None => k.to_string(),
    }
}

/// Day of week of the current study day, 0 = Sunday (JS `getDay`).
pub fn study_dow(ms: f64) -> u8 {
    study_date(ms).weekday().num_days_from_sunday() as u8
}

pub fn dow(d: NaiveDate) -> u8 {
    d.weekday().num_days_from_sunday() as u8
}

/// `jpDate`: 2026年11月25日（水）.
pub fn jp_date(ms: f64) -> String {
    let d = local(ms);
    format!("{}年{}月{}日（{}）", d.year(), d.month(), d.day(), WD_JP[d.weekday().num_days_from_sunday() as usize])
}

/// `hhmm`.
pub fn hhmm(ms: f64) -> String {
    let d = local(ms);
    format!("{:02}:{:02}", d.hour(), d.minute())
}

/// Minutes since midnight of "HH:MM" (`minutesOf`).
pub fn minutes_of(s: &str) -> i64 {
    let mut it = s.split(':');
    let a: i64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    let b: i64 = it.next().and_then(|x| x.trim().parse().ok()).unwrap_or(0);
    a * 60 + b
}

pub fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

/// `fmtIvl(days)`: 10min, 3h, 4d, 1.5m, 2a.
pub fn fmt_ivl(days: f64) -> String {
    if days < 1.0 {
        let m = (days * 1440.0).round().max(1.0);
        return if m < 60.0 { format!("{}min", m) } else { format!("{}h", (m / 60.0).round()) };
    }
    if days < 30.0 {
        return format!("{}d", days.round());
    }
    if days < 365.0 {
        return format!("{}m", trim1(days / 30.0));
    }
    format!("{}a", trim1(days / 365.0))
}

fn trim1(x: f64) -> String {
    let s = format!("{:.1}", x);
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

pub fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{} {}", n, if n == 1 { one } else { many })
}

/// Greeting by hour (`greeting()` in `04-app.js`).
pub fn greeting(ms: f64) -> &'static str {
    let h = local(ms).hour();
    if (5..11).contains(&h) {
        "おはよう"
    } else if (11..18).contains(&h) {
        "こんにちは"
    } else {
        "こんばんは"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_and_ivl() {
        assert_eq!(minutes_of("07:30"), 450);
        assert_eq!(minutes_of("21:05"), 1265);
        assert_eq!(fmt_ivl(10.0 / 1440.0), "10min");
        assert_eq!(fmt_ivl(0.5), "12h");
        assert_eq!(fmt_ivl(4.0), "4d");
        assert_eq!(fmt_ivl(45.0), "1.5m");
        assert_eq!(fmt_ivl(60.0), "2m");
        assert_eq!(fmt_ivl(730.0), "2a");
    }

    #[test]
    fn cutoff() {
        let d = NaiveDate::from_ymd_opt(2026, 11, 25).unwrap();
        let at_3 = ms_of(d.and_hms_opt(3, 59, 0).unwrap());
        let at_4 = ms_of(d.and_hms_opt(4, 0, 0).unwrap());
        assert_eq!(day_key(at_3), "2026-11-24");
        assert_eq!(day_key(at_4), "2026-11-25");
        assert_eq!(day_start(at_4 + 5.0 * 3_600_000.0), at_4);
        assert_eq!(key_add("2026-12-31", 1), "2027-01-01");
        assert_eq!(key_add("2026-03-01", -1), "2026-02-28");
    }
}
