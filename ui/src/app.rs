//! App shell (port of `04-app.js` + `shell.html`): rail navigation, phone
//! tab bar, top bar chips, hash router, toasts and modals.

use crate::store::{self, sigs};
use crate::tauri;
use crate::util::{document, esc};
use crate::views;
use leptos::prelude::*;
use michi_core::docs::{level_of, xp_for_level};
use michi_core::ops::Op;
use michi_core::time::{greeting, jp_date, now_ms};
use serde::Deserialize;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

/// (route, label, Japanese)
pub const ROUTES: [(&str, &str, &str); 10] = [
    ("hoje", "Hoje", "今日"),
    ("agenda", "Agenda", "予定"),
    ("jornada", "Jornada", "路線"),
    ("revisar", "Revisar", "復習"),
    ("jogos", "Jogos", "遊"),
    ("frases", "Frases", "文"),
    ("imersao", "Imersão", "浸"),
    ("sensei", "Sensei", "先生"),
    ("anki", "Anki", "暗記"),
    ("ajustes", "Ajustes", "設定"),
];
/// The Sensei (AI) is off in the first version: the tab stays, marked "em breve",
/// and every button that would open it is hidden.
pub const SENSEI_ON: bool = false;

const MOBILE: [&str; 7] = ["hoje", "agenda", "jornada", "revisar", "jogos", "frases", "mais"];
const EXTRA: [&str; 2] = ["avisos", "mais"];

thread_local! {
    static ROUTE: Cell<Option<RwSignal<String>>> = const { Cell::new(None) };
    static NAV_REV: Cell<Option<RwSignal<u64>>> = const { Cell::new(None) };
    static INTENT: RefCell<Option<Value>> = const { RefCell::new(None) };
    static TOASTS: Cell<Option<RwSignal<Vec<(u32, String, String)>>>> = const { Cell::new(None) };
    static TOAST_ID: Cell<u32> = const { Cell::new(0) };
    static MODALS: RefCell<Vec<Modal>> = const { RefCell::new(Vec::new()) };
    static MODAL_REV: Cell<Option<RwSignal<u64>>> = const { Cell::new(None) };
    static MODAL_ID: Cell<u32> = const { Cell::new(0) };
}

fn known(r: &str) -> bool {
    ROUTES.iter().any(|x| x.0 == r) || EXTRA.contains(&r)
}

fn hash_route() -> String {
    let h = document().location().and_then(|l| l.hash().ok()).unwrap_or_default();
    let r = h.trim_start_matches('#').to_string();
    if known(&r) {
        r
    } else {
        "hoje".into()
    }
}

pub fn current_route() -> String {
    ROUTE.with(|c| c.get()).map(|s| s.get_untracked()).unwrap_or_else(|| "hoje".into())
}

/// `go(route, intent)`.
pub fn go(route: &str, intent: Option<Value>) {
    INTENT.with(|i| *i.borrow_mut() = intent);
    if current_route() == route {
        if let Some(n) = NAV_REV.with(|c| c.get()) {
            n.update(|v| *v += 1);
        }
    } else if let Some(l) = document().location() {
        let _ = l.set_hash(route);
    }
}

/// The intent passed by `go`, consumed once by the view.
pub fn take_intent() -> Option<Value> {
    INTENT.with(|i| i.borrow_mut().take())
}

/// Same intent as a click on the Today board for a routine block.
pub fn intent_for_block(block: Option<&str>) -> Option<Value> {
    match block? {
        "srs" => Some(json!({"start": true})),
        "wagotabi" => Some(json!({"log": "wagotabi"})),
        "vlog" => Some(json!({"log": "vlog"})),
        _ => None,
    }
}

/* ---------------- toasts ---------------- */

pub fn toast(msg: &str, kind: &str) {
    let Some(sig) = TOASTS.with(|c| c.get()) else {
        web_sys::console::log_1(&msg.into());
        return;
    };
    let id = TOAST_ID.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    sig.update(|v| v.push((id, msg.to_string(), kind.to_string())));
    set_timeout(move || sig.update(|v| v.retain(|t| t.0 != id)), std::time::Duration::from_millis(3200));
}

/* ---------------- modals ---------------- */

