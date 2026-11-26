//! Typed versions of the artifact's documents (`profile`, `day:YYYY-MM-DD`).
//! Unknown fields are kept in `extra`, so a backup survives a round trip.

use crate::de;
use crate::routine::Block;
use crate::time::now_ms;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    #[serde(deserialize_with = "de::string_any")]
    pub name: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub new_kanji: f64,
    #[serde(deserialize_with = "de::f64_any")]
    pub new_vocab: f64,
    #[serde(deserialize_with = "de::f64_any")]
    pub new_mined: f64,
    #[serde(deserialize_with = "de::f64_any")]
    pub new_grammar: f64,
    #[serde(deserialize_with = "de::f64_any")]
    pub goal_min: f64,
    /// smart | always | never
    #[serde(deserialize_with = "de::string_any")]
    pub furi: String,
    #[serde(rename = "showPT", deserialize_with = "de::bool_any")]
    pub show_pt: bool,
    #[serde(deserialize_with = "de::f64_any")]
    pub tts_rate: f64,
    #[serde(deserialize_with = "de::string_any")]
    pub voice: String,
    #[serde(deserialize_with = "de::string_any")]
    pub theme: String,
    #[serde(deserialize_with = "de::bool_any")]
    pub minimal_day: bool,
    #[serde(deserialize_with = "de::string_any")]
    pub wagotabi: String,
    #[serde(deserialize_with = "de::string_any")]
    pub vlog_time: String,
    #[serde(deserialize_with = "de::opt_vec_lossy")]
    pub routine: Option<Vec<Block>>,
    /// Day key of the last "Sim, fiz" on the Anki banner.
    #[serde(deserialize_with = "de::string_any")]
    pub anki_done: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub created: f64,
    #[serde(deserialize_with = "de::f64_any")]
    pub xp: f64,
    #[serde(deserialize_with = "de::bool_any")]
    pub notif: bool,
    #[serde(deserialize_with = "de::bool_any", skip_serializing_if = "de::is_false")]
    pub onboarded: bool,
    #[serde(deserialize_with = "de::string_any", skip_serializing_if = "String::is_empty")]
    pub trip_start: String,
    #[serde(deserialize_with = "de::string_any", skip_serializing_if = "String::is_empty")]
    pub trip_end: String,
    /// Minutes before a routine block to remind (was the Google Calendar alarm).
    #[serde(deserialize_with = "de::f64_any")]
    pub cal_remind: f64,
    /// Last kana set picked in the games ("h:basic,daku").
    #[serde(deserialize_with = "de::string_any", skip_serializing_if = "String::is_empty")]
    pub last_kana: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            name: "Mirage".into(),
            new_kanji: 5.0,
            new_vocab: 0.0,
            new_mined: 10.0,
            new_grammar: 5.0,
            goal_min: 75.0,
            furi: "smart".into(),
            show_pt: false,
            tts_rate: 0.92,
            voice: String::new(),
            theme: "auto".into(),
            minimal_day: false,
            wagotabi: String::new(),
            vlog_time: "21:30".into(),
            routine: None,
            anki_done: String::new(),
            created: now_ms(),
            xp: 0.0,
            notif: false,
            onboarded: false,
            trip_start: String::new(),
            trip_end: String::new(),
            cal_remind: 0.0,
            last_kana: String::new(),
            u: None,
            extra: Map::new(),
        }
    }
}

impl Profile {
    pub fn trip_dates(&self) -> (String, String) {
        let s = if self.trip_start.is_empty() { "2028-12-01".to_string() } else { self.trip_start.clone() };
        let e = if self.trip_end.is_empty() { "2028-12-29".to_string() } else { self.trip_end.clone() };
        (s, e)
    }
    pub fn remind_before(&self) -> i64 {
        (self.cal_remind as i64).clamp(0, 60)
    }
}

