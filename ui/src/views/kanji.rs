//! Kanji (port of `13-kanji.js`): KanjiVG stroke-order viewer and the
//! drawing quiz, the kanji detail modal, the kanji browser and mining.

use crate::app::{close_modal, detached, go, open_modal, toast, SENSEI_ON};
use crate::data::{self, Kanji};
use crate::docs::MinedItem;
use crate::jp::{spk, to_hira};
use crate::store;
use crate::util::{clamp, esc, ev_value, reflow, sleep, uid};
use leptos::html::Div;
use leptos::prelude::*;
use michi_core::srs::mark_known;
use michi_core::time::{fmt_ivl, now_ms};
use michi_core::Card;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::{Element, SvgGeometryElement, SvgGraphicsElement, SvgsvgElement};

const SVGNS: &str = "http://www.w3.org/2000/svg";
const GUIDE: &str = r#"<rect x="1" y="1" width="107" height="107" rx="4" class="guide" style="stroke-dasharray:none"/><line x1="54.5" y1="2" x2="54.5" y2="107" class="guide"/><line x1="2" y1="54.5" x2="107" y2="54.5" class="guide"/>"#;

/* ---------------- stroke drawing helpers ---------------- */

fn stroke_svg(paths: &[String], ghost: bool) -> String {
    let mut s = format!(r#"<svg viewBox="0 0 109 109" xmlns="{SVGNS}">{GUIDE}"#);
    if ghost {
        for d in paths {
            s.push_str(&format!(r#"<path class="ghost" d="{}"/>"#, esc(d)));
        }
    }
    s.push_str(r#"<g class="inkg">"#);
    for (i, d) in paths.iter().enumerate() {
        s.push_str(&format!(r#"<path class="ink" data-i="{i}" d="{}" style="opacity:0"/>"#, esc(d)));
    }
    s.push_str(r#"</g><g class="numg"></g><g class="userg"></g></svg>"#);
    s
}

fn inks(root: &Element) -> Vec<SvgGeometryElement> {
    let list = root.query_selector_all(".ink").ok();
    let mut out = Vec::new();
    if let Some(l) = list {
        for i in 0..l.length() {
            if let Some(n) = l.item(i).and_then(|n| n.dyn_into::<SvgGeometryElement>().ok()) {
                out.push(n);
            }
        }
    }
    out
}

fn set_style(el: &SvgGeometryElement, k: &str, v: &str) {
    let _ = el.unchecked_ref::<web_sys::SvgElement>().style().set_property(k, v);
}

fn len_of(p: &SvgGeometryElement) -> f64 {
    p.get_total_length() as f64
}

fn point_at(p: &SvgGeometryElement, l: f64) -> (f64, f64) {
    p.get_point_at_length(l as f32).map(|q| (q.x() as f64, q.y() as f64)).unwrap_or((0.0, 0.0))
}

fn place_numbers(root: &Element) {
    let Ok(Some(g)) = root.query_selector(".numg") else { return };
    g.set_inner_html("");
    let doc = crate::util::document();
    for (i, p) in inks(root).iter().enumerate() {
        let a = point_at(p, 0.0);
        let b = point_at(p, len_of(p).min(4.0));
        let (dx, dy) = (a.0 - b.0, a.1 - b.1);
        let n = dx.hypot(dy).max(1e-9);
        let n = if n == 0.0 { 1.0 } else { n };
        if let Ok(t) = doc.create_element_ns(Some(SVGNS), "text") {
            let _ = t.set_attribute("class", "snum");
            let _ = t.set_attribute("x", &format!("{:.1}", a.0 + dx / n * 5.0 - 2.0));
            let _ = t.set_attribute("y", &format!("{:.1}", a.1 + dy / n * 5.0 + 2.5));
            t.set_text_content(Some(&(i + 1).to_string()));
            let _ = g.append_child(&t);
        }
    }
}

fn show_all(root: &Element) {
    for p in inks(root) {
        set_style(&p, "transition", "none");
        set_style(&p, "stroke-dasharray", "none");
        set_style(&p, "stroke-dashoffset", "0");
        set_style(&p, "opacity", "1");
    }
}

/// `animateStrokes`: draws each stroke in order; stops when `gen` changes.
async fn animate(root: Element, token: Rc<Cell<u32>>, gen: u32, speed: f64) {
    let ps = inks(&root);
    for p in &ps {
        let l = len_of(p);
        set_style(p, "transition", "none");
        set_style(p, "stroke-dasharray", &l.to_string());
        set_style(p, "stroke-dashoffset", &l.to_string());
        set_style(p, "opacity", "1");
    }
    for p in &ps {
        if token.get() != gen {
            return;
        }
        let l = len_of(p);
        let dur = (l * 9.0).max(220.0) / speed;
        let _ = p.class_list().add_1("cur");
        reflow(p);
        set_style(p, "transition", &format!("stroke-dashoffset {dur}ms linear"));
        set_style(p, "stroke-dashoffset", "0");
        sleep((dur + 120.0 / speed) as i32).await;
        let _ = p.class_list().remove_1("cur");
    }
}

pub async fn strokes_of(ch: &str) -> Option<Vec<String>> {
    let s = data::need_strokes().await.ok()?;
    s.get(ch).cloned()
}

/* ---------------- viewer ---------------- */

/// `strokeViewer`: animated stroke order, step by step, and the drawing quiz.
#[component]
pub fn StrokeViewer(ch: String) -> impl IntoView {
    let paths = RwSignal::new(None::<Option<Vec<String>>>);
    let quiz = RwSignal::new(false);
    let ch2 = ch.clone();
    spawn_local(async move { paths.set(Some(strokes_of(&ch2).await)) });
    move || {
        let ch = ch.clone();
        match paths.get() {
            None => view! { <div class="loading"><div class="spin"></div>"Traços…"</div> }.into_any(),
            Some(None) => view! { <div class="empty small">"Sem dados de traço para este caractere."</div> }.into_any(),
            Some(Some(p)) if quiz.get() => view! {
                <StrokeQuiz ch=ch paths=p blind=false on_out=Callback::new(move |_| quiz.set(false))/>
            }
            .into_any(),
            Some(Some(p)) => viewer(p, quiz),
        }
    }
}

fn viewer(paths: Vec<String>, quiz: RwSignal<bool>) -> AnyView {
    let node = NodeRef::<Div>::new();
    let token = Rc::new(Cell::new(0u32));
    let step = Rc::new(Cell::new(0usize));
    let n = paths.len();
    let svg = stroke_svg(&paths, true);
    let t0 = token.clone();
    let play = move || {
        let Some(el) = node.get_untracked() else { return };
        let el: Element = el.into();
        t0.set(t0.get() + 1);
        let gen = t0.get();
        spawn_local(animate(el, t0.clone(), gen, 1.0));
    };
    let play2 = play.clone();
    Effect::new(move |_| {
        if let Some(el) = node.get() {
            let el: Element = el.into();
            place_numbers(&el);
            show_all(&el);
            let p = play2.clone();
            set_timeout(move || p(), std::time::Duration::from_millis(250));
        }
    });
    let t1 = token.clone();
    let step_once = move || {
        let Some(el) = node.get_untracked() else { return };
        let el: Element = el.into();
        t1.set(t1.get() + 1);
        let ps = inks(&el);
        if ps.is_empty() {
            return;
        }
        if step.get() == 0 || step.get() >= ps.len() {
            for p in &ps {
                set_style(p, "opacity", "0");
            }
            step.set(0);
        }
        let p = &ps[step.get()];
        step.set(step.get() + 1);
        let l = len_of(p);
        set_style(p, "transition", "none");
        set_style(p, "stroke-dasharray", &l.to_string());
        set_style(p, "stroke-dashoffset", &l.to_string());
        set_style(p, "opacity", "1");
        reflow(p);
        set_style(p, "transition", "stroke-dashoffset 500ms linear");
        set_style(p, "stroke-dashoffset", "0");
    };
    let t2 = token.clone();
    view! {
        <div class="stack">
            <div class="strokebox" node_ref=node inner_html=svg></div>
            <div class="row" style="justify-content:center">
                <button class="btn sm" on:click=move |_| play()>"▶ Animar"</button>
                <button class="btn sm" on:click=move |_| step_once()>"Traço a traço"</button>
                <button class="btn sm" on:click=move |_| { t2.set(t2.get() + 1); quiz.set(true); }>"✍️ Escrever"</button>
                <span class="tiny faint">{format!("{n} traços")}</span>
            </div>
        </div>
    }
    .into_any()
}

/* ---------------- drawing quiz ---------------- */

struct QState {
    i: usize,
    miss: u32,
    first_try: usize,
    pts: Option<Vec<(f64, f64)>>,
    line: Option<Element>,
    finished: bool,
}

fn resample(arr: &[(f64, f64)], n: usize) -> Vec<(f64, f64)> {
    let mut d = vec![0.0];
    for k in 1..arr.len() {
        d.push(d[k - 1] + (arr[k].0 - arr[k - 1].0).hypot(arr[k].1 - arr[k - 1].1));
    }
    let total = d.last().copied().filter(|x| *x > 0.0).unwrap_or(1.0);
    let mut out = Vec::with_capacity(n);
    let mut j = 0;
    for s in 0..n {
        let t = total * s as f64 / (n - 1) as f64;
        while j + 2 < d.len() && d[j + 1] < t {
            j += 1;
        }
        let seg = if d.len() > j + 1 { d[j + 1] - d[j] } else { 0.0 };
        let seg = if seg == 0.0 { 1.0 } else { seg };
        let f = clamp((t - d[j]) / seg, 0.0, 1.0);
        let a = arr[j];
        let b = *arr.get(j + 1).unwrap_or(&a);
        out.push((a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f));
    }
    out
}

fn mean_dist(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    a.iter().zip(b).map(|(p, q)| (p.0 - q.0).hypot(p.1 - q.1)).sum::<f64>() / a.len() as f64
}

/// `strokeQuiz`: draw each stroke; it counts when shape and direction match
/// the KanjiVG stroke. `blind` hides the grey model.
#[component]
pub fn StrokeQuiz(
    ch: String,
    paths: Vec<String>,
    blind: bool,
    #[prop(optional)] on_score: Option<Callback<u32>>,
    #[prop(optional)] on_out: Option<Callback<()>>,
) -> impl IntoView {
    let attempt = RwSignal::new(0u32);
    let ch2 = ch.clone();
    move || {
        attempt.get();
        quiz_round(ch2.clone(), paths.clone(), blind, on_score, on_out, attempt)
    }
}

fn quiz_round(
    _ch: String,
    paths: Vec<String>,
    blind: bool,
    on_score: Option<Callback<u32>>,
    on_out: Option<Callback<()>>,
    attempt: RwSignal<u32>,
) -> AnyView {
    let node = NodeRef::<Div>::new();
    let total = paths.len();
    let status = RwSignal::new(format!("Traço 1/{total}"));
    let result = RwSignal::new(None::<u32>);
    let st = Rc::new(RefCell::new(QState { i: 0, miss: 0, first_try: 0, pts: None, line: None, finished: false }));
    let svg = stroke_svg(&paths, !blind);

    let to_svg = move |e: &web_sys::PointerEvent| -> Option<(f64, f64)> {
        let root: Element = node.get_untracked()?.into();
        let svg = root.query_selector("svg").ok()??.dyn_into::<SvgsvgElement>().ok()?;
        let m = svg.unchecked_ref::<SvgGraphicsElement>().get_screen_ctm()?.inverse().ok()?;
        let p = svg.create_svg_point();
        p.set_x(e.client_x() as f32);
        p.set_y(e.client_y() as f32);
        let q = p.matrix_transform(&m);
        Some((q.x() as f64, q.y() as f64))
    };
    let hint = move |i: usize| {
        let Some(root) = node.get_untracked() else { return };
        let root: Element = root.into();
        let ps = inks(&root);
        let Some(p) = ps.get(i) else { return };
        let Ok(Some(ug)) = root.query_selector(".userg") else { return };
        if let Ok(c) = p.clone_node() {
            let c: Element = c.unchecked_into();
            let _ = c.set_attribute("class", "hint");
            let _ = c.set_attribute("style", "opacity:1;stroke-dasharray:none");
            let _ = ug.append_child(&c);
            set_timeout(move || c.remove(), std::time::Duration::from_millis(900));
        }
    };

    let s1 = st.clone();
    let down = move |e: web_sys::PointerEvent| {
        let mut s = s1.borrow_mut();
        if s.i >= total || s.finished {
            return;
        }
        let Some(root) = node.get_untracked() else { return };
        let root: Element = root.into();
        let _ = root.set_pointer_capture(e.pointer_id());
        let Some(p) = to_svg(&e) else { return };
        s.pts = Some(vec![p]);
        if let (Ok(Some(ug)), Ok(line)) =
            (root.query_selector(".userg"), crate::util::document().create_element_ns(Some(SVGNS), "polyline"))
        {
            let _ = line.set_attribute("class", "user");
            let _ = ug.append_child(&line);
            s.line = Some(line);
        }
    };
    let s2 = st.clone();
    let mv = move |e: web_sys::PointerEvent| {
        let mut s = s2.borrow_mut();
        if s.pts.is_none() {
            return;
        }
        let Some(p) = to_svg(&e) else { return };
        let pts = s.pts.as_mut().unwrap();
        pts.push(p);
        let attr = pts.iter().map(|p| format!("{},{}", p.0, p.1)).collect::<Vec<_>>().join(" ");
        if let Some(l) = &s.line {
            let _ = l.set_attribute("points", &attr);
        }
    };
    let s3 = st.clone();
    let end = move |_e: web_sys::PointerEvent| {
        let Some(root) = node.get_untracked() else { return };
        let root: Element = root.into();
        let (ok, i, miss_now, done_all, first) = {
            let mut s = s3.borrow_mut();
            let Some(mut drawn) = s.pts.take() else { return };
            if let Some(l) = s.line.take() {
                l.remove();
            }
            if drawn.len() < 2 {
                let p = drawn[0];
                drawn.push((p.0 + 0.5, p.1 + 0.5));
            }
            let ps = inks(&root);
            let Some(p) = ps.get(s.i) else { return };
            let u = resample(&drawn, 24);
            let l = len_of(p);
            let e: Vec<(f64, f64)> = (0..24).map(|k| point_at(p, l * k as f64 / 23.0)).collect();
            let rev: Vec<(f64, f64)> = e.iter().rev().cloned().collect();
            let fwd = mean_dist(&u, &e);
            let back = mean_dist(&u, &rev);
            let tol = clamp(l * 0.22, 9.0, 17.0);
            if fwd <= tol && fwd <= back + 1.0 {
                set_style(p, "transition", "none");
                set_style(p, "stroke-dasharray", "none");
                set_style(p, "opacity", "1");
                if s.miss == 0 {
                    s.first_try += 1;
                }
                s.i += 1;
                s.miss = 0;
                let done_all = s.i >= total;
                if done_all {
                    s.finished = true;
                }
                (true, s.i, 0, done_all, s.first_try)
            } else {
                s.miss += 1;
                (false, s.i, s.miss, false, s.first_try)
            }
        };
        if ok {
            status.set(if i < total { format!("Traço {}/{}", i + 1, total) } else { "Pronto!".into() });
            if done_all {
                let pct = (first as f64 / total as f64 * 100.0).round() as u32;
                store::with_today(|d| d.games += 1.0);
                store::add_xp((total as f64 / 2.0).round() + if pct == 100 { 5.0 } else { 0.0 });
                result.set(Some(pct));
                if let Some(cb) = on_score {
                    cb.run(pct);
                }
            }
        } else {
            if let Ok(Some(b)) = root.closest(".strokebox") {
                let _ = b.class_list().remove_1("shake");
                reflow(&b);
                let _ = b.class_list().add_1("shake");
            }
            if miss_now >= 2 {
                hint(i);
            }
        }
    };
    let end2 = end.clone();
    let s4 = st.clone();
    view! {
        <div class="stack">
            <div class="strokebox" node_ref=node inner_html=svg
                on:pointerdown=down on:pointermove=mv on:pointerup=end on:pointercancel=end2></div>
            <div class="row" style="justify-content:center">
                <span class="pill">{move || status.get()}</span>
                <button class="btn sm" on:click=move |_| { let i = s4.borrow().i; hint(i); }>"Dica"</button>
                {on_out.map(|cb| view! { <button class="btn sm ghost" on:click=move |_| cb.run(())>"Sair"</button> })}
            </div>
            {move || result.get().map(|pct| view! {
                <div class=format!("banner {}", if pct >= 80 { "ok" } else { "warn" })>
                    <span class="bico">{if pct >= 80 { "🖌️" } else { "✍️" }}</span>
                    <div class="grow">{format!("{pct}% dos traços certos de primeira.")}</div>
                    <button class="btn sm" on:click=move |_| attempt.update(|a| *a += 1)>"De novo"</button>
                    {on_out.map(|cb| view! { <button class="btn sm primary" on:click=move |_| cb.run(())>"Continuar"</button> })}
                </div>
            })}
        </div>
    }
    .into_any()
}

/* ---------------- kanji detail ---------------- */

pub fn kanji_of(ch: &str) -> Option<Kanji> {
    let k = data::kanji()?;
    let idx = data::kanji_index()?;
    idx.get(ch).map(|i| k[*i].clone())
}

const STATUS: [&str; 4] = ["novo", "aprendendo", "jovem", "maduro"];
const STATUS_CLS: [&str; 4] = ["", "warn", "line", "ok"];

/// `openKanji(ch)`.
pub fn open_kanji(ch: &str) {
    let Some(k) = kanji_of(ch) else { return };
    let title = format!(
        r#"<span class="hand" style="font-size:30px">{}</span> <span class="small muted">{}</span>"#,
        esc(ch),
        esc(&k.en.iter().take(2).cloned().collect::<Vec<_>>().join(", "))
    );
    let ch = ch.to_string();
    let mid = detached(|| StoredValue::new(0u32));
    let id = open_modal(&title, true, move || kanji_body(&ch, mid.get_value()));
    mid.set_value(id);
}

fn kanji_body(ch: &str, mid: u32) -> AnyView {
    let Some(k) = kanji_of(ch) else { return view! { <div></div> }.into_any() };
    let c = store::card(&format!("k:{ch}"));
    let st = Card::status(c.as_ref()) as usize;
    let lvl = k.n.map(|n| format!("N{n}")).unwrap_or_else(|| "jōyō".into());
    let grade = k.g.map(|g| if g <= 6 { format!("{g}º ano") } else { "secundário".into() });
    let ivl = c.filter(|c| c.state() == 2).map(|c| format!(" · {}", fmt_ivl(c.ivl()))).unwrap_or_default();
    let parts: String = k
        .parts
        .iter()
        .map(|p| if kanji_of(p).is_some() { format!(r##"<a href="#" data-kk="{0}">{0}</a>"##, esc(p)) } else { esc(p) })
        .collect::<Vec<_>>()
        .join(" + ");
    let mut dl = format!(
        "<dt>ON</dt><dd>{}</dd><dt>KUN</dt><dd>{}</dd>",
        esc(&if k.on.is_empty() { "—".into() } else { k.on.join("、") }),
        esc(&if k.kun.is_empty() { "—".into() } else { k.kun.join("、") })
    );
    if !k.rad.is_empty() {
        dl.push_str(&format!("<dt>RADICAL</dt><dd>{}</dd>", esc(&k.rad)));
    }
    if !k.parts.is_empty() {
        dl.push_str(&format!("<dt>PARTES</dt><dd>{parts}</dd>"));
    }
    let ex: String = if k.ex.is_empty() {
        r#"<div class="small muted">Sem palavras-exemplo nas listas JLPT.</div>"#.into()
    } else {
        k.ex.iter()
            .map(|e| {
                format!(
                    r#"<div class="ex"><div class="row" style="gap:8px"><span class="jp" style="font-size:20px">{}</span><span class="jp muted">{}</span>{}<span class="pill">N{}</span></div><div class="small">{}</div></div>"#,
                    esc(&e.0),
                    esc(&e.1),
                    spk(&e.1, "btn xs"),
                    e.3.map(|x| x.to_string()).unwrap_or_default(),
                    esc(&e.2)
                )
            })
            .collect()
    };
    let ch_l = ch.to_string();
    let ch_k = ch.to_string();
    let ch_s = ch.to_string();
    let jisho = format!("https://jisho.org/search/{}%20%23kanji", js_sys::encode_uri_component(ch));
    view! {
        <div class="grid2">
            <StrokeViewer ch=ch_s/>
            <div class="stack">
                <div class="row">
                    <span class="pill line">{lvl}</span>
                    {grade.map(|g| view! { <span class="pill">{g}</span> })}
                    {k.s.map(|s| view! { <span class="pill">{format!("{s} traços")}</span> })}
                    {k.f.map(|f| view! { <span class="pill">{format!("freq. #{f}")}</span> })}
                    <span class=format!("pill {}", STATUS_CLS[st])>{format!("{}{}", STATUS[st], ivl)}</span>
                </div>
                <div style="font-size:21px;font-weight:700">{k.en.join(", ")}</div>
                {(!k.pt.is_empty()).then(|| view! { <div class="muted">{format!("🇧🇷 {}", k.pt.join(", "))}</div> })}
                <dl class="readings" inner_html=dl on:click=move |ev: web_sys::MouseEvent| {
                    let t = ev.target().and_then(|t| t.dyn_into::<Element>().ok());
                    if let Some(a) = t.and_then(|t| t.closest("[data-kk]").ok().flatten()) {
                        ev.prevent_default();
                        close_modal(mid);
                        open_kanji(&a.get_attribute("data-kk").unwrap_or_default());
                    }
                }></dl>
                <div class="stack" inner_html=ex></div>
                <div class="row">
                    {c.is_none().then(|| view! {
                        <button class="btn sm primary" on:click=move |_| {
                            close_modal(mid);
                            go("revisar", Some(json!({"custom": [format!("k:{ch_l}")], "title": format!("Kanji {ch_l}")})));
                        }>"＋ Estudar agora"</button>
                        <button class="btn sm" on:click=move |_| {
                            store::set_card(&format!("k:{ch_k}"), Some(mark_known(21.0, now_ms(), crate::util::rand(), crate::util::rand())));
                            toast(&format!("{ch_k} marcado como conhecido."), "ok");
                            close_modal(mid);
                        }>"Já sei este"</button>
                    })}
                    {SENSEI_ON.then(|| view! { <button class="btn sm" on:click=|_| go("sensei", None)>"先生 Mnemônico"</button> })}
                    <a class="btn sm" href=jisho target="_blank" rel="noopener">"Jisho ↗"</a>
                </div>
            </div>
        </div>
    }
    .into_any()
}

/* ---------------- kanji browser ---------------- */

#[component]
pub fn KanjiBrowser() -> impl IntoView {
    let lvl = RwSignal::new(Some(5u32));
    let q = RwSignal::new(String::new());
    let rev = RwSignal::new(0u64);
    let confirm = RwSignal::new(false);
    let matches = move |k: &Kanji, lv: Option<u32>, q: &str| -> bool {
        let level_ok = match lv {
            None => true,
            Some(0) => k.n.is_none() || k.n == Some(0),
            Some(n) => k.n == Some(n),
        };
        if !level_ok {
            return false;
        }
        if q.is_empty() || k.k == q {
            return true;
        }
        let hq = to_hira(q);
        k.en.iter().any(|m| m.to_lowercase().contains(q))
            || k.pt.iter().any(|m| m.to_lowercase().contains(q))
            || k.on.iter().chain(k.kun.iter()).any(|r| to_hira(r).replace(['.', '-'], "").starts_with(&hq))
    };
    let segs = [(Some(5), "N5"), (Some(4), "N4"), (Some(3), "N3"), (Some(2), "N2"), (Some(1), "N1"), (Some(0), "+jōyō"), (None, "Todos")]
        .into_iter()
        .map(|(v, l)| view! { <button aria-pressed=move || (lvl.get() == v).to_string() on:click=move |_| { lvl.set(v); confirm.set(false); }>{l}</button> })
        .collect_view();
    let pending = RwSignal::new(0u32);
    view! {
        <div class="stack">
            <div class="row-between">
                <div class="seg" role="group">{segs}</div>
                <input type="search" placeholder="Buscar: significado, leitura ou kanji" style="min-width:min(100%,260px)"
                    on:input=move |ev| {
                        let v = ev_value(&ev).trim().to_lowercase();
                        pending.update(|p| *p += 1);
                        let me = pending.get_untracked();
                        set_timeout(move || if pending.get_untracked() == me { q.set(v.clone()) }, std::time::Duration::from_millis(250));
                    }/>
            </div>
            {move || {
                rev.get();
                let (lv, qq) = (lvl.get(), q.get());
                let all = data::kanji().unwrap_or_default();
                let list: Vec<&Kanji> = all.iter().filter(|k| matches(k, lv, &qq)).collect();
                let mut counts = [0usize; 4];
                for k in &list {
                    counts[Card::status(store::card(&format!("k:{}", k.k)).as_ref()) as usize] += 1;
                }
                let n_list = list.len();
                let tiles = list.iter().map(|k| {
                    let ch = k.k.clone();
                    let cls = format!("ktile s{}", Card::status(store::card(&format!("k:{}", k.k)).as_ref()));
                    view! { <button class=cls title={k.en.iter().take(2).cloned().collect::<Vec<_>>().join(", ")} on:click=move |_| open_kanji(&ch)>{k.k.clone()}</button> }
                }).collect_view();
                let novos: Vec<String> = list.iter().filter(|k| store::card(&format!("k:{}", k.k)).is_none()).map(|k| format!("k:{}", k.k)).collect();
                let n_new = novos.len();
                view! {
                    <div class="row small">
                        <span class="pill">{format!("{} novos", counts[0])}</span>
                        <span class="pill warn">{format!("{} aprendendo", counts[1])}</span>
                        <span class="pill line">{format!("{} jovens", counts[2])}</span>
                        <span class="pill ok">{format!("{} maduros", counts[3])}</span>
                        <span class="faint">{format!("· {n_list} kanji")}</span>
                        {(lv.is_some() && counts[0] > 0).then(|| view! { <button class="btn xs" on:click=move |_| confirm.set(true)>"Já sei todos estes…"</button> })}
                    </div>
                    {move || confirm.get().then(|| {
                        let novos = novos.clone();
                        view! {
                            <div class="banner warn"><span class="bico">"⚠️"</span>
                                <div class="grow">{format!("Marcar {n_new} kanji como conhecidos? Eles entram como revisões espaçadas (não somem), então você confirma se sabe mesmo.")}</div>
                                <button class="btn sm danger" on:click=move |_| {
                                    let now = now_ms();
                                    for id in &novos { store::set_card(id, Some(mark_known(21.0, now, crate::util::rand(), crate::util::rand()))); }
                                    toast(&format!("{} kanji marcados.", novos.len()), "ok");
                                    confirm.set(false);
                                    rev.update(|v| *v += 1);
                                }>"Marcar"</button>
                                <button class="btn sm ghost" on:click=move |_| confirm.set(false)>"Cancelar"</button>
                            </div>
                        }
                    })}
                    <div class="kgrid">{tiles}</div>
                }
            }}
        </div>
    }
}

/* ---------------- mining ---------------- */

const SOURCES: [&str; 7] = ["Wagotabi", "Vlog", "Genki", "Anime/drama", "Podcast", "Leitura", "Outro"];

/// `openMine(prefill)`: a word found in a game, vlog or book becomes a card.
pub fn open_mine(prefill: MinedItem, on_saved: Option<Callback<()>>) {
    detached(move || {
        let f = RwSignal::new(prefill.front.clone());
        let r = RwSignal::new(prefill.reading.clone());
        let m = RwSignal::new(prefill.meaning.clone());
        let s = RwSignal::new(prefill.sentence.clone());
        let src =
            RwSignal::new(if prefill.source.is_empty() { "Wagotabi".to_string() } else { prefill.source.clone() });
        let mid = StoredValue::new(0u32);
        let id = open_modal("⛏️ Minerar palavra", false, move || {
            let opts = SOURCES
                .iter()
                .map(|x| view! { <option selected=src.get_untracked() == *x>{*x}</option> })
                .collect_view();
            view! {
                <div class="stack">
                    <p class="small muted">"Achou uma palavra no Wagotabi, num vlog ou no Genki? Guarde aqui: vira carta no baralho Mineração e pode ser exportada para o Anki."</p>
                    <div class="grid2">
                        <div class="field"><label>"Palavra / expressão"</label><input type="text" prop:value=f.get_untracked() placeholder="例: 大丈夫" on:input=move |ev| f.set(ev_value(&ev))/></div>
                        <div class="field"><label>"Leitura (kana)"</label><input type="text" prop:value=r.get_untracked() placeholder="だいじょうぶ" on:input=move |ev| r.set(ev_value(&ev))/></div>
                    </div>
                    <div class="field"><label>"Significado (EN)"</label><input type="text" prop:value=m.get_untracked() placeholder="OK, all right" on:input=move |ev| m.set(ev_value(&ev))/></div>
                    <div class="field"><label>"Frase onde apareceu (opcional)"</label><input type="text" prop:value=s.get_untracked() on:input=move |ev| s.set(ev_value(&ev))/></div>
                    <div class="field"><label>"Fonte"</label><select on:change=move |ev| src.set(ev_value(&ev))>{opts}</select></div>
                    <div class="row">
                        <button class="btn primary" on:click=move |_| {
                            let front = f.get_untracked().trim().to_string();
                            if front.is_empty() { toast("Escreva a palavra.", ""); return; }
                            let item = MinedItem {
                                id: uid(), front, reading: r.get_untracked().trim().into(), meaning: m.get_untracked().trim().into(),
                                sentence: s.get_untracked().trim().into(), source: src.get_untracked(), added: now_ms(), ..MinedItem::default()
                            };
                            store::write(&["mined"], |st| st.mined.list.push(item));
                            store::add_xp(3.0);
                            toast("Carta minerada!", "ok");
                            close_modal(mid.get_value());
                            if let Some(cb) = on_saved { cb.run(()); }
                        }>"Salvar carta"</button>
                    </div>
                </div>
            }
            .into_any()
        });
        mid.set_value(id);
    });
}

#[component]
pub fn MinedList() -> impl IntoView {
    let rev = RwSignal::new(0u64);
    let del = RwSignal::new(None::<String>);
    move || {
        rev.get();
        let list = store::read(|s| s.mined.list.clone());
        let n = list.len();
        let rows = list
            .iter()
            .rev()
            .map(|x| {
                let id = x.id.clone();
                view! {
                    <tr><td class="jp">{x.front.clone()}</td><td class="jp">{x.reading.clone()}</td><td>{x.meaning.clone()}</td><td class="small">{x.source.clone()}</td>
                        <td><button class="btn xs ghost" aria-label="Apagar" on:click=move |_| del.set(Some(id.clone()))>"✕"</button></td></tr>
                }
            })
            .collect_view();
        view! {
            <div class="stack">
                <div class="row-between"><p class="muted">{format!("{n} cartas suas.")}</p>
                    <button class="btn primary sm" on:click=move |_| open_mine(MinedItem::default(), Some(Callback::new(move |_| rev.update(|v| *v += 1))))>"⛏️ Minerar palavra"</button></div>
                {if n > 0 {
                    view! { <div class="table-wrap"><table class="t"><thead><tr><th>"Palavra"</th><th>"Leitura"</th><th>"Significado"</th><th>"Fonte"</th><th></th></tr></thead><tbody>{rows}</tbody></table></div> }.into_any()
                } else {
                    view! { <div class="empty">"Nada minerado ainda. Quando o Wagotabi ou um vlog mostrar uma palavra nova, guarde aqui."</div> }.into_any()
                }}
                {move || del.get().map(|id| view! {
                    <div class="banner warn"><span class="bico">"⚠️"</span><div class="grow">"Apagar esta carta e o histórico dela?"</div>
                        <button class="btn sm danger" on:click=move |_| {
                            let id2 = id.clone();
                            store::write(&["mined"], |s| s.mined.list.retain(|x| x.id != id2));
                            store::set_card(&format!("m:{id}"), None);
                            del.set(None);
                            rev.update(|v| *v += 1);
                        }>"Apagar"</button>
                        <button class="btn sm ghost" on:click=move |_| del.set(None)>"Cancelar"</button>
                    </div>
                })}
            </div>
        }
    }
}
