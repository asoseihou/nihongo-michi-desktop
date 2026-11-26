//! The UI's copy of the learner's data (port of `Store` in `01-core.js`).
//! Documents live in memory; changes are marked dirty and written to SQLite
//! through the backend a moment later. Changes made by the backend (reminder
//! popup, agenda) arrive as `Op`s and are replayed here.

use crate::docs::{Journey, KanaDoc, Mined};
use crate::tauri;
use leptos::prelude::*;
use michi_core::agenda::DeskSettings;
use michi_core::ops::Op;
use michi_core::time::{now_ms, today_key};
use michi_core::{Card, DayDoc, Profile};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default)]
pub struct Store {
    pub p: Profile,
    pub j: Journey,
    pub ks: KanaDoc,
    pub mined: Mined,
    pub desk: DeskSettings,
    /// Other documents kept as-is (e.g. the artifact's `calendar`).
    pub other: BTreeMap<String, Value>,
    pub days: BTreeMap<String, DayDoc>,
    pub cards: HashMap<String, Card>,
    dirty: BTreeSet<String>,
    dirty_cards: BTreeSet<String>,
}

thread_local! {
    static S: RefCell<Store> = RefCell::new(Store::default());
    static FLUSH_PENDING: Cell<bool> = const { Cell::new(false) };
    static SIGS: Cell<Option<Sigs>> = const { Cell::new(None) };
    static ERROR_SHOWN: Cell<bool> = const { Cell::new(false) };
}

/// Global signals: `view` re-renders data views, `chips` the top bar,
/// `unread` the bell.
#[derive(Clone, Copy)]
pub struct Sigs {
    pub view: RwSignal<u64>,
    pub chips: RwSignal<u64>,
    pub unread: RwSignal<i64>,
    pub loaded: RwSignal<bool>,
}

pub fn init_sigs() -> Sigs {
    let s = Sigs {
        view: RwSignal::new(0),
        chips: RwSignal::new(0),
        unread: RwSignal::new(0),
        loaded: RwSignal::new(false),
    };
    SIGS.with(|c| c.set(Some(s)));
    s
}

pub fn sigs() -> Sigs {
    SIGS.with(|c| c.get()).expect("signals initialised")
}

/// Ask data views to redraw (the artifact's `render(currentRoute())`).
pub fn refresh() {
    if let Some(s) = SIGS.with(|c| c.get()) {
        s.view.update(|v| *v += 1);
        s.chips.update(|v| *v += 1);
    }
}

pub fn refresh_chips() {
    if let Some(s) = SIGS.with(|c| c.get()) {
        s.chips.update(|v| *v += 1);
    }
}

pub fn read<R>(f: impl FnOnce(&Store) -> R) -> R {
    S.with(|s| f(&s.borrow()))
}

/// Mutates documents and marks them for saving.
pub fn write<R>(names: &[&str], f: impl FnOnce(&mut Store) -> R) -> R {
    let r = S.with(|s| f(&mut s.borrow_mut()));
    S.with(|s| {
        let mut s = s.borrow_mut();
        for n in names {
            s.dirty.insert((*n).to_string());
        }
    });
    schedule();
    r
}

pub fn p() -> Profile {
    read(|s| s.p.clone())
}

pub fn j() -> Journey {
    read(|s| s.j.clone())
}

pub fn day(k: &str) -> Option<DayDoc> {
    read(|s| s.days.get(k).cloned())
}

/// Today's study-day document (created on first use, like `today()`).
pub fn today() -> DayDoc {
    let k = today_key();
    read(|s| s.days.get(&k).cloned()).unwrap_or_else(|| DayDoc::new(&k))
}

pub fn with_today<R>(f: impl FnOnce(&mut DayDoc) -> R) -> R {
    let k = today_key();
    let name = format!("day:{k}");
    write(&[&name], |s| f(s.days.entry(k.clone()).or_insert_with(|| DayDoc::new(&k))))
}

pub fn card(id: &str) -> Option<Card> {
    read(|s| s.cards.get(id).copied())
}