pub struct Modal {
    id: u32,
    title: String,
    wide: bool,
    body: Rc<dyn Fn() -> AnyView>,
}

thread_local! {
    /// Owners of recent modals. Dropping an owner disposes its signals, so
    /// they are kept here; only the oldest ones (long closed) are released.
    static DETACHED: RefCell<std::collections::VecDeque<Owner>> = RefCell::new(std::collections::VecDeque::new());
}

/// Runs `f` under a fresh root owner, so signals created for a modal or a
/// game are not disposed when the screen behind it redraws.
pub fn detached<R>(f: impl FnOnce() -> R) -> R {
    let owner = Owner::new_root(None);
    let r = owner.with(f);
    DETACHED.with(|d| {
        let mut d = d.borrow_mut();
        d.push_back(owner);
        while d.len() > 64 {
            d.pop_front();
        }
    });
    r
}

/// Opens a modal; `title` is HTML. Returns its id for `close_modal`.
pub fn open_modal(title: &str, wide: bool, body: impl Fn() -> AnyView + 'static) -> u32 {
    let id = MODAL_ID.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    MODALS.with(|m| m.borrow_mut().push(Modal { id, title: title.to_string(), wide, body: Rc::new(body) }));
    bump_modals();
    id
}

pub fn close_modal(id: u32) {
    MODALS.with(|m| m.borrow_mut().retain(|x| x.id != id));
    bump_modals();
}

pub fn close_all_modals() {
    MODALS.with(|m| m.borrow_mut().clear());
    bump_modals();
}

/// Redraws open modals (after their content changed).
pub fn bump_modals() {
    if let Some(s) = MODAL_REV.with(|c| c.get()) {
        s.update(|v| *v += 1);
    }
}

fn close_top_modal() -> bool {
    let top = MODALS.with(|m| m.borrow().last().map(|x| x.id));
    match top {
        Some(id) => {
            close_modal(id);
            true
        }
        None => false,
    }
}

#[component]
fn Modals() -> impl IntoView {
    let rev = RwSignal::new(0u64);
    MODAL_REV.with(|c| c.set(Some(rev)));
    move || {
        rev.get();
        let list: Vec<(u32, String, bool, Rc<dyn Fn() -> AnyView>)> =
            MODALS.with(|m| m.borrow().iter().map(|x| (x.id, x.title.clone(), x.wide, x.body.clone())).collect());
        list.into_iter()
            .map(|(id, title, wide, body)| {
                view! {
                    <div class="modal-back" on:click=move |ev: web_sys::MouseEvent| {
                        let t = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
                        let cur = ev.current_target().and_then(|t| t.dyn_into::<web_sys::Element>().ok());
                        if t.is_some() && t == cur { close_modal(id); }
                    }>
                        <div class="modal" class:wide=wide role="dialog" aria-modal="true">
                            <div class="modal-h">
                                <div class="grow"><h2 inner_html=title></h2></div>
                                <button class="icon-btn" aria-label="Fechar" on:click=move |_| close_modal(id)>"✕"</button>
                            </div>
                            <div class="modal-b">{body()}</div>
                        </div>
                    </div>
                }
            })
            .collect_view()
    }
}

/* ---------------- top bar ---------------- */

#[component]
fn Chips() -> impl IntoView {
    let s = sigs();
    move || {
        s.chips.get();
        let (xp, stamps) = store::read(|st| (st.p.xp, st.j.stamps.len()));
        let lv = level_of(xp);
        let (a, b) = (xp_for_level(lv), xp_for_level(lv + 1));
        let w = (((xp - a) / (b - a)) * 100.0).round().clamp(0.0, 100.0);
        let unread = s.unread.get();
        view! {
            <span class="chip streak" title="Dias seguidos com estudo">"🔥 "<b class="num">{store::streak()}</b>" dias"</span>
            <span class="chip" title=format!("{xp} XP no total")>
                <span class="tiny faint">"Nv."</span><b class="num">{lv}</b>
                <span class="bar" style="width:56px"><i style=format!("width:{w}%")></i></span>
            </span>
            <span class="chip" title="Selos de estação conquistados">"🎴 "<b class="num">{stamps}</b></span>
            <a class="chip bell" href="#avisos" title="Central de avisos" aria-label="Central de avisos">
                "🔔"{(unread > 0).then(|| view! { <b class="num bell-n">{unread}</b> })}
            </a>
        }
    }
}

