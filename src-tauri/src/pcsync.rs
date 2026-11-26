//! Sync between PCs through a shared folder (OneDrive, Google Drive, Dropbox,
//! a USB drive…). Each PC writes only its own file, `michi-<pc>-<id>.sync.json`
//! (a full copy of the progress with the version of every record), and reads
//! the others', so the cloud never sees two PCs writing the same file.
//!
//! Per record, the side that changed since the last sync wins; when both
//! changed, `michi_core::merge` keeps what each side did. Resetting the
//! progress or restoring a backup starts a new "epoch": PCs still on the old
//! one take the new progress (after a safety copy) instead of merging it.

use crate::autobackup::{self, write_atomic};
use crate::db::{self, err, Db, R};
use crate::local::{self, Local};
use michi_core::agenda::Event;
use michi_core::merge::{merge_day, merge_json, merge_profile};
use michi_core::time::{now_ms, today_key};
use michi_core::{Card, DayDoc, Profile};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub const FORMAT: &str = "nihongo-michi-sync";
const SUFFIX: &str = ".sync.json";

/* ---------------- records ---------------- */

/// One record and its version (ms of the last change). `v: None` = deleted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rec<T> {
    pub t: i64,
    // A missing `v` reads as None (deleted) without `default`, which would
    // ask `T: Default` of every record type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v: Option<T>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub docs: BTreeMap<String, Rec<Value>>,
    pub days: BTreeMap<String, Rec<Value>>,
    pub cards: BTreeMap<String, Rec<[f64; 7]>>,
    pub events: BTreeMap<String, Rec<Event>>,
    /// "<event_id>|<occ>" → done
    pub occ: BTreeMap<String, Rec<bool>>,
}

impl Data {
    pub fn max_t(&self) -> i64 {
        let a = self.docs.values().map(|r| r.t);
        let b = self.days.values().map(|r| r.t);
        let c = self.cards.values().map(|r| r.t);
        let d = self.events.values().map(|r| r.t);
        let e = self.occ.values().map(|r| r.t);
        a.chain(b).chain(c).chain(d).chain(e).max().unwrap_or(0)
    }

    /// (total XP, study days with XP, SRS cards) for the "which PC has what" list.
    pub fn summary(&self) -> (f64, usize, usize) {
        let num = |v: &Option<Value>| v.as_ref().and_then(|v| v.get("xp")).and_then(Value::as_f64).unwrap_or(0.0);
        let xp = self.docs.get("profile").map(|r| num(&r.v)).unwrap_or(0.0);
        let days = self.days.values().filter(|r| num(&r.v) > 0.0).count();
        let cards = self.cards.values().filter(|r| r.v.is_some()).count();
        (xp, days, cards)
    }

    /// Nothing studied yet (a fresh install).
    pub fn is_fresh(&self) -> bool {
        let (xp, days, cards) = self.summary();
        xp < 1.0 && days == 0 && cards == 0
    }
}

/// A PC's file in the shared folder.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub app: String,
    pub v: u32,
    pub device_id: String,
    pub device_name: String,
    #[serde(default)]
    pub app_version: String,
    pub at: f64,
    #[serde(default)]
    pub epoch: i64,
    #[serde(default)]
    pub data: Data,
}

/// Everything that syncs, read from the database.
pub fn read_data(c: &Connection) -> R<Data> {
    let mut d = Data::default();
    {
        let mut st = c.prepare("SELECT name, data, updated_at FROM docs").map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)));
        for (n, s, t) in rows.map_err(err)?.flatten() {
            if let Ok(v) = serde_json::from_str(&s) {
                d.docs.insert(n, Rec { t, v: Some(v) });
            }
        }
    }
    {
        let mut st = c.prepare("SELECT day, data, updated_at FROM days").map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)));
        for (k, s, t) in rows.map_err(err)?.flatten() {
            if let Ok(v) = serde_json::from_str(&s) {
                d.days.insert(k, Rec { t, v: Some(v) });
            }
        }
    }
    {
        let mut st = c.prepare("SELECT id,due,ivl,ease,reps,lapses,state,step,updated_at FROM cards").map_err(err)?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get::<_, i64>(6)? as f64, r.get(7)?],
                r.get::<_, i64>(8)?,
            ))
        });
        for (id, card, t) in rows.map_err(err)?.flatten() {
            d.cards.insert(id, Rec { t, v: Some(card) });
        }
    }
    {
        let times: HashMap<String, i64> = {
            let mut st = c.prepare("SELECT id, updated_at FROM events").map_err(err)?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(err)?;
            rows.flatten().collect()
        };
        for e in db::events_all(c)? {
            let t = times.get(&e.id).copied().unwrap_or(0);
            d.events.insert(e.id.clone(), Rec { t, v: Some(e) });
        }
    }
    {
        let mut st = c.prepare("SELECT event_id, occ, done_at FROM occ_done").map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)));
        for (e, o, t) in rows.map_err(err)?.flatten() {
            d.occ.insert(format!("{e}|{o}"), Rec { t, v: Some(true) });
        }
    }
    for (id, t) in db::tombs(c, "card")? {
        d.cards.entry(id).or_insert(Rec { t, v: None });
    }
    for (id, t) in db::tombs(c, "event")? {
        d.events.entry(id).or_insert(Rec { t, v: None });
    }
    for (id, t) in db::tombs(c, "occ")? {
        d.occ.entry(id).or_insert(Rec { t, v: None });
    }
    Ok(d)
}

