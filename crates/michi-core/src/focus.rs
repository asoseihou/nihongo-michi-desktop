//! Focus mode session, shared by the backend (timer, credit) and the UI
//! (the focus window and the Today card count down from the same numbers).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Focus {
    pub id: String,
    pub title: String,
    pub jp: String,
    pub color: String,
    /// Day key the minutes go to (`DayDoc.fo`): `rotina:<block>`,
    /// `<event>@<date>` or `livre:<id>`.
    pub key: String,
    pub block: Option<String>,
    pub event_id: Option<String>,
    pub occ: Option<String>,
    /// Study day when it started.
    pub day: String,
    pub planned_ms: f64,
    pub started_at: f64,
    pub paused_at: Option<f64>,
    pub paused_ms: f64,
    pub beat_at: f64,
    /// running | paused | done
    pub state: String,
    pub finished_at: f64,
    /// Ran to the end (not stopped early).
    pub completed: bool,
    pub credited_min: f64,
    pub xp: f64,
    /// The block / item was marked done from the summary.
    pub marked: bool,
}

impl Focus {
    pub fn elapsed_ms(&self, now: f64) -> f64 {
        let end = match self.state.as_str() {
            "done" => self.finished_at,
            "paused" => self.paused_at.unwrap_or(now),
            _ => now,
        };
        (end - self.started_at - self.paused_ms).clamp(0.0, self.planned_ms.max(0.0))
    }

    pub fn remaining_ms(&self, now: f64) -> f64 {
        (self.planned_ms - self.elapsed_ms(now)).max(0.0)
    }

    /// Tied to a routine block or an agenda item (can be marked done).
    pub fn linked(&self) -> bool {
        self.block.is_some() || self.event_id.is_some()
    }

    /// Counting (running or paused).
    pub fn active(&self) -> bool {
        self.state == "running" || self.state == "paused"
    }
}

/// What starts a session (Today, the agenda, the reminder, the tray).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Spec {
    pub title: String,
    pub jp: String,
    pub color: String,
    pub minutes: f64,
    pub block: Option<String>,
    pub event_id: Option<String>,
    pub occ: Option<String>,
}

/// "mm:ss" (or "h:mm:ss") of a duration in ms, rounded up to the second.
pub fn clock(ms: f64) -> String {
    let s = (ms / 1000.0).ceil().max(0.0) as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_counts_pauses_and_caps_at_plan() {
        let mut f = Focus {
            planned_ms: 25.0 * 60_000.0,
            started_at: 1_000_000.0,
            state: "running".into(),
            ..Default::default()
        };
        assert_eq!(f.elapsed_ms(1_000_000.0 + 60_000.0), 60_000.0);
        f.state = "paused".into();
        f.paused_at = Some(1_000_000.0 + 120_000.0);
        assert_eq!(f.elapsed_ms(1_000_000.0 + 900_000.0), 120_000.0, "paused time does not count");
        f.paused_ms = 780_000.0;
        f.paused_at = None;
        f.state = "running".into();
        assert_eq!(f.elapsed_ms(1_000_000.0 + 960_000.0), 180_000.0);
        assert_eq!(f.elapsed_ms(1e12), 25.0 * 60_000.0, "never more than planned");
        assert_eq!(f.remaining_ms(1e12), 0.0);
        assert!(f.active() && !f.linked());
        assert_eq!(clock(61_000.0), "01:01");
        assert_eq!(clock(59_001.0), "01:00");
        assert_eq!(clock(3_725_000.0), "1:02:05");
    }
}