pub fn set_card(id: &str, c: Option<Card>) {
    S.with(|s| {
        let mut s = s.borrow_mut();
        match c {
            Some(c) => s.cards.insert(id.to_string(), c),
            None => s.cards.remove(id),
        };
        s.dirty_cards.insert(id.to_string());
    });
    schedule();
}

/// `addXP(n)`: profile total + today.
pub fn add_xp(n: f64) {
    if n == 0.0 {
        return;
    }
    let k = today_key();
    let name = format!("day:{k}");
    write(&["profile", &name], |s| {
        s.p.xp += n;
        s.days.entry(k.clone()).or_insert_with(|| DayDoc::new(&k)).xp += n;
    });
    refresh_chips();
}

pub fn add_xp_at(n: f64, ev: Option<&web_sys::MouseEvent>) {
    add_xp(n);
    crate::util::xp_float(n, ev);
}

pub fn streak() -> i64 {
    read(|s| michi_core::docs::streak(&s.days, &today_key()))
}

/* ---------------- persistence ---------------- */

fn schedule() {
    if FLUSH_PENDING.with(|c| c.replace(true)) {
        return;
    }
    set_timeout(
        || {
            FLUSH_PENDING.with(|c| c.set(false));
            flush();
        },
        std::time::Duration::from_millis(1500),
    );
}

#[derive(Serialize)]
struct DocOut {
    name: String,
    data: Value,
}

#[derive(Serialize)]
struct CardOut {
    id: String,
    card: Option<Card>,
}

#[derive(Serialize)]
struct Items<T> {
    items: Vec<T>,
}

fn doc_value(s: &mut Store, name: &str, now: f64) -> Option<Value> {
    let v = match name {
        "profile" => {
            s.p.u = Some(now);
            serde_json::to_value(&s.p)
        }
        "journey" => {
            s.j.u = Some(now);
            serde_json::to_value(&s.j)
        }
        "kana" => {
            s.ks.u = Some(now);
            serde_json::to_value(&s.ks)
        }
        "mined" => {
            s.mined.u = Some(now);
            serde_json::to_value(&s.mined)
        }
        "desk" => {
            s.desk.u = Some(now);
            serde_json::to_value(&s.desk)
        }
        n if n.starts_with("day:") => {
            let d = s.days.get_mut(&n[4..])?;
            d.u = Some(now);
            serde_json::to_value(&*d)
        }
        n => return s.other.get(n).cloned(),
    };
    v.ok()
}

fn take_dirty() -> (Vec<DocOut>, Vec<CardOut>) {
    let now = now_ms();
    S.with(|s| {
        let mut s = s.borrow_mut();
        let names: Vec<String> = std::mem::take(&mut s.dirty).into_iter().collect();
        let mut docs = Vec::new();
        for n in names {
            if let Some(v) = doc_value(&mut s, &n, now) {
                docs.push(DocOut { name: n, data: v });
            }
        }
        let ids: Vec<String> = std::mem::take(&mut s.dirty_cards).into_iter().collect();
        let cards: Vec<CardOut> = ids.into_iter().map(|id| CardOut { card: s.cards.get(&id).copied(), id }).collect();
        (docs, cards)
    })
}

async fn save(docs: Vec<DocOut>, cards: Vec<CardOut>) {
    let r1 = if docs.is_empty() { Ok(()) } else { tauri::invoke::<(), _>("save_docs", &Items { items: docs }).await };
    let r2 =
        if cards.is_empty() { Ok(()) } else { tauri::invoke::<(), _>("save_cards", &Items { items: cards }).await };
    if let Err(e) = r1.and(r2) {
        if !ERROR_SHOWN.with(|c| c.replace(true)) {
            crate::app::toast(&format!("Não consegui salvar ({e}). Tente de novo em instantes."), "bad");
        }
    }
}