/// Newest change anywhere in the synced tables (cheap "anything new?" check).
pub fn local_max_t(c: &Connection) -> i64 {
    c.query_row(
        "SELECT MAX(x) FROM (
           SELECT MAX(updated_at) x FROM docs UNION ALL SELECT MAX(updated_at) FROM days UNION ALL
           SELECT MAX(updated_at) FROM cards UNION ALL SELECT MAX(updated_at) FROM events UNION ALL
           SELECT MAX(done_at) FROM occ_done UNION ALL SELECT MAX(at) FROM tombstones)",
        [],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
    .unwrap_or(0)
}

/* ---------------- merge ---------------- */

/// Keys changed by a merge, per table.
#[derive(Debug, Default)]
pub struct Merge {
    pub docs: BTreeSet<String>,
    pub days: BTreeSet<String>,
    pub cards: BTreeSet<String>,
    pub events: BTreeSet<String>,
    pub occ: BTreeSet<String>,
}

impl Merge {
    pub fn count(&self) -> usize {
        self.docs.len() + self.days.len() + self.cards.len() + self.events.len() + self.occ.len()
    }
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }
}

fn newer_first<'a, T: Serialize>(a: &'a Rec<T>, b: &'a Rec<T>) -> (&'a Rec<T>, &'a Rec<T>) {
    if a.t != b.t {
        return if a.t > b.t { (a, b) } else { (b, a) };
    }
    // Same instant on two PCs: any fixed rule works, as long as both pick the same.
    let (sa, sb) = (serde_json::to_string(&a.v).unwrap_or_default(), serde_json::to_string(&b.v).unwrap_or_default());
    if sa >= sb {
        (a, b)
    } else {
        (b, a)
    }
}

/// Brings `remote` into `local`, table by table. `base` holds each record's
/// version at the last sync: a side "changed" when its version is newer.
fn merge_map<T: Clone + PartialEq + Serialize>(
    cat: &str,
    local: &mut BTreeMap<String, Rec<T>>,
    remote: &BTreeMap<String, Rec<T>>,
    base: &HashMap<String, i64>,
    resolve: impl Fn(&str, &Rec<T>, &Rec<T>) -> Option<T>,
    changed: &mut BTreeSet<String>,
) {
    for (k, r) in remote {
        let next = match local.get(k) {
            None => r.clone(),
            Some(l) if l.t == r.t => continue,
            Some(l) => {
                let b = base.get(&format!("{cat}:{k}")).copied();
                if b.is_some_and(|b| r.t <= b) {
                    continue; // the other PC did not touch it since the last sync
                }
                if b.is_some_and(|b| l.t <= b) {
                    r.clone() // only the other PC changed it
                } else {
                    // Changed on both PCs (or never synced before): merge.
                    let (n, o) = newer_first(l, r);
                    let v = resolve(k, n, o);
                    if v == n.v {
                        n.clone()
                    } else {
                        // Newer than both, so every PC takes the merged version.
                        Rec { t: l.t.max(r.t) + 1, v }
                    }
                }
            }
        };
        if local.get(k) != Some(&next) {
            local.insert(k.clone(), next);
            changed.insert(k.clone());
        }
    }
}

fn resolve_doc(name: &str, n: &Rec<Value>, o: &Rec<Value>) -> Option<Value> {
    let (Some(nv), Some(ov)) = (&n.v, &o.v) else { return n.v.clone() };
    Some(match name {
        "profile" => {
            match (serde_json::from_value::<Profile>(nv.clone()), serde_json::from_value::<Profile>(ov.clone())) {
                (Ok(a), Ok(b)) => serde_json::to_value(merge_profile(&a, &b)).unwrap_or_else(|_| nv.clone()),
                _ => nv.clone(),
            }
        }
        // Settings: the newer choice wins.
        "desk" => nv.clone(),
        _ => merge_json(nv, ov),
    })
}

fn resolve_day(_: &str, n: &Rec<Value>, o: &Rec<Value>) -> Option<Value> {
    let (Some(nv), Some(ov)) = (&n.v, &o.v) else { return n.v.clone() };
    match (serde_json::from_value::<DayDoc>(nv.clone()), serde_json::from_value::<DayDoc>(ov.clone())) {
        (Ok(a), Ok(b)) => Some(serde_json::to_value(merge_day(&a, &b)).unwrap_or_else(|_| nv.clone())),
        _ => Some(nv.clone()),
    }
}

fn resolve_newer<T: Clone>(_: &str, n: &Rec<T>, _: &Rec<T>) -> Option<T> {
    n.v.clone()
}

pub fn merge_all(local: &mut Data, remote: &Data, base: &HashMap<String, i64>, m: &mut Merge) {
    merge_map("doc", &mut local.docs, &remote.docs, base, resolve_doc, &mut m.docs);
    merge_map("day", &mut local.days, &remote.days, base, resolve_day, &mut m.days);
    merge_map("card", &mut local.cards, &remote.cards, base, resolve_newer, &mut m.cards);
    merge_map("event", &mut local.events, &remote.events, base, resolve_newer, &mut m.events);
    merge_map("occ", &mut local.occ, &remote.occ, base, resolve_newer, &mut m.occ);
}

