//! Revisar (port of `12-review.js`): decks, the Anki-style review session,
//! the kanji browser and the mined cards.

use crate::app::{bar, go, take_intent, toast};
use crate::data::{self, Sentence};
use crate::jp::{sent_html, sent_plain, spk};
use crate::logic::{due_counts, queue_for, Deck, Queue};
use crate::store::{self, sigs};
use crate::tts::say;
use crate::util::{esc, ev_value, pick};
use crate::views::kanji::{open_kanji, KanjiBrowser, MinedList};
use leptos::prelude::*;
use michi_core::srs::{preview_label, schedule};
use michi_core::time::{day_start, now_ms, plural, DAY_MS};
use michi_core::Card;
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone)]
enum Mode {
    Tabs(String),
    Session { decks: Option<Vec<Deck>>, custom: Option<Vec<String>>, title: Option<String> },
}

#[component]
pub fn Revisar() -> impl IntoView {
    let s = sigs();
    let intent = take_intent();
    let mode = RwSignal::new(start_mode(intent.as_ref()));
    let ready = RwSignal::new(false);
    spawn_local(async move {
        let _ = data::need_content().await;
        let _ = data::need_kanji().await;
        let wants_vocab = store::read(|s| s.p.new_vocab > 0.0 || s.cards.keys().any(|k| k.starts_with('v')));
        if wants_vocab {
            let _ = data::need_vocab().await;
        }
        ready.set(true);
    });
    move || {
        if !ready.get() {
            return view! { <div class="loading"><div class="spin"></div>"Carregando baralhos…"</div> }.into_any();
        }
        match mode.get() {
            Mode::Tabs(tab) => {
                s.view.get();
                tabs_view(mode, &tab)
            }
            Mode::Session { decks, custom, title } => {
                view! { <Session decks=decks custom=custom title=title mode=mode/> }.into_any()
            }
        }
    }
}

fn start_mode(intent: Option<&Value>) -> Mode {
    let Some(i) = intent else { return Mode::Tabs("decks".into()) };
    if let Some(c) = i.get("custom").and_then(Value::as_array) {
        let ids = c.iter().filter_map(|x| x.as_str().map(str::to_string)).collect();
        return Mode::Session {
            decks: None,
            custom: Some(ids),
            title: i.get("title").and_then(Value::as_str).map(str::to_string),
        };
    }
    if i.get("start").is_some() {
        return Mode::Session { decks: Some(Deck::ALL.to_vec()), custom: None, title: None };
    }
    Mode::Tabs(i.get("tab").and_then(Value::as_str).unwrap_or("decks").to_string())
}

fn deck_blurb(d: Deck) -> String {
    match d {
        Deck::Kanji => {
            let k = data::kanji().unwrap_or_default();
            let st = k.iter().filter(|x| store::card(&format!("k:{}", x.k)).is_some()).count();
            format!("{st}/{} iniciados (N5→N1)", k.len())
        }
        Deck::Vocab => match data::vocab() {
            Some(v) => format!("{} palavras N5–N3", v.len()),
            None => "N5–N3 (tanos)".into(),
        },
        Deck::Grammar => format!("{} pontos estudados", Deck::Grammar.ids().len()),
        Deck::Mined => format!("{} cartas suas", store::read(|s| s.mined.list.len())),
    }
}

fn tabs_view(mode: RwSignal<Mode>, tab: &str) -> AnyView {
    let tabs = [("decks", "Baralhos"), ("kanji", "Kanji 2k"), ("mined", "Mineração")]
        .into_iter()
        .map(|(t, l)| {
            let sel = t == tab;
            view! { <button role="tab" aria-selected=sel.to_string() on:click=move |_| mode.set(Mode::Tabs(t.to_string()))>{l}</button> }
        })
        .collect_view();
    let body = match tab {
        "kanji" => view! { <KanjiBrowser/> }.into_any(),
        "mined" => view! { <MinedList/> }.into_any(),
        _ => decks_view(mode),
    };
    view! {
        <div class="row-between"><h1>"Revisar"</h1>
            <button class="btn primary" on:click=move |_| {
                spawn_local(async move {
                    let _ = data::need_vocab().await;
                    mode.set(Mode::Session { decks: Some(Deck::ALL.to_vec()), custom: None, title: None });
                });
            }>"Revisar tudo"</button>
        </div>
        <div class="tabs" role="tablist">{tabs}</div>
        <div>{body}</div>
    }
    .into_any()
}