#[component]
fn Topbar() -> impl IntoView {
    let s = sigs();
    view! {
        <div class="topbar">
            <div>
                <div class="eyebrow">{move || { s.chips.get(); format!("{}、{}", greeting(now_ms()), store::read(|st| st.p.name.clone())) }}</div>
                <div class="date num">{move || { s.chips.get(); jp_date(now_ms()) }}</div>
            </div>
            <div class="chips"><Chips/></div>
        </div>
    }
}

/* ---------------- shell ---------------- */

pub fn refresh_unread() {
    spawn_local(async {
        if let Ok(n) = tauri::invoke::<i64, _>("avisos_unread", &tauri::NoArgs {}).await {
            sigs().unread.set(n);
        }
    });
}

#[derive(Deserialize)]
struct Navigate {
    route: String,
    block: Option<String>,
}

fn install_global_handlers() {
    // [data-say] buttons inside HTML strings, and external links.
    let click = Closure::<dyn FnMut(web_sys::MouseEvent)>::new(|ev: web_sys::MouseEvent| {
        let Some(t) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if let Ok(Some(b)) = t.closest("[data-say]") {
            ev.prevent_default();
            crate::tts::say(&b.get_attribute("data-say").unwrap_or_default(), None);
            return;
        }
        if let Ok(Some(a)) = t.closest("a[href]") {
            let href = a.get_attribute("href").unwrap_or_default();
            if href.starts_with("http://") || href.starts_with("https://") {
                ev.prevent_default();
                #[derive(serde::Serialize)]
                struct U {
                    url: String,
                }
                tauri::call("open_external", U { url: href });
            }
        }
    });
    let doc = document();
    let _ = doc.add_event_listener_with_callback("click", click.as_ref().unchecked_ref());
    click.forget();

    let act = Closure::<dyn FnMut()>::new(store::touch_activity);
    for ev in ["pointerdown", "keydown"] {
        let _ = doc.add_event_listener_with_callback(ev, act.as_ref().unchecked_ref());
    }
    act.forget();

    let vis = Closure::<dyn FnMut()>::new(|| {
        if document().visibility_state() == web_sys::VisibilityState::Hidden {
            store::flush();
        }
    });
    let _ = doc.add_event_listener_with_callback("visibilitychange", vis.as_ref().unchecked_ref());
    vis.forget();

    let _ = window_event_listener(leptos::ev::keydown, |e| {
        if e.key() == "Escape" {
            close_top_modal();
        }
    });
    let _ = set_interval_with_handle(store::activity_tick, std::time::Duration::from_secs(15));
}

fn route_view(r: &str) -> AnyView {
    match r {
        "hoje" => view! { <views::hoje::Hoje/> }.into_any(),
        "agenda" => view! { <views::agenda::Agenda/> }.into_any(),
        "ajustes" => view! { <views::ajustes::Ajustes/> }.into_any(),
        "jornada" => view! { <views::jornada::Jornada/> }.into_any(),
        "revisar" => view! { <views::revisar::Revisar/> }.into_any(),
        "jogos" => view! { <views::jogos::Jogos/> }.into_any(),
        "frases" => view! { <views::frases::Frases/> }.into_any(),
        "imersao" => view! { <views::imersao::Imersao/> }.into_any(),
        "anki" => view! { <views::anki::Anki/> }.into_any(),
        "avisos" => view! { <views::avisos::Avisos/> }.into_any(),
        "mais" => view! { <views::mais::Mais/> }.into_any(),
        other => view! { <views::soon::Soon route=other.to_string()/> }.into_any(),
    }
}