/// The profile total must cover the study days (days studied on another PC
/// may arrive with a profile that has not seen them yet).
fn fix_xp(d: &mut Data, m: &mut Merge) {
    if m.days.is_empty() {
        return;
    }
    let sum: f64 = d
        .days
        .values()
        .filter_map(|r| r.v.as_ref().and_then(|v| v.get("xp")).and_then(Value::as_f64))
        .sum::<f64>()
        .round();
    let Some(rec) = d.docs.get_mut("profile") else { return };
    let Some(Value::Object(p)) = rec.v.as_mut() else { return };
    let xp = p.get("xp").and_then(Value::as_f64).unwrap_or(0.0);
    if sum > xp {
        p.insert("xp".into(), serde_json::json!(sum));
        rec.t += 1;
        m.docs.insert("profile".into());
    }
}

/* ---------------- database side ---------------- */

pub fn epoch(c: &Connection) -> i64 {
    db::meta_get(c, "sync_epoch").and_then(|s| s.parse().ok()).unwrap_or(0)
}

fn set_epoch(c: &Connection, e: i64) -> R<()> {
    db::meta_set(c, "sync_epoch", &e.to_string())
}

/// After a reset or a restored backup: this progress replaces the other PCs'.
pub fn start_new_epoch(c: &Connection) -> R<()> {
    set_epoch(c, db::now())?;
    c.execute("DELETE FROM sync_base", []).map_err(err)?;
    c.execute("DELETE FROM tombstones", []).map_err(err)?;
    Ok(())
}

fn base_load(c: &Connection) -> R<HashMap<String, i64>> {
    let mut st = c.prepare("SELECT key, t FROM sync_base").map_err(err)?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(err)?;
    Ok(rows.flatten().collect())
}

fn base_save(c: &Connection, d: &Data) -> R<()> {
    let tx = c.unchecked_transaction().map_err(err)?;
    tx.execute("DELETE FROM sync_base", []).map_err(err)?;
    {
        let mut st = tx.prepare("INSERT INTO sync_base(key,t) VALUES(?1,?2)").map_err(err)?;
        let mut put = |cat: &str, k: &str, t: i64| st.execute(params![format!("{cat}:{k}"), t]).map(|_| ());
        for (k, r) in &d.docs {
            put("doc", k, r.t).map_err(err)?;
        }
        for (k, r) in &d.days {
            put("day", k, r.t).map_err(err)?;
        }
        for (k, r) in &d.cards {
            put("card", k, r.t).map_err(err)?;
        }
        for (k, r) in &d.events {
            put("event", k, r.t).map_err(err)?;
        }
        for (k, r) in &d.occ {
            put("occ", k, r.t).map_err(err)?;
        }
    }
    tx.commit().map_err(err)
}

fn wipe(c: &Connection) -> R<()> {
    c.execute_batch(
        "DELETE FROM docs; DELETE FROM days; DELETE FROM cards; DELETE FROM events;
         DELETE FROM occ_done; DELETE FROM tombstones; DELETE FROM sync_base;",
    )
    .map_err(err)
}

fn apply_doc(c: &Connection, k: &str, r: &Rec<Value>) -> R<()> {
    match &r.v {
        Some(v) => db::doc_put_at(c, k, v, r.t),
        None => c.execute("DELETE FROM docs WHERE name=?1", [k]).map(|_| ()).map_err(err),
    }
}

fn apply_day(c: &Connection, k: &str, r: &Rec<Value>) -> R<()> {
    match &r.v {
        Some(v) => db::day_put_at(c, k, v, r.t),
        None => c.execute("DELETE FROM days WHERE day=?1", [k]).map(|_| ()).map_err(err),
    }
}

fn apply_occ(c: &Connection, k: &str, r: &Rec<bool>) -> R<()> {
    let Some((e, o)) = k.rsplit_once('|') else { return Ok(()) };
    db::occ_done_set_at(c, e, o, r.v == Some(true), r.t)
}

fn apply_event(c: &Connection, k: &str, r: &Rec<Event>) -> R<()> {
    match &r.v {
        Some(e) => db::event_put_at(c, e, r.t),
        None => db::event_delete_at(c, k, r.t),
    }
}

/// Writes the changed records (all of them when `m` is None).
fn apply(c: &Connection, d: &Data, m: Option<&Merge>) -> R<()> {
    let pick = |set: Option<&BTreeSet<String>>, k: &str| set.map(|s| s.contains(k)).unwrap_or(true);
    for (k, r) in &d.docs {
        if pick(m.map(|m| &m.docs), k) {
            apply_doc(c, k, r)?;
        }
    }
    for (k, r) in &d.days {
        if pick(m.map(|m| &m.days), k) {
            apply_day(c, k, r)?;
        }
    }
    for (k, r) in &d.cards {
        if pick(m.map(|m| &m.cards), k) {
            db::card_put_at(c, k, r.v.map(Card).as_ref(), r.t)?;
        }
    }
    for (k, r) in &d.events {
        if pick(m.map(|m| &m.events), k) {
            apply_event(c, k, r)?;
        }
    }
    for (k, r) in &d.occ {
        if pick(m.map(|m| &m.occ), k) {
            apply_occ(c, k, r)?;
        }
    }
    Ok(())
}

/// Reminders of things already done on the other PC leave the popup.
fn close_done_reminders(c: &Connection, d: &Data, m: Option<&Merge>) -> R<()> {
    let recent = [today_key(), michi_core::time::key_add(&today_key(), -1)];
    for day in &recent {
        if m.map(|m| m.days.contains(day)).unwrap_or(true) {
            crate::agenda::close_done_routine(c, day)?;
        }
    }
    for (k, r) in &d.occ {
        if r.v == Some(true) && m.map(|m| m.occ.contains(k)).unwrap_or(true) {
            if let Some((e, o)) = k.rsplit_once('|') {
                db::notif_close_occ(c, e, o, "done")?;
            }
        }
    }
    Ok(())
}