/// One immersion log entry (`d.imm`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ImmEntry {
    #[serde(deserialize_with = "de::string_any")]
    pub t: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub min: f64,
    #[serde(deserialize_with = "de::string_any")]
    pub title: String,
    #[serde(deserialize_with = "de::string_any")]
    pub note: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub at: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `day:YYYY-MM-DD`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DayDoc {
    #[serde(deserialize_with = "de::string_any")]
    pub k: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub xp: f64,
    #[serde(deserialize_with = "de::f64_any")]
    pub rev: f64,
    /// New cards introduced per deck.
    #[serde(deserialize_with = "de::f64_map")]
    pub nw: BTreeMap<String, f64>,
    /// Active seconds per category.
    #[serde(deserialize_with = "de::f64_map")]
    pub sec: BTreeMap<String, f64>,
    /// Routine blocks checked by hand.
    #[serde(deserialize_with = "de::bool_map")]
    pub done: BTreeMap<String, bool>,
    #[serde(deserialize_with = "de::vec_lossy")]
    pub imm: Vec<ImmEntry>,
    #[serde(deserialize_with = "de::f64_any")]
    pub games: f64,
    #[serde(deserialize_with = "de::f64_any")]
    pub sent: f64,
    /// Grammar points studied.
    #[serde(deserialize_with = "de::f64_any")]
    pub gp: f64,
    /// Desktop only: minutes credited by marking agenda items as done,
    /// keyed by `eventId@YYYY-MM-DD` (or `rotina:<blockId>`).
    #[serde(deserialize_with = "de::f64_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub ag: BTreeMap<String, f64>,
    /// Desktop only: minutes of focus sessions, by the same keys as `ag`
    /// (`livre:<id>` for free study), so a focused block is not counted twice.
    #[serde(deserialize_with = "de::f64_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub fo: BTreeMap<String, f64>,
    /// Desktop only: the SRS queue was emptied today (after ≥10 reviews).
    #[serde(rename = "srsClear", deserialize_with = "de::bool_any", skip_serializing_if = "de::is_false")]
    pub srs_clear: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl DayDoc {
    pub fn new(k: &str) -> Self {
        DayDoc { k: k.to_string(), ..Default::default() }
    }

    /// Minutes logged in the app's own way (`studyMinutes`), before agenda credit.
    pub fn tracked_minutes(&self) -> f64 {
        self.sec.values().sum::<f64>() / 60.0 + self.imm.iter().map(|e| e.min).sum::<f64>()
    }

    /// Minutes credited by the agenda and by focus sessions; per item, the
    /// larger of the two (a block focused for 30 min and marked done counts 30).
    pub fn credited_minutes(&self) -> f64 {
        let mut total = 0.0;
        for (k, v) in &self.ag {
            total += v.max(self.fo.get(k).copied().unwrap_or(0.0));
        }
        for (k, v) in &self.fo {
            if !self.ag.contains_key(k) {
                total += v;
            }
        }
        total
    }

    /// `studyMinutes(d)` plus the minutes credited from the agenda and focus.
    /// Credit is a floor: it only adds what active time did not already count.
    pub fn study_minutes(&self) -> f64 {
        let tracked = self.tracked_minutes();
        tracked.max(self.credited_minutes()).max(0.0).round()
    }

    /// `BLOCK_AUTO`: blocks that complete themselves from what was done today.
    pub fn auto_done(&self, block_id: &str) -> bool {
        match block_id {
            "srs" => self.srs_clear,
            "jogo" => self.games >= 1.0,
            "estacao" => self.gp >= 1.0,
            "wagotabi" => self.imm.iter().any(|e| e.t == "wagotabi"),
            "frases" => self.sent >= 5.0,
            "vlog" => self.imm.iter().any(|e| e.t == "vlog" || e.t == "podcast"),
            _ => false,
        }
    }

    /// `blockDone(b, d)` (the SRS rule needs live due counts, so the UI
    /// also checks them; the backend relies on `srsClear`).
    pub fn block_done(&self, block_id: &str) -> bool {
        self.done.get(block_id).copied().unwrap_or(false) || self.auto_done(block_id)
    }
}

/// `levelOf(xp)`.
pub fn level_of(xp: f64) -> i64 {
    (xp / 40.0).sqrt().floor() as i64 + 1
}

/// `xpForLevel(l)`.
pub fn xp_for_level(l: i64) -> f64 {
    40.0 * ((l - 1) * (l - 1)) as f64
}

/// `streak()`: consecutive study days with ≥ 20 XP, counting today only
/// once it qualifies.
pub fn streak(days: &BTreeMap<String, DayDoc>, today: &str) -> i64 {
    let ok = |k: &str| days.get(k).map(|d| d.xp >= 20.0).unwrap_or(false);
    let mut k = today.to_string();
    if !ok(&k) {
        k = crate::time::key_add(&k, -1);
    }
    let mut n = 0;
    while ok(&k) {
        n += 1;
        k = crate::time::key_add(&k, -1);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_messy_profile() {
        let j =
            r#"{"name":"Mirage","newKanji":"7","goalMin":null,"routine":null,"xp":120.5,"notif":1,"foo":{"bar":1}}"#;
        let p: Profile = serde_json::from_str(j).unwrap();
        assert_eq!(p.new_kanji, 7.0);
        assert_eq!(p.goal_min, 0.0);
        assert!(p.notif);
        assert!(p.routine.is_none());
        assert_eq!(p.extra.get("foo").unwrap()["bar"], 1);
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["foo"]["bar"], 1);
        assert_eq!(back["newKanji"], 7.0);
    }

    #[test]
    fn day_minutes_and_auto() {
        let j = r#"{"k":"2026-11-25","xp":30,"sec":{"srs":600,"jogos":300},"imm":[{"t":"vlog","min":15}],"games":1,"done":{"x":true,"y":0},"notified":{"srs":1}}"#;
        let mut d: DayDoc = serde_json::from_str(j).unwrap();
        assert_eq!(d.study_minutes(), 30.0);
        assert!(d.block_done("vlog") && d.block_done("jogo") && d.block_done("x") && !d.block_done("y"));
        d.ag.insert("e@2026-11-25".into(), 60.0);
        assert_eq!(d.study_minutes(), 60.0);
        // Focus on the same item counts once (the larger); free focus adds.
        d.fo.insert("e@2026-11-25".into(), 75.0);
        assert_eq!(d.study_minutes(), 75.0);
        d.fo.insert("livre:1".into(), 25.0);
        assert_eq!(d.study_minutes(), 100.0);
        assert!(serde_json::to_value(&d).unwrap().get("notified").is_some());
    }

    #[test]
    fn streak_counts_back() {
        let mut m = BTreeMap::new();
        for (k, xp) in [("2026-11-22", 25.0), ("2026-11-23", 40.0), ("2026-11-24", 20.0), ("2026-11-25", 5.0)] {
            let mut d = DayDoc::new(k);
            d.xp = xp;
            m.insert(k.to_string(), d);
        }
        assert_eq!(streak(&m, "2026-11-25"), 3);
        m.get_mut("2026-11-25").unwrap().xp = 20.0;
        assert_eq!(streak(&m, "2026-11-25"), 4);
        assert_eq!(level_of(0.0), 1);
        assert_eq!(level_of(40.0), 2);
        assert_eq!(xp_for_level(3), 160.0);
    }
}
