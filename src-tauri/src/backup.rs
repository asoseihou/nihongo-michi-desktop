//! Backup in the artifact's format (`{app:'nihongo-michi', v:1, at, docs}`),
//! so a backup made in the web version (Ajustes → 💾 Exportar backup)
//! restores here, and a desktop backup still opens in the artifact. The
//! desktop-only agenda travels in an extra `desktop` key the artifact ignores.

use crate::db::{self, err, R};
use michi_core::agenda::Event;
use michi_core::srs::chunk_of;
use michi_core::Card;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Map, Value};

#[derive(Serialize, Clone, Debug)]
pub struct BackupInfo {
    pub path: String,
    pub at: f64,
    pub name: String,
    pub xp: f64,
    pub days: usize,
    pub cards: usize,
    pub docs: Vec<String>,
    pub desktop: bool,
}

pub fn export(c: &Connection) -> R<Value> {
    let mut docs: Map<String, Value> = Map::new();
    for (k, v) in db::docs_all(c)? {
        docs.insert(k, v);
    }
    for (k, v) in db::days_all(c)? {
        docs.insert(format!("day:{k}"), v);
    }
    let now = michi_core::time::now_ms();
    let mut buckets: Map<String, Value> = Map::new();
    for i in 0..8 {
        buckets.insert(format!("srs{i}"), json!({"c": {}, "u": now}));
    }
    for (id, card) in db::cards_all(c)? {
        if let Some(Value::Object(b)) = buckets.get_mut(&chunk_of(&id)) {
            if let Some(Value::Object(cm)) = b.get_mut("c") {
                cm.insert(id, serde_json::to_value(card.0).map_err(err)?);
            }
        }
    }
    docs.extend(buckets);
    let events = db::events_all(c)?;
    let done: Vec<Value> = {
        let mut st = c.prepare("SELECT event_id, occ, done_at FROM occ_done").map_err(err)?;
        let rows = st
            .query_map([], |r| Ok(json!([r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?])))
            .map_err(err)?;
        rows.flatten().collect()
    };
    Ok(json!({
        "app": "nihongo-michi",
        "v": 1,
        "at": now,
        "docs": docs,
        "desktop": {"v": 1, "events": events, "occDone": done}
    }))
}

/// Reads and checks a backup file without touching the database.
pub fn inspect(path: &str) -> R<(Value, BackupInfo)> {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 64 * 1024 * 1024 {
        return Err("o arquivo é grande demais para um backup do Michi".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("não consegui abrir o arquivo: {e}"))?;
    let j: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("o arquivo não é JSON válido: {e}"))?;
    if j.get("app").and_then(Value::as_str) != Some("nihongo-michi")
        || !j.get("docs").map(Value::is_object).unwrap_or(false)
    {
        return Err("arquivo não é um backup do Michi".into());
    }
    let docs = j["docs"].as_object().cloned().unwrap_or_default();
    let mut info = BackupInfo {
        path: path.to_string(),
        at: j.get("at").and_then(Value::as_f64).unwrap_or(0.0),
        name: docs.get("profile").and_then(|p| p.get("name")).and_then(Value::as_str).unwrap_or("").to_string(),
        xp: docs.get("profile").and_then(|p| p.get("xp")).and_then(Value::as_f64).unwrap_or(0.0),
        days: 0,
        cards: 0,
        docs: vec![],
        desktop: j.get("desktop").is_some(),
    };
    for (k, v) in &docs {
        if k.starts_with("day:") {
            info.days += 1;
        } else if is_srs(k) {
            info.cards += v.get("c").and_then(Value::as_object).map(|m| m.len()).unwrap_or(0);
        } else {
            info.docs.push(k.clone());
        }
    }
    Ok((j, info))
}

fn is_srs(k: &str) -> bool {
    k.len() == 4 && k.starts_with("srs") && k.as_bytes()[3].is_ascii_digit()
}