/* ---------------- files ---------------- */

fn slug(s: &str) -> String {
    let t: String =
        s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).take(24).collect();
    let t = t.trim_matches('-').to_string();
    if t.is_empty() {
        "pc".into()
    } else {
        t
    }
}

pub fn own_file(dir: &Path, l: &Local) -> PathBuf {
    dir.join(format!("michi-{}-{}{SUFFIX}", slug(&l.device_name), l.device_id))
}

type Stamp = (u64, u128);

fn stamp(p: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some((m.len(), t))
}

fn sync_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let n = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            n.starts_with("michi-") && n.ends_with(SUFFIX)
        })
        .collect();
    v.sort();
    v
}

/// A sync file larger than this is not read (a whole progress is a few MB).
const MAX_FILE: u64 = 64 * 1024 * 1024;

pub fn read_snapshot(p: &Path) -> R<Snapshot> {
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    if size > MAX_FILE {
        return Err(format!("{} é grande demais para um arquivo de sincronização", p.display()));
    }
    let text = std::fs::read_to_string(p).map_err(|e| format!("não consegui ler {}: {e}", p.display()))?;
    let s: Snapshot = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("arquivo de sincronização inválido ({}): {e}", p.display()))?;
    if s.app != FORMAT || s.v != 1 {
        return Err(format!("{} não é um arquivo de sincronização do Michi", p.display()));
    }
    // An epoch is the time of a reset or restore: one from the future would
    // make every PC replace its progress with that file forever.
    if s.epoch < 0 || s.epoch as f64 > now_ms() + 86_400_000.0 {
        return Err(format!("{} tem uma data de época inválida", p.display()));
    }
    Ok(s)
}

/* ---------------- state ---------------- */

#[derive(Clone, Debug, Default, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub at: f64,
    pub version: String,
    pub xp: f64,
    pub days: usize,
    pub cards: usize,
    pub this: bool,
}

fn device_info(s: &Snapshot, this: bool) -> DeviceInfo {
    let (xp, days, cards) = s.data.summary();
    DeviceInfo {
        id: s.device_id.clone(),
        name: s.device_name.clone(),
        at: s.at,
        version: s.app_version.clone(),
        xp,
        days,
        cards,
        this,
    }
}

#[derive(Default)]
struct Info {
    last_at: f64,
    error: String,
    pending: Vec<String>,
    devices: Vec<DeviceInfo>,
}

/// Sync state kept while the app runs.
#[derive(Default)]
pub struct SyncState {
    run: Mutex<()>,
    /// Other PCs' files, parsed, by path (re-read only when they change).
    cache: Mutex<HashMap<PathBuf, (Stamp, Snapshot)>>,
    /// Stamps of the other PCs' files at the last full sync.
    seen: Mutex<HashMap<PathBuf, Stamp>>,
    info: Mutex<Info>,
}

impl SyncState {
    /// The other PCs' files (bad or half-synced files are skipped this time).
    fn remotes(&self, dir: &Path, me: &str) -> (Vec<Snapshot>, HashMap<PathBuf, Stamp>, Vec<String>) {
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let mut out = Vec::new();
        let mut stamps = HashMap::new();
        let mut errors = Vec::new();
        for p in sync_files(dir) {
            let Some(st) = stamp(&p) else { continue };
            stamps.insert(p.clone(), st);
            let fresh = cache.get(&p).map(|(s, _)| *s == st).unwrap_or(false);
            if !fresh {
                match read_snapshot(&p) {
                    Ok(s) => {
                        cache.insert(p.clone(), (st, s));
                    }
                    Err(e) => {
                        errors.push(e);
                        continue;
                    }
                }
            }
            if let Some((_, s)) = cache.get(&p) {
                if s.device_id != me {
                    out.push(s.clone());
                }
            }
        }
        cache.retain(|p, _| stamps.contains_key(p));
        (out, stamps, errors)
    }

    /// Did another PC's file change since the last full sync?
    pub fn remote_changed(&self, dir: &Path, me_file: &Path) -> bool {
        let seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        let files: Vec<PathBuf> = sync_files(dir).into_iter().filter(|p| p != me_file).collect();
        files.len() != seen.len() || files.iter().any(|p| seen.get(p) != stamp(p).as_ref())
    }

    fn set_info(&self, f: impl FnOnce(&mut Info)) {
        f(&mut self.info.lock().unwrap_or_else(|p| p.into_inner()));
    }
}

/* ---------------- sync ---------------- */

/// How this sync treats the other PCs' progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Normal sync.
    Normal,
    /// First time with this folder: join both sides, whatever their epochs.
    JoinMerge,
    /// First time with this folder: replace this PC's progress with the folder's.
    JoinAdopt,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub on: bool,
    /// Records brought from the other PCs.
    pub received: usize,
    /// This PC's file was written.
    pub sent: bool,
    /// Changes from other PCs wait for the window to close (or for "Atualizar").
    pub pending: Vec<String>,
}

fn version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

/// What a sync round needs from the app, kept apart so the tests can run
/// whole rounds between two databases without a window.
pub struct Ctx<'a> {
    pub version: String,
    /// Safety copy before this PC's progress is replaced or merged.
    pub safety: &'a dyn Fn(&Connection, &str) -> R<()>,
}