/// Writes every dirty document and card now.
pub fn flush() {
    let (docs, cards) = take_dirty();
    if docs.is_empty() && cards.is_empty() {
        return;
    }
    wasm_bindgen_futures::spawn_local(save(docs, cards));
}

/// Same as `flush`, finishing before it returns (before a sync).
pub async fn flush_now() {
    let (docs, cards) = take_dirty();
    if !docs.is_empty() || !cards.is_empty() {
        save(docs, cards).await;
    }
}

#[derive(Deserialize)]
struct LoadAll {
    docs: BTreeMap<String, Value>,
    days: BTreeMap<String, Value>,
    cards: Vec<(String, Card)>,
}

fn parse<T: serde::de::DeserializeOwned + Default>(v: Option<Value>) -> T {
    v.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// Loads everything from SQLite (start-up, after a restore or reset).
pub async fn load() -> Result<(), String> {
    let all: LoadAll = tauri::invoke("load_all", &tauri::NoArgs {}).await?;
    let mut docs = all.docs;
    let mut st = Store {
        p: parse(docs.remove("profile")),
        j: parse(docs.remove("journey")),
        ks: parse(docs.remove("kana")),
        mined: parse(docs.remove("mined")),
        desk: parse(docs.remove("desk")),
        other: docs,
        ..Store::default()
    };
    for (k, v) in all.days {
        let d: DayDoc = serde_json::from_value(v).unwrap_or_else(|_| DayDoc::new(&k));
        st.days.insert(k, d);
    }
    st.cards = all.cards.into_iter().collect();
    S.with(|s| *s.borrow_mut() = st);
    Ok(())
}

/// Replays backend operations on the in-memory documents, then saves them
/// back so both copies stay identical.
pub fn apply_ops(ops: &[Op]) {
    let mut names: Vec<String> = Vec::new();
    S.with(|s| {
        let mut s = s.borrow_mut();
        for op in ops {
            let k = op.day().to_string();
            op.apply_day(s.days.entry(k.clone()).or_insert_with(|| DayDoc::new(&k)));
            names.push(format!("day:{k}"));
            if op.touches_profile() {
                op.apply_profile(&mut s.p);
                names.push("profile".into());
            }
        }
    });
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    write(&refs, |_| ());
    refresh();
}

/// Re-reads one document from the backend (tray changed `desk`).
pub async fn reload_desk() {
    #[derive(Serialize)]
    struct A {
        name: &'static str,
    }
    if let Ok(Some(v)) = tauri::invoke::<Option<Value>, _>("doc_get", &A { name: "desk" }).await {
        if let Ok(d) = serde_json::from_value::<DeskSettings>(v) {
            S.with(|s| s.borrow_mut().desk = d);
            refresh();
        }
    }
}

/* ---------------- active-time tracker ---------------- */

thread_local! {
    static ACT: RefCell<(f64, &'static str, u32)> = const { RefCell::new((0.0, "geral", 0)) };
}

/// Last interaction (pointer/keyboard).
pub fn touch_activity() {
    ACT.with(|a| a.borrow_mut().0 = now_ms());
}

pub fn set_activity_cat(cat: &'static str) {
    ACT.with(|a| a.borrow_mut().1 = cat);
}

/// Every 15 s: counts seconds while the window is visible and the learner
/// touched it in the last 90 s; saves once a minute.
pub fn activity_tick() {
    let visible = crate::util::document().visibility_state() == web_sys::VisibilityState::Visible;
    let (last, cat, ticks) = ACT.with(|a| *a.borrow());
    if !visible || now_ms() - last > 90_000.0 {
        return;
    }
    let k = today_key();
    S.with(|s| {
        let mut s = s.borrow_mut();
        let d = s.days.entry(k.clone()).or_insert_with(|| DayDoc::new(&k));
        *d.sec.entry(cat.to_string()).or_insert(0.0) += 15.0;
    });
    ACT.with(|a| a.borrow_mut().2 = ticks + 1);
    if (ticks + 1) % 4 == 0 {
        let name = format!("day:{k}");
        write(&[&name], |_| ());
    }
}
