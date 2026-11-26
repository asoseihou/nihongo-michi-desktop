//! The subset of RFC 5545 RRULE the agenda needs: FREQ (DAILY, WEEKLY,
//! MONTHLY, YEARLY), INTERVAL, BYDAY (weekly), UNTIL and COUNT. Single
//! occurrences are skipped with EXDATE-style dates kept on the event.
//! Times are floating (local wall clock), which fits a one-person agenda.

use chrono::{Datelike, Duration, Months, NaiveDate, Weekday};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RRule {
    pub freq: Freq,
    pub interval: u32,
    /// Weekdays for WEEKLY (empty = weekday of DTSTART).
    pub byday: Vec<Weekday>,
    /// Last allowed date (inclusive).
    pub until: Option<NaiveDate>,
    pub count: Option<u32>,
}

const CODES: [(&str, Weekday); 7] = [
    ("MO", Weekday::Mon),
    ("TU", Weekday::Tue),
    ("WE", Weekday::Wed),
    ("TH", Weekday::Thu),
    ("FR", Weekday::Fri),
    ("SA", Weekday::Sat),
    ("SU", Weekday::Sun),
];

fn code_of(w: Weekday) -> &'static str {
    CODES.iter().find(|(_, x)| *x == w).map(|(c, _)| *c).unwrap_or("MO")
}

/// JS weekday number (0 = Sunday) ↔ chrono.
pub fn weekday_of_js(d: u8) -> Weekday {
    match d % 7 {
        0 => Weekday::Sun,
        1 => Weekday::Mon,
        2 => Weekday::Tue,
        3 => Weekday::Wed,
        4 => Weekday::Thu,
        5 => Weekday::Fri,
        _ => Weekday::Sat,
    }
}

pub fn js_of_weekday(w: Weekday) -> u8 {
    w.num_days_from_sunday() as u8
}

impl RRule {
    pub fn daily() -> Self {
        RRule { freq: Freq::Daily, interval: 1, byday: vec![], until: None, count: None }
    }

    /// Daily when all 7 days are set, otherwise WEEKLY;BYDAY=… (the
    /// routine's `rruleFor`).
    pub fn for_js_days(days: &[u8]) -> Self {
        let mut set: Vec<u8> = days.to_vec();
        set.sort_unstable();
        set.dedup();
        if set.len() >= 7 || set.is_empty() {
            return Self::daily();
        }
        let mut byday: Vec<Weekday> = set.iter().map(|d| weekday_of_js(*d)).collect();
        byday.sort_by_key(|w| w.num_days_from_monday());
        RRule { freq: Freq::Weekly, interval: 1, byday, until: None, count: None }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.strip_prefix("RRULE:").unwrap_or(s);
        if s.is_empty() {
            return None;
        }
        let mut r = RRule { freq: Freq::Daily, interval: 1, byday: vec![], until: None, count: None };
        let mut has_freq = false;
        for part in s.split(';') {
            let (k, v) = part.split_once('=')?;
            match k.trim().to_ascii_uppercase().as_str() {
                "FREQ" => {
                    has_freq = true;
                    r.freq = match v.trim().to_ascii_uppercase().as_str() {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        _ => return None,
                    }
                }
                // Bounded so date steps stay far from chrono's limits.
                "INTERVAL" => r.interval = v.trim().parse::<u32>().ok()?.clamp(1, 1000),
                "BYDAY" => {
                    for c in v.split(',') {
                        let c = c.trim().to_ascii_uppercase();
                        let code = &c[c.len().saturating_sub(2)..];
                        if let Some((_, w)) = CODES.iter().find(|(x, _)| *x == code) {
                            if !r.byday.contains(w) {
                                r.byday.push(*w);
                            }
                        }
                    }
                    r.byday.sort_by_key(|w| w.num_days_from_monday());
                }
                "UNTIL" => {
                    let v = v.trim();
                    let d = v.get(..8)?;
                    r.until = NaiveDate::parse_from_str(d, "%Y%m%d").ok();
                }
                "COUNT" => r.count = v.trim().parse::<u32>().ok().map(|c| c.min(10_000)),
                _ => {}
            }
        }
        has_freq.then_some(r)
    }