fn snapshot_text(version: &str, l: &Local, epoch: i64, data: &Data) -> R<String> {
    let s = Snapshot {
        app: FORMAT.into(),
        v: 1,
        device_id: l.device_id.clone(),
        device_name: l.device_name.clone(),
        app_version: version.to_string(),
        at: now_ms(),
        epoch,
        data: data.clone(),
    };
    serde_json::to_string(&s).map_err(err)
}

/// One sync round. With `allow_apply` false (window open), changes from the
/// other PCs are not applied: the UI is told and decides when.
pub fn sync_once(app: &AppHandle, allow_apply: bool, mode: Mode) -> R<Report> {
    let safety = |c: &Connection, what: &str| autobackup::safety(app, c, what).map(|_| ());
    let ctx = Ctx { version: version(app), safety: &safety };
    run(&ctx, &app.state::<SyncState>(), &app.state::<Db>(), allow_apply, mode)
}

pub fn run(ctx: &Ctx, st: &SyncState, db: &Db, allow_apply: bool, mode: Mode) -> R<Report> {
    let _running = st.run.lock().unwrap_or_else(|p| p.into_inner());
    let l = local::get(&db.lock());
    if !l.sync_on() {
        return Ok(Report::default());
    }
    let dir = PathBuf::from(l.sync_dir.trim());
    let res = sync_inner(ctx, st, db, &l, &dir, allow_apply, mode);
    match &res {
        Ok(r) => st.set_info(|i| {
            i.error.clear();
            i.pending = r.pending.clone();
            if r.pending.is_empty() {
                i.last_at = now_ms();
            }
        }),
        Err(e) => st.set_info(|i| i.error = e.clone()),
    }
    res
}

fn sync_inner(ctx: &Ctx, st: &SyncState, db: &Db, l: &Local, dir: &Path, allow_apply: bool, mode: Mode) -> R<Report> {
    if !dir.is_dir() {
        return Err(format!("a pasta {} não está acessível", dir.display()));
    }
    let (remotes, stamps, _bad) = st.remotes(dir, &l.device_id);
    let own = own_file(dir, l);
    st.set_info(|i| {
        let mut devs: Vec<DeviceInfo> = remotes.iter().map(|s| device_info(s, false)).collect();
        devs.sort_by(|a, b| b.at.partial_cmp(&a.at).unwrap_or(std::cmp::Ordering::Equal));
        i.devices = devs;
    });

    let mut c = db.lock();
    let local_epoch = epoch(&c);
    let top_remote = remotes.iter().map(|r| r.epoch).max();
    let top = top_remote.unwrap_or(local_epoch).max(local_epoch);
    let adopt = mode == Mode::JoinAdopt || (mode == Mode::Normal && top > local_epoch);
    if mode == Mode::JoinAdopt && remotes.is_empty() {
        return Err("a pasta não tem o progresso de outro PC".into());
    }
    // Which files take part: all of them when joining, else those of the newest epoch.
    let joining: Vec<&Snapshot> =
        remotes.iter().filter(|r| mode == Mode::JoinMerge || mode == Mode::JoinAdopt || r.epoch == top).collect();
    let (mut data, base) = if adopt {
        (Data::default(), HashMap::new())
    } else if mode == Mode::JoinMerge {
        (read_data(&c)?, HashMap::new())
    } else {
        (read_data(&c)?, base_load(&c)?)
    };
    let mut m = Merge::default();
    for r in &joining {
        merge_all(&mut data, &r.data, &base, &mut m);
    }
    fix_xp(&mut data, &mut m);
    let changes = adopt || !m.is_empty() || (mode == Mode::JoinMerge && top != local_epoch);
    let mut report = Report { on: true, received: m.count(), ..Default::default() };

    if changes && !allow_apply {
        // Not now: send this PC's own changes anyway (without moving the base).
        report.received = 0;
        report.pending = joining.iter().map(|s| s.device_name.clone()).collect();
        report.pending.dedup();
        let dirty = local_max_t(&c) > db::meta_get(&c, "sync_pushed").and_then(|s| s.parse().ok()).unwrap_or(-1);
        if dirty || !own.exists() {
            let mine = read_data(&c)?;
            let text = snapshot_text(&ctx.version, l, local_epoch, &mine)?;
            let max_t = mine.max_t();
            drop(c);
            write_atomic(&own, &text)?;
            db::meta_set(&db.lock(), "sync_pushed", &max_t.to_string())?;
            report.sent = true;
        }
        return Ok(report);
    }

    if changes {
        if adopt || mode == Mode::JoinMerge {
            (ctx.safety)(&c, "sincronizar")?;
        }
        let tx = c.transaction().map_err(err)?;
        if adopt {
            wipe(&tx)?;
            apply(&tx, &data, None)?;
        } else {
            apply(&tx, &data, Some(&m))?;
        }
        if mode == Mode::JoinAdopt {
            // This PC joins the folder's line (it does not start a new one).
            set_epoch(&tx, top_remote.unwrap_or(local_epoch))?;
        } else if adopt || mode == Mode::JoinMerge {
            set_epoch(&tx, top)?;
        }
        crate::sync::sync_events(&tx)?;
        close_done_reminders(&tx, &data, if adopt { None } else { Some(&m) })?;
        tx.commit().map_err(err)?;
    }

    // This PC's file: written when something changed here or arrived.
    let mine = read_data(&c)?;
    let max_t = mine.max_t();
    let pushed: i64 = db::meta_get(&c, "sync_pushed").and_then(|s| s.parse().ok()).unwrap_or(-1);
    let need = changes || mode != Mode::Normal || max_t > pushed || !own.exists();
    let ep = epoch(&c);
    drop(c);
    if need {
        let text = snapshot_text(&ctx.version, l, ep, &mine)?;
        write_atomic(&own, &text)?;
        let c = db.lock();
        base_save(&c, &mine)?;
        db::meta_set(&c, "sync_pushed", &max_t.to_string())?;
        report.sent = true;
    }
    *st.seen.lock().unwrap_or_else(|p| p.into_inner()) = stamps.into_iter().filter(|(p, _)| p != &own).collect();
    Ok(report)
}