fn decks_view(mode: RwSignal<Mode>) -> AnyView {
    let cards = Deck::ALL
        .into_iter()
        .map(|d| {
            let c = due_counts(d);
            let lim = d.limit();
            let counts = match c {
                Some(c) => view! {
                    <div class="row small">
                        <span class="pill bad" title="aprendendo">{c.learn}</span>
                        <span class="pill line" title="revisões">{c.rev}</span>
                        <span class="pill ok" title="novos">{c.fresh}</span>
                    </div>
                }
                .into_any(),
                None => view! { <span class="pill">"carregar"</span> }.into_any(),
            };
            let need_load = d == Deck::Vocab && data::vocab().is_none();
            view! {
                <div class="card stack" style=format!("border-left:5px solid {}", d.lc())>
                    <div class="row-between"><div><span class="jp muted">{d.jp()}</span><h2>{d.name()}</h2></div>{counts}</div>
                    <div class="small muted">{format!("{} · {} novos/dia", deck_blurb(d), lim)}</div>
                    <div class="row">
                        <button class="btn sm primary" on:click=move |_| {
                            spawn_local(async move {
                                if d == Deck::Vocab { let _ = data::need_vocab().await; }
                                mode.set(Mode::Session { decks: Some(vec![d]), custom: None, title: None });
                            });
                        }>"Estudar"</button>
                        {need_load.then(|| view! {
                            <button class="btn sm" on:click=|_| spawn_local(async { let _ = data::need_vocab().await; store::refresh(); })>"Carregar vocabulário"</button>
                        })}
                        <label class="small row" style="gap:6px">"Novos/dia "
                            <input type="number" min="0" max="50" value=lim.to_string() style="width:70px;min-height:32px" on:change=move |ev| {
                                let v = ev_value(&ev).parse::<f64>().unwrap_or(0.0).floor().clamp(0.0, 50.0);
                                store::write(&["profile"], |s| match d {
                                    Deck::Kanji => s.p.new_kanji = v,
                                    Deck::Vocab => s.p.new_vocab = v,
                                    Deck::Grammar => s.p.new_grammar = v,
                                    Deck::Mined => s.p.new_mined = v,
                                });
                                toast("Limite salvo.", "");
                            }/>
                        </label>
                    </div>
                </div>
            }
        })
        .collect_view();
    view! {
        <div class="grid2">{cards}</div>
        <p class="small muted">"Vermelho = aprendendo · azul-verde = revisões de hoje · verde = novos liberados. O vocabulário começa pausado (0/dia) porque o seu Kaishi 1.5k no Anki já cobre isso — ative se quiser estudar aqui também."</p>
    }
    .into_any()
}

/* ---------------- card faces ---------------- */

fn vocab_of(key: &str) -> Option<data::Vocab> {
    data::vocab()?.iter().find(|v| format!("{}|{}", v.0, v.1) == key).cloned()
}