    /// Occurrence dates of a series starting on `start`, limited to
    /// `[from, to]` (inclusive). Skipped dates are applied by the caller.
    pub fn dates(&self, start: NaiveDate, from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
        let mut out = Vec::new();
        let limit = match self.until {
            Some(u) => u.min(to),
            None => to,
        };
        if limit < start {
            return out;
        }
        let max = self.count.unwrap_or(u32::MAX);
        let mut n: u32 = 0;
        let step = self.interval.max(1);
        // Hard stop so a malformed rule can never spin forever.
        let mut guard = 0u32;
        let push = |d: NaiveDate, n: &mut u32, out: &mut Vec<NaiveDate>| -> bool {
            if d > limit || *n >= max {
                return false;
            }
            *n += 1;
            if d >= from {
                out.push(d);
            }
            true
        };
        match self.freq {
            Freq::Daily => {
                let mut d = start;
                // Without COUNT we can jump close to `from`.
                if self.count.is_none() && from > start {
                    let gap = (from - start).num_days() as u32;
                    d = start + Duration::days(((gap / step) * step) as i64);
                }
                while guard < 200_000 && push(d, &mut n, &mut out) {
                    d += Duration::days(step as i64);
                    guard += 1;
                }
            }
            Freq::Weekly => {
                let days: Vec<Weekday> = if self.byday.is_empty() { vec![start.weekday()] } else { self.byday.clone() };
                let mut week = start - Duration::days(start.weekday().num_days_from_monday() as i64);
                if self.count.is_none() && from > start {
                    let gap_weeks = ((from - week).num_days() / 7) as u32;
                    week += Duration::weeks(((gap_weeks / step) * step) as i64);
                }
                'outer: while guard < 100_000 {
                    for w in &days {
                        let d = week + Duration::days(w.num_days_from_monday() as i64);
                        if d < start {
                            continue;
                        }
                        if !push(d, &mut n, &mut out) {
                            break 'outer;
                        }
                    }
                    week += Duration::weeks(step as i64);
                    guard += 1;
                }
            }
            Freq::Monthly | Freq::Yearly => {
                let months = if self.freq == Freq::Monthly { step } else { step * 12 };
                let mut i: u32 = 0;
                while guard < 50_000 {
                    let first = NaiveDate::from_ymd_opt(start.year(), start.month(), 1).unwrap();
                    let Some(m) = first.checked_add_months(Months::new(i * months)) else { break };
                    if m > limit {
                        break;
                    }
                    // Months without that day (31st, Feb 29) are skipped, as in RFC 5545.
                    if let Some(d) = NaiveDate::from_ymd_opt(m.year(), m.month(), start.day()) {
                        if !push(d, &mut n, &mut out) {
                            break;
                        }
                    }
                    i += 1;
                    guard += 1;
                }
            }
        }
        out
    }

    /// Short Portuguese description for the UI.
    pub fn describe(&self) -> String {
        const PT: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
        let every = |one: &str, many: &str| {
            if self.interval == 1 {
                one.to_string()
            } else {
                format!("a cada {} {}", self.interval, many)
            }
        };
        let mut s = match self.freq {
            Freq::Daily => every("todo dia", "dias"),
            Freq::Weekly => {
                let mut ds: Vec<&str> = self.byday.iter().map(|w| PT[w.num_days_from_monday() as usize]).collect();
                if ds.is_empty() {
                    ds.push("mesmo dia");
                }
                let base = every("toda semana", "semanas");
                let set: Vec<u32> = self.byday.iter().map(|w| w.num_days_from_monday()).collect();
                if set == [0, 1, 2, 3, 4] {
                    format!("{base} · seg a sex")
                } else if set == [5, 6] {
                    format!("{base} · fim de semana")
                } else {
                    format!("{base} · {}", ds.join(", "))
                }
            }
            Freq::Monthly => every("todo mês", "meses"),
            Freq::Yearly => every("todo ano", "anos"),
        };
        if let Some(u) = self.until {
            s.push_str(&format!(" · até {}", u.format("%d/%m/%Y")));
        }
        if let Some(c) = self.count {
            s.push_str(&format!(" · {c} vezes"));
        }
        s
    }
}