/// Writes this PC's file without reading the others' (on exit).
pub fn push_only(app: &AppHandle) -> R<bool> {
    push_only_with(&version(app), &app.state::<SyncState>(), &app.state::<Db>())
}

fn push_only_with(version: &str, st: &SyncState, db: &Db) -> R<bool> {
    let _running = st.run.lock().unwrap_or_else(|p| p.into_inner());
    let c = db.lock();
    let l = local::get(&c);
    if !l.sync_on() {
        return Ok(false);
    }
    let dir = PathBuf::from(l.sync_dir.trim());
    let own = own_file(&dir, &l);
    let pushed: i64 = db::meta_get(&c, "sync_pushed").and_then(|s| s.parse().ok()).unwrap_or(-1);
    if local_max_t(&c) <= pushed && own.exists() {
        return Ok(false);
    }
    let mine = read_data(&c)?;
    let text = snapshot_text(version, &l, epoch(&c), &mine)?;
    drop(c);
    write_atomic(&own, &text)?;
    db::meta_set(&db.lock(), "sync_pushed", &mine.max_t().to_string())?;
    Ok(true)
}

/* ---------------- what the UI sees ---------------- */

#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub on: bool,
    pub dir: String,
    pub device_name: String,
    pub last_at: f64,
    pub error: String,
    pub pending: Vec<String>,
    pub devices: Vec<DeviceInfo>,
}

pub fn status(app: &AppHandle) -> Status {
    let db = app.state::<Db>();
    let l = local::get(&db.lock());
    let st = app.state::<SyncState>();
    let i = st.info.lock().unwrap_or_else(|p| p.into_inner());
    Status {
        on: l.sync_on(),
        dir: l.sync_dir.clone(),
        device_name: l.device_name.clone(),
        last_at: i.last_at,
        error: i.error.clone(),
        pending: i.pending.clone(),
        devices: i.devices.clone(),
    }
}

/// What a folder holds, before using it.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Preview {
    pub dir: String,
    pub devices: Vec<DeviceInfo>,
    pub here: DeviceInfo,
    pub fresh: bool,
}

pub fn preview(app: &AppHandle, dir: &Path) -> R<Preview> {
    if !dir.is_dir() {
        return Err("pasta não encontrada".into());
    }
    let db = app.state::<Db>();
    let (l, mine) = {
        let c = db.lock();
        (local::get(&c), read_data(&c)?)
    };
    let mut devices = Vec::new();
    for p in sync_files(dir) {
        if let Ok(s) = read_snapshot(&p) {
            if s.device_id != l.device_id {
                devices.push(device_info(&s, false));
            }
        }
    }
    devices.sort_by(|a, b| b.at.partial_cmp(&a.at).unwrap_or(std::cmp::Ordering::Equal));
    let (xp, days, cards) = mine.summary();
    let here = DeviceInfo {
        id: l.device_id.clone(),
        name: l.device_name.clone(),
        at: now_ms(),
        version: version(app),
        xp,
        days,
        cards,
        this: true,
    };
    Ok(Preview { dir: dir.display().to_string(), devices, here, fresh: mine.is_fresh() })
}

/// Starts syncing with `dir`. `how`: "merge" (join both), "adopt" (take the
/// folder's progress) or "mine" (this PC's progress replaces the others').
pub fn join(app: &AppHandle, dir: &Path, how: &str) -> R<Report> {
    let safety = |c: &Connection, what: &str| autobackup::safety(app, c, what).map(|_| ());
    let ctx = Ctx { version: version(app), safety: &safety };
    join_with(&ctx, &app.state::<SyncState>(), &app.state::<Db>(), dir, how)
}

fn join_with(ctx: &Ctx, st: &SyncState, db: &Db, dir: &Path, how: &str) -> R<Report> {
    if !dir.is_dir() {
        return Err("pasta não encontrada".into());
    }
    {
        let c = db.lock();
        local::update(&c, |l| l.sync_dir = dir.display().to_string())?;
        c.execute("DELETE FROM sync_base", []).map_err(err)?;
        db::meta_set(&c, "sync_pushed", "-1")?;
        if how == "mine" {
            start_new_epoch(&c)?;
        }
    }
    st.seen.lock().unwrap_or_else(|p| p.into_inner()).clear();
    let mode = match how {
        "adopt" => Mode::JoinAdopt,
        "merge" => Mode::JoinMerge,
        _ => Mode::Normal,
    };
    let r = run(ctx, st, db, true, mode);
    if r.is_err() && how == "adopt" {
        // Nothing was replaced; leave sync off so the next start does not try again.
        let c = db.lock();
        let _ = local::update(&c, |l| l.sync_dir.clear());
    }
    r
}