fn card_front(id: &str, sentence: &Option<Sentence>) -> String {
    let (t, key) = (id.chars().next().unwrap_or(' '), &id[2.min(id.len())..]);
    match t {
        'k' => format!(r#"<div class="front">{}</div><div class="small muted">Significado e leitura?</div>"#, esc(key)),
        'v' => {
            let w = vocab_of(key).map(|v| v.0).unwrap_or_else(|| key.split('|').next().unwrap_or("").to_string());
            format!(r#"<div class="front word">{}</div><div class="small muted">Leitura e significado?</div>"#, esc(&w))
        }
        'm' => match store::read(|s| s.mined.list.iter().find(|x| x.id == key).cloned()) {
            Some(m) => format!(
                r#"<div class="front word">{}</div>{}<div class="small muted">Do seu {}</div>"#,
                esc(&m.front),
                if m.sentence.is_empty() {
                    String::new()
                } else {
                    format!(r#"<div class="sent" style="font-size:18px">{}</div>"#, esc(&m.sentence))
                },
                esc(if m.source.is_empty() { "garimpo" } else { &m.source })
            ),
            None => "<div>?</div>".into(),
        },
        'g' => {
            let c = data::content();
            match (c.as_ref().and_then(|c| c.point(key)), sentence) {
                (Some(p), Some(s)) => format!(
                    r#"<div class="eyebrow">{}</div><div class="front sent">{}</div><div class="small muted">Entenda a frase. Qual é o papel de cada pedaço?</div>"#,
                    esc(&p.title),
                    sent_html(s, false, false, "")
                ),
                _ => "<div>?</div>".into(),
            }
        }
        _ => String::new(),
    }
}

fn card_back(id: &str, sentence: &Option<Sentence>) -> String {
    let (t, key) = (id.chars().next().unwrap_or(' '), &id[2.min(id.len())..]);
    match t {
        'k' => {
            let Some(k) = crate::views::kanji::kanji_of(key) else { return String::new() };
            let pt = if store::read(|s| s.p.show_pt) && !k.pt.is_empty() {
                format!(r#"<div class="muted">{}</div>"#, esc(&k.pt.join(", ")))
            } else {
                String::new()
            };
            let ex: String = k
                .ex
                .iter()
                .map(|e| {
                    format!(
                        r#"<div class="row" style="justify-content:center;gap:8px"><span class="jp" style="font-size:20px">{}</span><span class="jp muted">{}</span><span class="small">{}</span>{}</div>"#,
                        esc(&e.0),
                        esc(&e.1),
                        esc(&e.2),
                        spk(&e.1, "btn xs")
                    )
                })
                .collect();
            format!(
                r#"<div class="stack" style="width:100%;max-width:560px"><div style="font-size:22px;font-weight:700">{}</div>{}<dl class="readings" style="justify-content:center"><dt>ON</dt><dd>{}</dd><dt>KUN</dt><dd>{}</dd></dl><div class="stack">{}</div><div class="row" style="justify-content:center"><button class="btn sm" data-kopen="{}">✍️ Traços e detalhes</button></div></div>"#,
                esc(&k.en.join(", ")),
                pt,
                esc(&if k.on.is_empty() { "—".into() } else { k.on.join("、") }),
                esc(&if k.kun.is_empty() { "—".into() } else { k.kun.join("、") }),
                ex,
                esc(&k.k)
            )
        }
        'v' => {
            let (r, m) = match vocab_of(key) {
                Some(v) => (v.1, v.2),
                None => (key.split('|').nth(1).unwrap_or("").to_string(), String::new()),
            };
            format!(
                r#"<div class="stack"><div class="jp" style="font-size:24px">{}</div><div style="font-size:19px;font-weight:700">{}</div><div>{}</div></div>"#,
                esc(&r),
                esc(&m),
                spk(&r, "btn sm")
            )
        }
        'm' => {
            let m = store::read(|s| s.mined.list.iter().find(|x| x.id == key).cloned()).unwrap_or_default();
            format!(
                r#"<div class="stack"><div class="jp" style="font-size:22px">{}</div><div style="font-size:19px;font-weight:700">{}</div>{}<div>{}</div></div>"#,
                esc(&m.reading),
                esc(&m.meaning),
                if m.note.is_empty() {
                    String::new()
                } else {
                    format!(r#"<div class="small muted">{}</div>"#, esc(&m.note))
                },
                spk(if m.reading.is_empty() { &m.front } else { &m.reading }, "btn sm")
            )
        }
        'g' => {
            let c = data::content();
            match (c.as_ref().and_then(|c| c.point(key)), sentence) {
                (Some(p), Some(s)) => format!(
                    r#"<div class="stack" style="max-width:640px"><div class="sent">{}</div><div class="en">{}</div>{}<div class="small"><b>{}</b> — {}</div></div>"#,
                    sent_html(s, true, true, ""),
                    esc(&s.en),
                    spk(&sent_plain(s), "btn sm"),
                    esc(&p.pattern),
                    esc(&p.gist)
                ),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

fn speak_card(id: &str, sentence: &Option<Sentence>) {
    let (t, key) = (id.chars().next().unwrap_or(' '), &id[2.min(id.len())..]);
    match t {
        'k' => {
            if let Some(e) = crate::views::kanji::kanji_of(key).and_then(|k| k.ex.first().cloned()) {
                say(&e.1, None);
            }
        }
        'v' => {
            if let Some(v) = vocab_of(key) {
                say(&v.1, None);
            }
        }
        'g' => {
            if let Some(s) = sentence {
                say(&sent_plain(s), None);
            }
        }
        'm' => {
            if let Some(m) = store::read(|s| s.mined.list.iter().find(|x| x.id == key).cloned()) {
                say(if m.reading.is_empty() { &m.front } else { &m.reading }, None);
            }
        }
        _ => {}
    }
}

/* ---------------- session ---------------- */

#[derive(Clone)]
struct Undo {
    id: String,
    before: Option<Card>,
    lists: (Vec<String>, Vec<String>, Vec<String>),
    done: usize,
    since_new: usize,
}

#[derive(Clone)]
struct S {
    q: (Vec<String>, Vec<String>, Vec<String>),
    done: usize,
    since_new: usize,
    cur: Option<String>,
    shown: bool,
    undo: Option<Undo>,
    sentence: Option<Sentence>,
    finished: bool,
}

fn refill(decks: &Option<Vec<Deck>>, custom: &Option<Vec<String>>) -> (Vec<String>, Vec<String>, Vec<String>) {
    if let Some(ids) = custom {
        let now = now_ms();
        let end_t = day_start(now) + DAY_MS;
        let ready: Vec<String> = ids
            .iter()
            .filter(|id| match store::card(id) {
                None => true,
                Some(c) => c.state() != 2 || c.due() < end_t,
            })
            .cloned()
            .collect();
        let mut learn: Vec<String> =
            ready.iter().filter(|id| store::card(id).map(|c| c.state() != 2).unwrap_or(false)).cloned().collect();
        learn.sort_by(|a, b| due(a).partial_cmp(&due(b)).unwrap_or(std::cmp::Ordering::Equal));
        let rev = ready.iter().filter(|id| store::card(id).map(|c| c.state() == 2).unwrap_or(false)).cloned().collect();
        let fresh = ready.iter().filter(|id| store::card(id).is_none()).cloned().collect();
        (learn, rev, fresh)
    } else {
        let Queue { learn, rev, fresh } = queue_for(decks.as_deref().unwrap_or(&Deck::ALL));
        (learn, rev, fresh)
    }
}

fn due(id: &str) -> f64 {
    store::card(id).map(|c| c.due()).unwrap_or(0.0)
}

/// Next card: learning due now, then reviews (a new card every 4), then new.
fn next_card(s: &S) -> Option<String> {
    let now = now_ms();
    if let Some(id) = s.q.0.iter().find(|id| due(id) <= now) {
        return Some(id.clone());
    }
    if !s.q.1.is_empty() && (s.q.2.is_empty() || s.since_new < 4) {
        return Some(s.q.1[0].clone());
    }
    if let Some(id) = s.q.2.first() {
        return Some(id.clone());
    }
    if let Some(id) = s.q.1.first() {
        return Some(id.clone());
    }
    s.q.0.iter().find(|id| due(id) <= now + 20.0 * 60_000.0).cloned()
}

fn grammar_sentence(id: &str) -> Option<Sentence> {
    if !id.starts_with("g:") {
        return None;
    }
    let c = data::content()?;
    let p = c.point(&id[2..])?;
    pick(&p.s)
}

#[component]
fn Session(
    decks: Option<Vec<Deck>>,
    custom: Option<Vec<String>>,
    title: Option<String>,
    mode: RwSignal<Mode>,
) -> impl IntoView {
    let q = refill(&decks, &custom);
    let start_total = q.0.len() + q.1.len() + q.2.len();
    let t0 = now_ms();
    let mut first =
        S { q, done: 0, since_new: 0, cur: None, shown: false, undo: None, sentence: None, finished: false };
    first.cur = next_card(&first);
    first.sentence = first.cur.as_deref().and_then(grammar_sentence);
    first.finished = first.cur.is_none();
    let st = RwSignal::new(first);

    let advance = move || {
        st.update(|s| {
            s.cur = next_card(s);
            s.shown = false;
            s.sentence = s.cur.as_deref().and_then(grammar_sentence);
            s.finished = s.cur.is_none();
        });
    };
    let flip = move || {
        let s = st.get_untracked();
        let Some(cur) = s.cur.clone() else { return };
        if s.shown {
            return;
        }
        st.update(|s| s.shown = true);
        if !cur.starts_with('g') {
            speak_card(&cur, &s.sentence);
        }
    };
    let grade = move |g: u8, ev: Option<web_sys::MouseEvent>| {
        let s = st.get_untracked();
        let Some(cur) = s.cur.clone() else { return };
        if !s.shown {
            return;
        }
        let before = store::card(&cur);
        let was_new = before.is_none();
        let nc = schedule(before.as_ref(), g, now_ms(), crate::util::rand());
        store::set_card(&cur, Some(nc));
        st.update(|s| {
            s.undo = Some(Undo { id: cur.clone(), before, lists: s.q.clone(), done: s.done, since_new: s.since_new });
            s.q.0.retain(|x| *x != cur);
            s.q.1.retain(|x| *x != cur);
            s.q.2.retain(|x| *x != cur);
            if nc.state() != 2 {
                s.q.0.push(cur.clone());
            }
            if was_new {
                s.since_new = 0;
            } else {
                s.since_new += 1;
            }
            s.done += 1;
        });
        let deck = Deck::of_id(&cur).map(|d| d.id()).unwrap_or("kanji");
        store::with_today(|d| {
            d.rev += 1.0;
            if was_new {
                *d.nw.entry(deck.to_string()).or_insert(0.0) += 1.0;
            }
        });
        store::add_xp_at(if was_new { 3.0 } else { 1.0 }, ev.as_ref());
        advance();
    };
    let quit = move || {
        store::flush();
        store::refresh();
        mode.set(Mode::Tabs("decks".into()));
    };
    let key = window_event_listener(leptos::ev::keydown, move |e| {
        let tag = e
            .target()
            .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok())
            .map(|t| t.tag_name())
            .unwrap_or_default();
        if matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT") {
            return;
        }
        match e.key().as_str() {
            " " | "Enter" => {
                e.prevent_default();
                if !st.get_untracked().shown {
                    flip()
                } else {
                    grade(3, None)
                }
            }
            "1" | "2" | "3" | "4" => grade(e.key().parse().unwrap_or(3), None),
            "r" | "R" => {
                let s = st.get_untracked();
                if let Some(c) = &s.cur {
                    speak_card(c, &s.sentence);
                }
            }
            _ => {}
        }
    });
    on_cleanup(move || key.remove());
    let title2 = title.clone();

    move || {
        let s = st.get();
        if s.finished {
            let mins = ((now_ms() - t0) / 60_000.0).round().max(1.0);
            let left = s.q.0.len();
            return view! {
                <div class="card stack" style="text-align:center;align-items:center;padding:30px">
                    <div style="font-size:48px">"🎉"</div><h1>"お疲れさま！"</h1>
                    <p class="muted">{format!("{} em ~{} min. {}", plural(s.done as i64, "carta", "cartas"), mins,
                        if left > 0 { format!("Ainda há {left} em aprendizado para daqui a pouco.") } else { "Tudo em dia.".into() })}</p>
                    <div class="row" style="justify-content:center">
                        <button class="btn" on:click=move |_| quit()>"Voltar"</button>
                        <button class="btn primary" on:click=|_| { store::flush(); go("hoje", None); }>"Ir para Hoje"</button>
                    </div>
                </div>
            }
            .into_any();
        }
        let cur = s.cur.clone().unwrap_or_default();
        let remaining = s.q.0.len() + s.q.1.len() + s.q.2.len();
        let frac = if start_total > 0 { s.done as f64 / (s.done + remaining) as f64 } else { 1.0 };
        let front = card_front(&cur, &s.sentence);
        let back = if s.shown {
            format!(r#"<hr class="hr" style="width:100%">{}"#, card_back(&cur, &s.sentence))
        } else {
            String::new()
        };
        let c = store::card(&cur);
        let is_new = c.is_none();
        let actions = if s.shown {
            let now = now_ms();
            let grades = [(1u8, "Errei", "g1"), (2, "Difícil", "g2"), (3, "Bom", "g3"), (4, "Fácil", "g4")]
                .into_iter()
                .map(|(g, l, cl)| {
                    view! { <button class=format!("btn {cl}") on:click=move |ev| grade(g, Some(ev))>{l}<small>{format!("{} · {}", preview_label(c.as_ref(), g, now), g)}</small></button> }
                })
                .collect_view();
            view! { <div class="grades">{grades}</div> }.into_any()
        } else {
            view! { <button class="btn primary block" style="min-height:52px" on:click=move |_| flip()>"Mostrar resposta "<span class="kbd">"espaço"</span></button> }.into_any()
        };
        let has_undo = s.undo.is_some();
        let cur2 = cur.clone();
        let sent2 = s.sentence.clone();
        view! {
            <div class="row-between">
                <button class="btn ghost sm" on:click=move |_| quit()>"← Sair"</button>
                <div class="row small">
                    <span class="pill bad">{s.q.0.len()}</span><span class="pill line">{s.q.1.len()}</span><span class="pill ok">{s.q.2.len()}</span>
                    {has_undo.then(|| view! {
                        <button class="btn xs" on:click=move |_| {
                            let Some(u) = st.get_untracked().undo else { return };
                            store::set_card(&u.id, u.before);
                            st.update(|s| { s.q = u.lists.clone(); s.done = u.done; s.since_new = u.since_new; s.undo = None; });
                            advance();
                        }>"↶ desfazer"</button>
                    })}
                </div>
            </div>
            {title2.clone().map(|t| view! { <div class="eyebrow">{t}</div> })}
            <div inner_html=bar(frac, "")></div>
            <div class="card flash" on:click=move |ev: web_sys::MouseEvent| {
                let t = ev.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok());
                if let Some(k) = t.as_ref().and_then(|t| t.closest("[data-kopen]").ok().flatten()) {
                    open_kanji(&k.get_attribute("data-kopen").unwrap_or_default());
                    return;
                }
                let on_button = t.as_ref().and_then(|t| t.closest("button").ok().flatten()).is_some();
                if !on_button { flip(); }
            }>
                <div inner_html=front></div>
                <div inner_html=back style="width:100%;display:flex;flex-direction:column;align-items:center"></div>
            </div>
            <div>{actions}</div>
            <div class="row" style="justify-content:center">
                <button class="btn sm ghost" on:click=move |_| speak_card(&cur2, &sent2)>"🔊 Ouvir "<span class="kbd">"R"</span></button>
                <span class="tiny faint">{if is_new { "carta nova" } else { "" }}</span>
            </div>
        }
        .into_any()
    }
}