#[component]
pub fn App() -> impl IntoView {
    let s = store::init_sigs();
    let route = RwSignal::new(hash_route());
    let nav_rev = RwSignal::new(0u64);
    let toasts = RwSignal::new(Vec::<(u32, String, String)>::new());
    ROUTE.with(|c| c.set(Some(route)));
    NAV_REV.with(|c| c.set(Some(nav_rev)));
    TOASTS.with(|c| c.set(Some(toasts)));

    let _ = window_event_listener(leptos::ev::hashchange, move |_| route.set(hash_route()));
    install_global_handlers();
    crate::tts::init();

    if tauri::available() {
        tauri::listen::<Vec<Op>>("michi://patch", |ops| store::apply_ops(&ops));
        tauri::listen::<Navigate>("michi://navigate", |n| go(&n.route, intent_for_block(n.block.as_deref())));
        tauri::listen::<Value>("michi://avisos", |_| refresh_unread());
        tauri::listen::<Value>("michi://reload-desk", |_| spawn_local(store::reload_desk()));
    }

    spawn_local(async move {
        if let Err(e) = store::load().await {
            toast(&format!("Não consegui abrir seus dados: {e}"), "bad");
        }
        crate::tts::pick();
        s.loaded.set(true);
        store::refresh();
        refresh_unread();
        let _ = crate::data::need_content().await;
        let _ = crate::data::need_kanji().await;
    });

    let nav_links = ROUTES
        .iter()
        .map(|(id, pt, jp)| {
            let id = *id;
            view! {
                <a href=format!("#{id}") aria-current=move || if route.get() == id { "page" } else { "false" }>
                    <span class="dot"></span>{*pt}<span class="jpl">{*jp}</span>
                </a>
            }
        })
        .collect_view();
    let tab_links = MOBILE
        .iter()
        .map(|id| {
            let id = *id;
            let (pt, jp) = ROUTES.iter().find(|x| x.0 == id).map(|x| (x.1, x.2)).unwrap_or(("Mais", "他"));
            view! {
                <a href=format!("#{id}") aria-current=move || {
                    let r = route.get();
                    let more = matches!(r.as_str(), "sensei" | "anki" | "ajustes" | "avisos" | "imersao");
                    if r == id || (id == "mais" && more) { "page" } else { "false" }
                }>
                    <span class="tj">{jp}</span>{pt}
                </a>
            }
        })
        .collect_view();

    view! {
        <div class="app">
            <aside class="rail" aria-label="Navegação">
                <div class="brand">
                    <div class="brand-mark">"道"</div>
                    <div><div class="brand-name">"Nihongo Michi"</div><div class="brand-sub">"日本語の道"</div></div>
                </div>
                <nav class="nav">{nav_links}</nav>
                <div class="rail-foot">
                    <div class="sync cloud"><i></i>"Salvo neste PC"</div>
                    <div class="tiny faint">"KANJIDIC2 · KanjiVG · tanos (CC)"</div>
                </div>
            </aside>
            <main class="main" id="main" tabindex="-1">
                <div class="wrap">
                    <Topbar/>
                    <div class="stack-lg" id="view">
                        {move || {
                            let r = route.get();
                            nav_rev.get();
                            crate::tts::stop();
                            store::set_activity_cat(michi_core::routine::cat_of(&r));
                            crate::util::scroll_top();
                            if !s.loaded.get() {
                                view! { <div class="loading"><div class="spin"></div>"Carregando o seu caminho…"</div> }.into_any()
                            } else {
                                route_view(&r)
                            }
                        }}
                    </div>
                </div>
            </main>
        </div>
        <nav class="tabbar" aria-label="Navegação">{tab_links}</nav>
        <div class="toasts" aria-live="polite">
            {move || toasts.get().into_iter().map(|(_, m, k)| view! { <div class=format!("toast {k}") role="status">{m}</div> }).collect_view()}
        </div>
        <Modals/>
    }
}

/// HTML of a stat tile (`statTile`).
pub fn stat_tile(v: &str, l: &str) -> String {
    format!(r#"<div class="stat"><div class="v num">{v}</div><div class="l">{}</div></div>"#, esc(l))
}

/// HTML of a progress bar (`bar(frac, lc)`).
pub fn bar(frac: f64, lc: &str) -> String {
    format!(
        r#"<div class="bar" {}><i style="width:{}"></i></div>"#,
        if lc.is_empty() { String::new() } else { format!(r#"style="--lc:{lc}""#) },
        crate::util::pct(frac)
    )
}