pub fn turn_off(app: &AppHandle) -> R<()> {
    let db = app.state::<Db>();
    let c = db.lock();
    local::update(&c, |l| l.sync_dir.clear())?;
    app.state::<SyncState>().set_info(|i| *i = Info::default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use serde_json::json;

    fn rec<T>(t: i64, v: T) -> Rec<T> {
        Rec { t, v: Some(v) }
    }

    #[test]
    fn merge_picks_the_side_that_changed() {
        let base: HashMap<String, i64> =
            [("day:2026-11-25".to_string(), 100), ("card:k:日".to_string(), 100)].into_iter().collect();
        // Day: only the other PC changed it → taken as is.
        let mut local = Data::default();
        local.days.insert("2026-11-25".into(), rec(100, json!({"k":"2026-11-25","xp":10})));
        local.cards.insert("k:日".into(), rec(150, [1.0, 2.0, 2500.0, 1.0, 0.0, 2.0, 0.0]));
        let mut remote = Data::default();
        remote.days.insert("2026-11-25".into(), rec(200, json!({"k":"2026-11-25","xp":40})));
        // Card: only this PC changed it; the other PC's copy is the old one.
        remote.cards.insert("k:日".into(), rec(100, [0.0; 7]));
        // New record from the other PC, and a deletion.
        remote.cards.insert("k:月".into(), Rec { t: 300, v: None });
        let mut m = Merge::default();
        merge_all(&mut local, &remote, &base, &mut m);
        assert_eq!(local.days["2026-11-25"].v.as_ref().unwrap()["xp"], 40);
        assert_eq!(local.cards["k:日"].t, 150, "this PC's review stays");
        assert!(local.cards["k:月"].v.is_none());
        assert_eq!((m.days.len(), m.cards.len()), (1, 1));
    }

    #[test]
    fn both_changed_merges_and_converges() {
        let base: HashMap<String, i64> = [("day:2026-11-26".to_string(), 100)].into_iter().collect();
        let a = rec(200, json!({"k":"2026-11-26","xp":30,"sec":{"srs":600},"done":{"srs":true}}));
        let b = rec(250, json!({"k":"2026-11-26","xp":20,"sec":{"jogos":300},"done":{"vlog":true}}));
        let mut pa = Data::default();
        pa.days.insert("2026-11-26".into(), a.clone());
        let mut pb = Data::default();
        pb.days.insert("2026-11-26".into(), b.clone());
        let (ra, rb) = (pa.clone(), pb.clone());
        merge_all(&mut pa, &rb, &base, &mut Merge::default());
        merge_all(&mut pb, &ra, &base, &mut Merge::default());
        assert_eq!(pa.days, pb.days, "both PCs end with the same day");
        let d = pa.days["2026-11-26"].clone();
        assert_eq!(d.t, 251);
        let v = d.v.unwrap();
        assert_eq!(v["xp"], 30.0);
        assert_eq!(v["sec"], json!({"jogos":300.0,"srs":600.0}));
        assert_eq!(v["done"], json!({"srs":true,"vlog":true}));
    }

    fn tick() {
        // Records carry ms versions; keep competing changes apart.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    #[test]
    fn full_rounds_between_two_pcs() {
        let dir = std::env::temp_dir().join(format!("michi-rounds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let calls = std::cell::RefCell::new(Vec::<String>::new());
        let safety = |_: &Connection, what: &str| {
            calls.borrow_mut().push(what.to_string());
            Ok(())
        };
        let ctx = Ctx { version: "test".into(), safety: &safety };
        let (a, b) = (Db::memory(), Db::memory());
        let (sa, sb) = (SyncState::default(), SyncState::default());

        // PC A has progress and starts syncing with an empty folder.
        {
            let c = a.lock();
            db::doc_put(&c, "profile", &json!({"name":"Mirage","xp":120,"created":1700000000000.0})).unwrap();
            db::day_put(&c, "2026-11-25", &json!({"k":"2026-11-25","xp":120,"sec":{"srs":600}})).unwrap();
            db::card_put(&c, "k:日", Some(&Card([1.0, 3.0, 2500.0, 2.0, 0.0, 2.0, 0.0]))).unwrap();
            crate::sync::sync_events(&c).unwrap();
        }
        let r = join_with(&ctx, &sa, &a, &dir, "merge").unwrap();
        assert!(r.sent && r.received == 0);
        assert_eq!(sync_files(&dir).len(), 1);

        // PC B (fresh) takes the folder's progress.
        tick();
        let r = join_with(&ctx, &sb, &b, &dir, "adopt").unwrap();
        assert!(r.received > 0 && r.sent);
        assert_eq!(calls.borrow().as_slice(), ["sincronizar"]);
        {
            let c = b.lock();
            assert_eq!(db::profile(&c).xp, 120.0);
            assert_eq!(db::cards_all(&c).unwrap().len(), 1);
            assert!(db::event_get(&c, "rotina-srs").is_some());
        }
        assert_eq!(sync_files(&dir).len(), 2);

        // B studies and deletes the card; A, with its window open, is only told.
        tick();
        {
            let c = b.lock();
            db::day_put(&c, "2026-11-26", &json!({"k":"2026-11-26","xp":30})).unwrap();
            db::card_put(&c, "k:日", None).unwrap();
        }
        assert!(run(&ctx, &sb, &b, true, Mode::Normal).unwrap().sent);
        tick();
        {
            let c = a.lock();
            db::day_put(&c, "2026-11-25", &json!({"k":"2026-11-25","xp":120,"sec":{"srs":900}})).unwrap();
        }
        let r = run(&ctx, &sa, &a, false, Mode::Normal).unwrap();
        assert_eq!(r.received, 0);
        assert_eq!(r.pending.len(), 1, "B's progress waits");
        assert!(r.sent, "A's own change still goes to the folder");
        assert!(db::day_get(&a.lock(), "2026-11-26").is_none());

        // Window closed: A takes B's progress.
        let r = run(&ctx, &sa, &a, true, Mode::Normal).unwrap();
        assert!(r.received >= 2 && r.pending.is_empty());
        {
            let c = a.lock();
            assert_eq!(db::day_get(&c, "2026-11-26").unwrap().xp, 30.0);
            assert!(db::cards_all(&c).unwrap().is_empty());
            assert_eq!(db::profile(&c).xp, 150.0);
        }
        // And B gets A's change from while it waited.
        run(&ctx, &sb, &b, true, Mode::Normal).unwrap();
        assert_eq!(db::day_get(&b.lock(), "2026-11-25").unwrap().sec["srs"], 900.0);
        // Nothing new: no rewrite, nothing received.
        let r = run(&ctx, &sb, &b, true, Mode::Normal).unwrap();
        assert_eq!((r.received, r.sent), (0, false));

        // A resets its progress: B follows (after a safety copy).
        tick();
        {
            let mut c = a.lock();
            crate::backup::reset_progress(&mut c).unwrap();
            start_new_epoch(&c).unwrap();
        }
        run(&ctx, &sa, &a, true, Mode::Normal).unwrap();
        tick();
        let r = run(&ctx, &sb, &b, true, Mode::Normal).unwrap();
        assert!(r.received > 0);
        assert_eq!(calls.borrow().len(), 2);
        {
            let c = b.lock();
            assert_eq!(db::profile(&c).xp, 0.0);
            assert!(db::day_get(&c, "2026-11-26").is_none());
            assert_eq!(epoch(&c), epoch(&a.lock()));
        }
        // Both PCs hold the same progress.
        let (da, dbb) = (read_data(&a.lock()).unwrap(), read_data(&b.lock()).unwrap());
        assert_eq!(da.summary(), dbb.summary());
        assert_eq!(da.days.keys().collect::<Vec<_>>(), dbb.days.keys().collect::<Vec<_>>());
        assert!(push_only_with("test", &sa, &a).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_pcs_through_a_folder() {
        let dir = std::env::temp_dir().join(format!("michi-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (Db::memory(), Db::memory());
        let la = Local { device_id: "aaaa0001".into(), device_name: "MESA".into(), ..Default::default() };
        let lb = Local { device_id: "bbbb0002".into(), device_name: "NOTE".into(), ..Default::default() };
        let snap = |db: &Db, l: &Local| {
            let c = db.lock();
            let d = read_data(&c).unwrap();
            let s = Snapshot {
                app: FORMAT.into(),
                v: 1,
                device_id: l.device_id.clone(),
                device_name: l.device_name.clone(),
                app_version: "t".into(),
                at: 0.0,
                epoch: epoch(&c),
                data: d,
            };
            std::fs::write(own_file(&dir, l), serde_json::to_string(&s).unwrap()).unwrap();
        };
        // PC A studies; PC B is fresh.
        {
            let c = a.lock();
            db::doc_put(&c, "profile", &json!({"name":"Mirage","xp":120})).unwrap();
            db::day_put(&c, "2026-11-25", &json!({"k":"2026-11-25","xp":120})).unwrap();
            db::card_put(&c, "k:日", Some(&Card([1.0, 3.0, 2500.0, 2.0, 0.0, 2.0, 0.0]))).unwrap();
            crate::sync::sync_events(&c).unwrap();
        }
        snap(&a, &la);
        let sa = read_snapshot(&own_file(&dir, &la)).unwrap();
        assert!(Data::default().is_fresh());
        assert!(!sa.data.is_fresh());
        // B takes A's progress (adopt): everything arrives, base saved.
        {
            let mut c = b.lock();
            let mut data = Data::default();
            let mut m = Merge::default();
            merge_all(&mut data, &sa.data, &HashMap::new(), &mut m);
            let tx = c.transaction().unwrap();
            wipe(&tx).unwrap();
            apply(&tx, &data, None).unwrap();
            tx.commit().unwrap();
            base_save(&c, &read_data(&c).unwrap()).unwrap();
            assert_eq!(db::profile(&c).xp, 120.0);
            assert_eq!(db::cards_all(&c).unwrap().len(), 1);
            assert!(db::event_get(&c, "rotina-srs").is_some());
        }
        // B deletes the card and studies more; A syncs and gets both.
        {
            let c = b.lock();
            db::card_put(&c, "k:日", None).unwrap();
            db::day_put(&c, "2026-11-26", &json!({"k":"2026-11-26","xp":30})).unwrap();
        }
        snap(&b, &lb);
        let sb = read_snapshot(&own_file(&dir, &lb)).unwrap();
        {
            let c = a.lock();
            base_save(&c, &read_data(&c).unwrap()).unwrap();
            let mut data = read_data(&c).unwrap();
            let base = base_load(&c).unwrap();
            let mut m = Merge::default();
            merge_all(&mut data, &sb.data, &base, &mut m);
            fix_xp(&mut data, &mut m);
            apply(&c, &data, Some(&m)).unwrap();
            assert!(db::cards_all(&c).unwrap().is_empty(), "the deletion reached A");
            assert_eq!(db::day_get(&c, "2026-11-26").unwrap().xp, 30.0);
            assert_eq!(db::profile(&c).xp, 150.0, "profile total covers the new day");
        }
        assert_eq!(slug("MESA-01"), "mesa-01");
        assert_eq!(slug("Notebook do Escritório"), "notebook-do-escrit-rio");
        let _ = std::fs::remove_dir_all(dir);
    }
}