impl fmt::Display for RRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let freq = match self.freq {
            Freq::Daily => "DAILY",
            Freq::Weekly => "WEEKLY",
            Freq::Monthly => "MONTHLY",
            Freq::Yearly => "YEARLY",
        };
        write!(f, "FREQ={freq}")?;
        if self.interval > 1 {
            write!(f, ";INTERVAL={}", self.interval)?;
        }
        if self.freq == Freq::Weekly && !self.byday.is_empty() {
            let codes: Vec<&str> = self.byday.iter().map(|w| code_of(*w)).collect();
            write!(f, ";BYDAY={}", codes.join(","))?;
        }
        if let Some(u) = self.until {
            write!(f, ";UNTIL={}", u.format("%Y%m%d"))?;
        }
        if let Some(c) = self.count {
            write!(f, ";COUNT={c}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn roundtrip() {
        let r = RRule::parse("RRULE:FREQ=WEEKLY;BYDAY=WE,MO;UNTIL=20270131T235959Z").unwrap();
        assert_eq!(r.to_string(), "FREQ=WEEKLY;BYDAY=MO,WE;UNTIL=20270131");
        assert_eq!(RRule::for_js_days(&[0, 1, 2, 3, 4, 5, 6]).to_string(), "FREQ=DAILY");
        assert_eq!(RRule::for_js_days(&[0, 6]).to_string(), "FREQ=WEEKLY;BYDAY=SA,SU");
        assert!(RRule::parse("BYDAY=MO").is_none());
    }

    #[test]
    fn weekly_byday_until() {
        let r = RRule::parse("FREQ=WEEKLY;BYDAY=MO,WE,FR;UNTIL=20261211").unwrap();
        // start Wed 2026-11-25
        let v = r.dates(d(2026, 11, 25), d(2026, 11, 1), d(2027, 1, 1));
        assert_eq!(v.first(), Some(&d(2026, 11, 25)));
        assert_eq!(v.last(), Some(&d(2026, 12, 11)));
        assert_eq!(v.len(), 8); // 25,27 | 30,2,4 | 7,9,11
    }

    #[test]
    fn daily_window_and_count() {
        let r = RRule::parse("FREQ=DAILY;INTERVAL=2").unwrap();
        let v = r.dates(d(2026, 1, 1), d(2026, 1, 10), d(2026, 1, 14));
        assert_eq!(v, vec![d(2026, 1, 11), d(2026, 1, 13)]);
        let c = RRule::parse("FREQ=DAILY;COUNT=3").unwrap();
        assert_eq!(c.dates(d(2026, 1, 1), d(2026, 1, 2), d(2026, 2, 1)), vec![d(2026, 1, 2), d(2026, 1, 3)]);
    }

    #[test]
    fn weekly_window_jump_keeps_interval() {
        let r = RRule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU").unwrap();
        // start Tue 2026-01-06 → every other Tuesday
        let v = r.dates(d(2026, 1, 6), d(2026, 3, 1), d(2026, 3, 31));
        assert_eq!(v, vec![d(2026, 3, 3), d(2026, 3, 17), d(2026, 3, 31)]);
    }

    #[test]
    fn monthly_skips_short_months() {
        let r = RRule::parse("FREQ=MONTHLY").unwrap();
        let v = r.dates(d(2026, 1, 31), d(2026, 1, 1), d(2026, 5, 31));
        assert_eq!(v, vec![d(2026, 1, 31), d(2026, 3, 31), d(2026, 5, 31)]);
    }
}
