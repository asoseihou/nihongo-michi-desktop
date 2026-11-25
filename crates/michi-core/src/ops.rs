//! Small mutations the backend applies to documents (from the notification
//! window or the agenda) and replays in the UI, so both copies stay equal.

use crate::docs::{DayDoc, Profile};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Op {
    /// `day.done[id] = v`
    Done { day: String, id: String, v: bool },
    /// `profile.xp += n; day.xp += n`
    Xp { day: String, n: f64 },
    /// `day.ag[key] = min` (None removes)
    Ag { day: String, key: String, min: Option<f64> },
}

impl Op {
    pub fn day(&self) -> &str {
        match self {
            Op::Done { day, .. } | Op::Xp { day, .. } | Op::Ag { day, .. } => day,
        }
    }

    pub fn touches_profile(&self) -> bool {
        matches!(self, Op::Xp { .. })
    }

    pub fn apply_day(&self, d: &mut DayDoc) {
        match self {
            Op::Done { id, v, .. } => {
                d.done.insert(id.clone(), *v);
            }
            Op::Xp { n, .. } => d.xp += n,
            Op::Ag { key, min, .. } => match min {
                Some(m) => {
                    d.ag.insert(key.clone(), *m);
                }
                None => {
                    d.ag.remove(key);
                }
            },
        }
    }

    pub fn apply_profile(&self, p: &mut Profile) {
        if let Op::Xp { n, .. } = self {
            p.xp += n;
        }
    }
}

/// XP for checking a block or an agenda item (same as the board's checkbox).
pub const DONE_XP: f64 = 10.0;

/// Operations for marking an agenda occurrence done / not done.
/// `block_id` is set for routine events. XP is never taken back (the
/// artifact's checkbox does not subtract it either).
pub fn mark_ops(day: &str, block_id: Option<&str>, ag_key: &str, minutes: f64, done: bool) -> Vec<Op> {
    let mut ops = Vec::new();
    if let Some(b) = block_id {
        ops.push(Op::Done { day: day.into(), id: b.into(), v: done });
    }
    ops.push(Op::Ag { day: day.into(), key: ag_key.into(), min: if done { Some(minutes) } else { None } });
    if done {
        ops.push(Op::Xp { day: day.into(), n: DONE_XP });
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_and_unmark() {
        let mut d = DayDoc::new("2026-11-25");
        let mut p = Profile::default();
        for op in mark_ops("2026-11-25", Some("vlog"), "rotina:vlog", 15.0, true) {
            op.apply_day(&mut d);
            op.apply_profile(&mut p);
        }
        assert!(d.block_done("vlog"));
        assert_eq!((d.xp, p.xp, d.study_minutes()), (10.0, 10.0, 15.0));
        for op in mark_ops("2026-11-25", Some("vlog"), "rotina:vlog", 15.0, false) {
            op.apply_day(&mut d);
        }
        assert!(!d.block_done("vlog"));
        assert_eq!(d.study_minutes(), 0.0);
        let j = serde_json::to_string(&Op::Xp { day: "x".into(), n: 10.0 }).unwrap();
        assert_eq!(j, r#"{"t":"xp","day":"x","n":10.0}"#);
    }
}