/// Replaces the progress with the backup (like the artifact's restore).
/// Agenda events are replaced only when the backup carries them.
pub fn restore(c: &mut Connection, j: &Value) -> R<()> {
    let docs = j["docs"].as_object().cloned().unwrap_or_default();
    let tx = c.transaction().map_err(err)?;
    tx.execute_batch("DELETE FROM docs; DELETE FROM days; DELETE FROM cards;").map_err(err)?;
    let now = michi_core::time::now_ms();
    for (k, mut v) in docs {
        if let Value::Object(m) = &mut v {
            m.insert("u".into(), json!(now));
        }
        if let Some(day) = k.strip_prefix("day:") {
            db::day_put(&tx, day, &v)?;
        } else if is_srs(&k) {
            if let Some(cm) = v.get("c").and_then(Value::as_object) {
                for (id, raw) in cm {
                    if let Ok(card) = serde_json::from_value::<Card>(raw.clone()) {
                        db::card_put(&tx, id, Some(&card))?;
                    }
                }
            }
        } else {
            db::doc_put(&tx, &k, &v)?;
        }
    }
    if let Some(d) = j.get("desktop") {
        tx.execute_batch("DELETE FROM events; DELETE FROM occ_done;").map_err(err)?;
        if let Some(evs) = d.get("events").and_then(Value::as_array) {
            for e in evs {
                if let Ok(ev) = serde_json::from_value::<Event>(e.clone()) {
                    db::event_put(&tx, &ev)?;
                }
            }
        }
        if let Some(done) = d.get("occDone").and_then(Value::as_array) {
            for x in done {
                if let (Some(e), Some(o)) = (x.get(0).and_then(Value::as_str), x.get(1).and_then(Value::as_str)) {
                    let at = x.get(2).and_then(Value::as_i64).unwrap_or(now as i64);
                    tx.execute(
                        "INSERT OR REPLACE INTO occ_done(event_id,occ,done_at) VALUES(?1,?2,?3)",
                        params![e, o, at],
                    )
                    .map_err(err)?;
                }
            }
        }
    }
    crate::sync::sync_events(&tx)?;
    tx.commit().map_err(err)?;
    Ok(())
}

/// "Zerar progresso": everything but the settings (profile keeps its
/// configuration with XP reset, like the artifact).
pub fn reset_progress(c: &mut Connection) -> R<()> {
    let tx = c.transaction().map_err(err)?;
    let mut p = db::profile(&tx);
    p.xp = 0.0;
    p.u = Some(michi_core::time::now_ms());
    tx.execute_batch(
        "DELETE FROM days; DELETE FROM cards; DELETE FROM occ_done;
         DELETE FROM docs WHERE name NOT IN ('profile','desk');",
    )
    .map_err(err)?;
    db::doc_put(&tx, "profile", &serde_json::to_value(&p).map_err(err)?)?;
    tx.commit().map_err(err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    const ARTIFACT_BACKUP: &str = r#"{"app":"nihongo-michi","v":1,"at":1764000000000,"docs":{
      "profile":{"name":"Mirage","xp":420,"routine":[{"id":"srs","time":"06:45","min":20,"jp":"復習","name":"Revisões","go":"revisar","core":true,"days":[1,2,3,4,5]}],"u":1},
      "journey":{"done":{"K01":1},"best":{"K01":90},"points":{},"stamps":{"K01":1763000000000},"u":1},
      "kana":{"s":{"あ":[10,9]},"u":1},
      "mined":{"list":[],"u":1},
      "calendar":{"ids":{},"sig":{},"u":1},
      "srs3":{"c":{"k:日":[1764100000000,12.5,2500,4,0,2,0]},"u":1},
      "srs5":{"c":{"v:食べる|たべる":[1764000000000,0,2500,1,0,1,1]},"u":1},
      "day:2026-11-24":{"k":"2026-11-24","xp":35,"rev":20,"nw":{"kanji":5},"sec":{"srs":900},"done":{"srs":true},"imm":[],"games":1,"sent":0,"gp":0,"notified":{"srs":1},"u":1}
    }}"#;

    #[test]
    fn restores_artifact_backup_and_exports_compatible() {
        let dir = std::env::temp_dir().join(format!("michi-bk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("b.json");
        std::fs::write(&f, ARTIFACT_BACKUP).unwrap();
        let (j, info) = inspect(f.to_str().unwrap()).unwrap();
        assert_eq!((info.days, info.cards, info.xp), (1, 2, 420.0));
        assert!(!info.desktop);

        let db = Db::memory();
        {
            let mut c = db.lock();
            restore(&mut c, &j).unwrap();
        }
        let c = db.lock();
        assert_eq!(db::profile(&c).xp, 420.0);
        assert_eq!(db::cards_all(&c).unwrap().len(), 2);
        assert!(db::day_get(&c, "2026-11-24").unwrap().block_done("srs"));
        assert_eq!(db::event_get(&c, "rotina-srs").unwrap().time, "06:45");
        assert!(db::event_get(&c, "rotina-vlog").is_none());

        let out = export(&c).unwrap();
        assert_eq!(chunk_of("k:日"), "srs6");
        assert_eq!(out["docs"]["srs6"]["c"]["k:日"][1], 12.5);
        assert_eq!(out["docs"]["day:2026-11-24"]["notified"]["srs"], 1);
        assert_eq!(out["docs"]["journey"]["best"]["K01"], 90);
        assert!(out["desktop"]["events"].as_array().unwrap().len() >= 30);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_other_files() {
        let f = std::env::temp_dir().join(format!("michi-x-{}.json", std::process::id()));
        std::fs::write(&f, r#"{"app":"outra"}"#).unwrap();
        assert!(inspect(f.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(f);
    }
}
