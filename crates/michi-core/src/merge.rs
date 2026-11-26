//! Conflict resolution for the sync between PCs. Most records simply take
//! the newer version; these functions run only when the same record changed
//! on two PCs since their last sync, and keep what each side did.

use crate::docs::{DayDoc, ImmEntry, Profile};
use serde_json::{Map, Value};
use std::cmp::Ordering;

fn max_into(dst: &mut std::collections::BTreeMap<String, f64>, src: &std::collections::BTreeMap<String, f64>) {
    for (k, v) in src {
        let e = dst.entry(k.clone()).or_insert(*v);
        *e = e.max(*v);
    }
}

fn same_imm(a: &ImmEntry, b: &ImmEntry) -> bool {
    a.at == b.at && a.t == b.t && a.title == b.title && a.min == b.min
}

/// A study day changed on two PCs. Counters keep the larger value (so
/// nothing is counted twice), checks and agenda credits follow the newer
/// version, and the immersion logs of both are joined.
pub fn merge_day(newer: &DayDoc, older: &DayDoc) -> DayDoc {
    let mut d = newer.clone();
    d.xp = newer.xp.max(older.xp);
    d.rev = newer.rev.max(older.rev);
    d.games = newer.games.max(older.games);
    d.sent = newer.sent.max(older.sent);
    d.gp = newer.gp.max(older.gp);
    max_into(&mut d.nw, &older.nw);
    max_into(&mut d.sec, &older.sec);
    max_into(&mut d.fo, &older.fo);
    for (k, v) in &older.done {
        d.done.entry(k.clone()).or_insert(*v);
    }
    for (k, v) in &older.ag {
        d.ag.entry(k.clone()).or_insert(*v);
    }
    for e in &older.imm {
        if !d.imm.iter().any(|x| same_imm(x, e)) {
            d.imm.push(e.clone());
        }
    }
    d.imm.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(Ordering::Equal));
    d.srs_clear = newer.srs_clear || older.srs_clear;
    for (k, v) in &older.extra {
        d.extra.entry(k.clone()).or_insert_with(|| v.clone());
    }
    d.u = match (newer.u, older.u) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    d
}

/// The profile changed on two PCs: settings follow the newer one, total XP
/// keeps the larger value and the start date the earlier one.
pub fn merge_profile(newer: &Profile, older: &Profile) -> Profile {
    let mut p = newer.clone();
    p.xp = newer.xp.max(older.xp);
    if older.created > 0.0 && (p.created <= 0.0 || older.created < p.created) {
        p.created = older.created;
    }
    p.onboarded |= older.onboarded;
    p
}

fn all_with_id(a: &[Value]) -> bool {
    a.iter().all(|x| x.get("id").and_then(Value::as_str).is_some())
}

/// Any other document (journey, kana, mined words…): objects are joined key
/// by key, numbers keep the larger value, lists of items with an `id` are
/// joined by id and lists of numbers element by element; anything else
/// follows the newer version.
pub fn merge_json(newer: &Value, older: &Value) -> Value {
    match (newer, older) {
        (Value::Object(n), Value::Object(o)) => {
            let mut out: Map<String, Value> = n.clone();
            for (k, ov) in o {
                let merged = match n.get(k) {
                    Some(nv) => merge_json(nv, ov),
                    None => ov.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (Value::Number(a), Value::Number(b)) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) if y > x => older.clone(),
            _ => newer.clone(),
        },
        (Value::Array(n), Value::Array(o)) if !n.is_empty() && !o.is_empty() && all_with_id(n) && all_with_id(o) => {
            let mut out = n.clone();
            for item in o {
                let id = item.get("id");
                if !n.iter().any(|x| x.get("id") == id) {
                    out.push(item.clone());
                }
            }
            Value::Array(out)
        }
        (Value::Array(n), Value::Array(o)) if n.len() == o.len() && n.iter().chain(o.iter()).all(Value::is_number) => {
            Value::Array(n.iter().zip(o).map(|(a, b)| merge_json(a, b)).collect())
        }
        _ => newer.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn day(j: Value) -> DayDoc {
        serde_json::from_value(j).unwrap()
    }

    #[test]
    fn day_keeps_both_sides() {
        let a = day(json!({"k":"2026-11-26","xp":40,"rev":30,"sec":{"srs":900},"done":{"srs":true},
            "ag":{"e@2026-11-26":0},"imm":[{"t":"vlog","min":15,"title":"A","at":2}],"games":1}));
        let b = day(
            json!({"k":"2026-11-26","xp":25,"rev":10,"sec":{"srs":300,"jogos":600},"done":{"srs":false,"vlog":true},
            "ag":{"e@2026-11-26":30,"x@2026-11-26":20},"imm":[{"t":"wagotabi","min":20,"title":"B","at":1}],"srsClear":true}),
        );
        let m = merge_day(&a, &b);
        assert_eq!((m.xp, m.rev, m.games), (40.0, 30.0, 1.0));
        assert_eq!(m.sec["srs"], 900.0);
        assert_eq!(m.sec["jogos"], 600.0);
        assert!(m.done["srs"], "the newer check wins");
        assert!(m.done["vlog"]);
        assert_eq!(m.ag["e@2026-11-26"], 0.0, "an unmarked credit stays unmarked");
        assert_eq!(m.ag["x@2026-11-26"], 20.0);
        assert_eq!(m.imm.len(), 2);
        assert_eq!(m.imm[0].title, "B", "logs sorted by time");
        assert!(m.srs_clear);
        // Merging again changes nothing.
        assert_eq!(merge_day(&m, &b), m);
        assert_eq!(merge_day(&m, &a), m);
    }

    #[test]
    fn profile_settings_from_newer_xp_from_larger() {
        let newer: Profile = serde_json::from_value(json!({"name":"Novo","xp":100,"created":2000})).unwrap();
        let older: Profile =
            serde_json::from_value(json!({"name":"Velho","xp":350,"created":1000,"onboarded":true})).unwrap();
        let p = merge_profile(&newer, &older);
        assert_eq!(p.name, "Novo");
        assert_eq!(p.xp, 350.0);
        assert_eq!(p.created, 1000.0);
        assert!(p.onboarded);
    }

    #[test]
    fn json_joins_maps_lists_and_numbers() {
        let newer = json!({"done":{"K01":1},"best":{"K01":70},"list":[{"id":"a","front":"新"}],"s":{"あ":[10,9,800]},"mode":"n"});
        let older = json!({"done":{"K02":1},"best":{"K01":90},"list":[{"id":"a","front":"旧"},{"id":"b"}],"s":{"あ":[12,8,900]},"mode":"o"});
        let m = merge_json(&newer, &older);
        assert_eq!(m["done"], json!({"K01":1,"K02":1}));
        assert_eq!(m["best"]["K01"], 90);
        assert_eq!(m["list"], json!([{"id":"a","front":"新"},{"id":"b"}]));
        assert_eq!(m["s"]["あ"], json!([12, 9, 900]));
        assert_eq!(m["mode"], "n");
        assert_eq!(merge_json(&m, &older), m);
    }
}
