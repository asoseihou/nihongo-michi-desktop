//! Frases (port of `15-frases.js` + `15b-fala.js`): build the sentence,
//! particles, X-ray, dictation, graded readings and 🎤 speaking practice.

use crate::app::{take_intent, toast, SENSEI_ON};
use crate::data::{self, Content, Point, Sentence};
use crate::jp::{chunk_text, plain_j, role, role_legend, ruby, segments, sent_html, sent_plain, spk, to_hira, Seg};
use crate::logic::{current_station, station_points};
use crate::store;
use crate::tauri::{self, NoArgs};
use crate::tts::{say, say_wait};
use crate::util::{esc, ev_checked, md, shuffle};
use leptos::prelude::*;
use michi_core::time::now_ms;
use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;

const CORE_P: [&str; 13] = ["は", "が", "を", "に", "で", "へ", "と", "も", "の", "から", "まで", "や", "より"];

/* ---------------- shared ---------------- */

/// `poolPoints(sel)`: chosen points, else studied ones, else the current station's.
fn pool_points(c: &Content, sel: &[String]) -> Vec<Point> {
    if !sel.is_empty() {
        return sel.iter().filter_map(|id| c.point(id).cloned()).collect();
    }
    let studied: Vec<Point> = store::read(|s| {
        s.j.points.iter().filter(|(_, x)| x.studied > 0.0).filter_map(|(pid, _)| c.point(pid).cloned()).collect()
    });
    if !studied.is_empty() {
        return studied;
    }
    let pts: Vec<Point> =
        current_station(c).map(|st| station_points(c, st).into_iter().cloned().collect()).unwrap_or_default();
    if !pts.is_empty() {
        return pts;
    }
    c.lesson("01-1").map(|l| l.points.clone()).unwrap_or_else(|| c.points().take(6).cloned().collect())
}

/// `recordPoint(pid, ok)`.
fn record_point(pid: &str, ok: bool) {
    if pid.is_empty() {
        return;
    }
    store::write(&["journey"], |s| {
        let x = s.j.points.entry(pid.to_string()).or_default();
        x.tries += 1.0;
        if ok {
            x.ok += 1.0;
        }
    });
    if ok {
        store::with_today(|d| d.sent += 1.0);
    }
}

fn round_done(xp: f64) {
    store::with_today(|d| d.games += 1.0);
    store::add_xp(xp);
}

fn chunk_tile_html(c: &data::Chunk) -> String {
    format!("{}{}", ruby(&c.w, ""), esc(c.p.as_deref().unwrap_or("")))
}

fn ids_of(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

#[derive(Clone, PartialEq)]
enum Page {
    Hub,
    Practice { mode: String, pids: Vec<String> },
    Reading(String),
    Speak { pids: Vec<String> },
}

#[component]
pub fn Frases() -> impl IntoView {
    let intent = take_intent();
    let start = match intent.as_ref().and_then(|i| i.get("mode").and_then(Value::as_str)) {
        Some("fala") => Page::Speak { pids: intent.as_ref().map(|i| ids_of(&i["pids"])).unwrap_or_default() },
        Some(m) => Page::Practice {
            mode: m.to_string(),
            pids: intent.as_ref().map(|i| ids_of(&i["pids"])).unwrap_or_default(),
        },
        None => Page::Hub,
    };
    let page = RwSignal::new(start);
    let ready = RwSignal::new(false);
    spawn_local(async move {
        let _ = data::need_content().await;
        let _ = data::need_kanji().await;
        ready.set(true);
    });
    let gen = RwSignal::new(0u32);
    move || {
        if !ready.get() {
            return view! { <div class="loading"><div class="spin"></div>"Carregando frases…"</div> }.into_any();
        }
        gen.get();
        crate::util::scroll_top();
        match page.get() {
            Page::Hub => hub(page),
            Page::Practice { mode, pids } => view! { <Practice mode=mode pids=pids page=page gen=gen/> }.into_any(),
            Page::Reading(id) => view! { <Reading rid=id page=page/> }.into_any(),
            Page::Speak { pids } => view! { <Speak pids=pids page=page gen=gen/> }.into_any(),
        }
    }
}

fn hub(page: RwSignal<Page>) -> AnyView {
    let c = data::content().unwrap_or_default();
    let pts = pool_points(&c, &[]);
    let mut modes = vec![
        (
            "builder",
            "🧩",
            "Montar a frase",
            "Coloque os pedaços na ordem. Ative \"do fim\" para montar a partir do predicado.",
        ),
        ("particles", "を", "Partícula certa", "Complete as partículas que faltam."),
        ("xray", "🔬", "Raio-X", "Toque no pedaço que responde à pergunta: onde? quem? o quê?"),
        ("dictation", "🎧", "Ditado", "Ouça e monte o que ouviu — sem ver a tradução."),
        ("fala", "🎤", "Falar", "Leia em voz alta ou diga a frase em japonês. O app confere pedaço por pedaço."),
    ];
    if SENSEI_ON {
        modes.push((
            "analyze",
            "先生",
            "Analisar qualquer frase",
            "Cole uma frase do Wagotabi ou de um vlog e veja a anatomia.",
        ));
        modes.push((
            "generate",
            "✨",
            "Frases novas com IA",
            "O Sensei cria frases novas no formato do app para os seus pontos.",
        ));
    }
    let cards = modes
        .into_iter()
        .map(|(m, j, t, d)| {
            view! {
                <button class="gamecard" on:click=move |_| page.set(if m == "fala" { Page::Speak { pids: vec![] } } else { Page::Practice { mode: m.to_string(), pids: vec![] } })>
                    <span class="gj">{j}</span><span class="gt">{t}</span><span class="small muted">{d}</span>
                </button>
            }
        })
        .collect_view();
    let reads = c.readings.clone();
    let n_reads = reads.len();
    let read_rows = reads
        .iter()
        .filter_map(|r| {
            let pt = r.points.first()?;
            let done = store::read(|s| s.j.points.get(&pt.id).map(|x| x.read > 0.0).unwrap_or(false));
            let id = r.id.clone();
            Some(view! {
                <button class="res" style="text-align:left;cursor:pointer;font:inherit;color:inherit" on:click=move |_| page.set(Page::Reading(id.clone()))>
                    <div><div class="rn">{r.title.clone()}" "<span class="jp muted small">{r.title_jp.clone()}</span></div><div class="small muted">{pt.gist.clone()}</div></div>
                    <span class="pill" class:ok=done>{if done { "✓ lido".to_string() } else { format!("N{}", r.jlpt) }}</span>
                </button>
            })
        })
        .collect_view();
    let names: Vec<String> = pts.iter().take(4).map(|p| p.title.clone()).collect();
    view! {
        <div class="row-between"><h1>"Frases"</h1><div class="small muted">"Sua maior dificuldade é a estrutura da frase — aqui cada pedaço tem cor e papel."</div></div>
        <div class="card stack"><div class="eyebrow">"Como ler qualquer frase japonesa"</div>
            <div class="small">"1) Ache o "<b>"predicado"</b>" no fim — é o que acontece. 2) Volte pedaço por pedaço: cada partícula é uma etiqueta que diz o papel (は tópico, が sujeito, を objeto, に/へ destino, で lugar/meio…). 3) Tudo que vem antes de um substantivo e não tem partícula o "<b>"descreve"</b>"."</div>
            <div inner_html=role_legend(&[])></div>
        </div>
        <div class="grid3">{cards}</div>
        <div class="card stack"><div class="row-between"><h2>"Leitura graduada"</h2><span class="small muted">{format!("{n_reads} textos")}</span></div>
            {if n_reads > 0 { view! { <div class="stack">{read_rows}</div> }.into_any() } else { view! { <div class="empty">"Os textos de leitura ainda estão sendo preparados."</div> }.into_any() }}
        </div>
        <div class="small muted">{format!("Praticando com: {} pontos ({}{}). Para escolher outros, abra uma estação na Jornada.", pts.len(), names.join(", "), if pts.len() > 4 { "…" } else { "" })}</div>
    }
    .into_any()
}

/* ---------------- practice: builder · dictation · particles · X-ray ---------------- */

#[derive(Clone, PartialEq)]
enum Fb {
    None,
    Shown(bool),
}

#[component]
fn Practice(mode: String, pids: Vec<String>, page: RwSignal<Page>, gen: RwSignal<u32>) -> impl IntoView {
    let c = data::content().unwrap_or_default();
    let pts = pool_points(&c, &pids);
    let mut bank: Vec<Sentence> =
        shuffle(&pts.iter().flat_map(|p| p.s.iter().cloned()).collect::<Vec<_>>()).into_iter().take(10).collect();
    if mode == "particles" {
        bank.retain(|s| s.c.iter().any(|c| c.p.as_deref().map(|p| CORE_P.contains(&p)).unwrap_or(false)));
    }
    if mode == "xray" {
        bank.retain(|s| s.c.len() >= 3);
    }
    let title = match mode.as_str() {
        "builder" => "🧩 Montar a frase",
        "particles" => "を Partícula certa",
        "xray" => "🔬 Raio-X",
        _ => "🎧 Ditado",
    };
    if bank.is_empty() {
        return view! {
            <button class="btn ghost sm" style="align-self:flex-start" on:click=move |_| page.set(Page::Hub)>"← Frases"</button>
            <div class="empty">"Sem frases para este modo ainda. Estude alguns pontos na Jornada primeiro."</div>
        }
        .into_any();
    }
    let total = bank.len();
    let bank = StoredValue::new(bank);
    let i = RwSignal::new(0usize);
    let right = RwSignal::new(0usize);
    let fb = RwSignal::new(Fb::None);
    let finished = RwSignal::new(false);
    let mode2 = mode.clone();

    let reveal = move |ok: bool| {
        let s = bank.with_value(|b| b[i.get_untracked()].clone());
        if ok {
            right.update(|r| *r += 1);
        }
        record_point(&s.pid, ok);
        fb.set(Fb::Shown(ok));
        if ok {
            say(&sent_plain(&s), None);
        }
    };
    let next = move || {
        fb.set(Fb::None);
        if i.get_untracked() + 1 >= total {
            round_done(5.0 + right.get_untracked() as f64 * 2.0);
            finished.set(true);
        } else {
            i.update(|x| *x += 1);
        }
    };
    let head = move || {
        view! {
            <div class="row-between">
                <button class="btn ghost sm" on:click=move |_| page.set(Page::Hub)>"← Frases"</button>
                <h2>{title}</h2>
                <span class="pill">{move || format!("{}/{} · ✔ {}", (i.get() + 1).min(total), total, right.get())}</span>
            </div>
        }
    };
    view! {
        {head()}
        {move || {
            if finished.get() {
                let r = right.get_untracked();
                return view! {
                    <div class="card stack" style="text-align:center;align-items:center">
                        <h1>{format!("{r}/{total}")}</h1>
                        <p class="muted">{if r as f64 >= total as f64 * 0.8 { "Excelente leitura de estrutura!" } else { "Repare nas cores: o papel de cada pedaço fica mais claro a cada rodada." }}</p>
                        <div class="row">
                            <button class="btn" on:click=move |_| gen.update(|g| *g += 1)>"Outra rodada"</button>
                            <button class="btn primary" on:click=move |_| page.set(Page::Hub)>"Voltar"</button>
                        </div>
                    </div>
                }
                .into_any();
            }
            let n = i.get();
            let s = bank.with_value(|b| b[n].clone());
            let body = match mode2.as_str() {
                "particles" => particles(s.clone(), fb, reveal),
                "xray" => xray(s.clone(), fb, reveal),
                m => builder(s.clone(), m == "dictation", fb, reveal),
            };
            view! {
                <div class="card stack">
                    {body}
                    {move || match fb.get() { Fb::Shown(ok) => feedback(&s, ok, next), Fb::None => view! { <span></span> }.into_any() }}
                </div>
            }
            .into_any()
        }}
    }
    .into_any()
}

fn feedback(s: &Sentence, ok: bool, next: impl Fn() + Copy + 'static) -> AnyView {
    let last = s.c.last().map(chunk_text).unwrap_or_default();
    let rest: String =
        s.c.iter()
            .rev()
            .skip(1)
            .map(|c| {
                format!(
                    r#"<span class="role-{}" style="color:var(--rc);font-weight:700">{}</span> = {}"#,
                    role(&c.r).0,
                    esc(&chunk_text(c)),
                    esc(&role(&c.r).3.to_lowercase())
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
    let read = format!(
        "Leia do fim: <b>{}</b> é o que acontece{}.",
        esc(&last),
        if s.c.len() > 1 { format!("; {rest}") } else { String::new() }
    );
    view! {
        <div class="stack" style="margin-top:6px">
            <div class=format!("banner {}", if ok { "ok" } else { "warn" })>
                <span class="bico">{if ok { "⭕" } else { "❌" }}</span>
                <div class="grow">{if ok { "Certo!" } else { "A ordem natural é esta:" }}</div>
                <span inner_html=spk(&sent_plain(s), "btn sm")></span>
                <button class="btn sm primary" on:click=move |_| next()>"Próxima →"</button>
            </div>
            <div class="sent" inner_html=sent_html(s, true, true, "")></div>
            <div class="en">{s.en.clone()}</div>
            <div class="small muted" inner_html=read></div>
        </div>
    }
    .into_any()
}

/// `showBuilder(dict)`: tap the chunks in order; "do fim" builds from the predicate.
fn builder(s: Sentence, dict: bool, fb: RwSignal<Fb>, reveal: impl Fn(bool) + Copy + 'static) -> AnyView {
    let n = s.c.len();
    let order = StoredValue::new(shuffle(&(0..n).collect::<Vec<_>>()));
    let placed = RwSignal::new(Vec::<usize>::new());
    let from_end = RwSignal::new(store::read(|st| st.p.extra.get("fromEnd").and_then(Value::as_bool).unwrap_or(false)));
    let s = StoredValue::new(s);
    if dict {
        let t = s.with_value(sent_plain);
        set_timeout(move || say(&t, None), std::time::Duration::from_millis(300));
    }
    let locked = move || fb.get() != Fb::None;
    let check = move || {
        let p = placed.get_untracked();
        let seq: Vec<usize> = if from_end.get_untracked() { p.iter().rev().cloned().collect() } else { p };
        s.with_value(|s| {
            let txt = seq.iter().map(|k| chunk_text(&s.c[*k])).collect::<Vec<_>>().join("|");
            let mut orders: Vec<Vec<usize>> = vec![(0..s.c.len()).collect()];
            orders.extend(s.alt.iter().cloned());
            let ok = orders
                .iter()
                .any(|o| o.iter().filter_map(|k| s.c.get(*k)).map(chunk_text).collect::<Vec<_>>().join("|") == txt);
            reveal(ok);
        });
    };
    view! {
        {if dict {
            view! {
                <div style="text-align:center">
                    <button class="btn primary" style="min-height:56px;font-size:18px" on:click=move |_| say(&s.with_value(sent_plain), None)>"🔊 Ouvir a frase"</button>" "
                    <button class="btn sm ghost" on:click=move |_| say(&s.with_value(sent_plain), Some(0.6))>"🐢 devagar"</button>
                </div>
            }.into_any()
        } else {
            view! { <div style="font-size:18px;font-weight:650;text-align:center">{format!("“{}”", s.with_value(|s| s.en.clone()))}</div> }.into_any()
        }}
        <div class="row-between">
            <span class="small muted">{move || if from_end.get() { "Comece pelo predicado (o fim) e vá voltando ←" } else { "Toque nos pedaços na ordem" }}</span>
            <label class="switch small"><input type="checkbox" prop:checked=move || from_end.get() on:change=move |ev| {
                let v = ev_checked(&ev);
                from_end.set(v);
                store::write(&["profile"], |st| { st.p.extra.insert("fromEnd".into(), Value::Bool(v)); });
                placed.set(vec![]);
            }/>" montar do fim"</label>
        </div>
        <div class="slots" class:rtl=move || from_end.get()>
            {move || placed.get().into_iter().map(|k| {
                let (html, r) = s.with_value(|s| (chunk_tile_html(&s.c[k]), role(&s.c[k].r).0));
                view! { <button class=format!("tile placed role-{r}") disabled=locked inner_html=html on:click=move |_| placed.update(|p| p.retain(|x| *x != k))></button> }
            }).collect_view()}
        </div>
        <div class="tilebank">
            {order.get_value().into_iter().map(|k| {
                let html = s.with_value(|s| chunk_tile_html(&s.c[k]));
                view! { <button class="tile" class:used=move || placed.get().contains(&k) disabled=locked inner_html=html
                    on:click=move |_| if !placed.get_untracked().contains(&k) { placed.update(|p| p.push(k)) }></button> }
            }).collect_view()}
        </div>
        <div class="row" style="justify-content:center">
            <button class="btn primary" disabled=move || placed.get().len() != n || locked() on:click=move |_| check()>"Conferir"</button>
            <button class="btn ghost" disabled=locked on:click=move |_| reveal(false)>"Pular"</button>
        </div>
    }
    .into_any()
}

/// `showParticles`: fill the particles one by one.
fn particles(s: Sentence, fb: RwSignal<Fb>, reveal: impl Fn(bool) + Copy + 'static) -> AnyView {
    let blanks: Vec<usize> =
        s.c.iter()
            .enumerate()
            .filter(|(_, c)| c.p.as_deref().map(|p| CORE_P.contains(&p)).unwrap_or(false))
            .map(|(k, _)| k)
            .collect();
    let mut set: Vec<String> = blanks.iter().filter_map(|k| s.c[*k].p.clone()).collect();
    let need = {
        let mut u = set.clone();
        u.sort();
        u.dedup();
        u.len()
    };
    set.extend(shuffle(&CORE_P.iter().map(|x| x.to_string()).collect::<Vec<_>>()).into_iter().take(8));
    set.sort();
    set.dedup();
    let opts: Vec<String> = shuffle(&set).into_iter().take(6.max(need + 3)).collect();
    let at = RwSignal::new(0usize);
    let miss = RwSignal::new(0u32);
    let wrong = RwSignal::new(Vec::<String>::new());
    let s = StoredValue::new(s);
    let blanks = StoredValue::new(blanks);
    let sentence = move || {
        let a = at.get();
        s.with_value(|s| {
            blanks.with_value(|b| {
                s.c.iter()
                    .enumerate()
                    .map(|(k, c)| {
                        let p = match &c.p {
                            Some(p) if b.contains(&k) => {
                                let idx = b.iter().position(|x| *x == k).unwrap_or(0);
                                if idx < a {
                                    format!(
                                        r#"<span class="pt" style="color:var(--ok);font-weight:700">{}</span>"#,
                                        esc(p)
                                    )
                                } else {
                                    format!(r#"<span class="blank">{}</span>"#, if idx == a { "？" } else { "＿" })
                                }
                            }
                            Some(p) => esc(p),
                            None => String::new(),
                        };
                        format!("{}{}", ruby(&c.w, ""), p)
                    })
                    .collect::<String>()
                    + &esc(&crate::jp::end_of(s))
            })
        })
    };
    let btns = opts
        .into_iter()
        .map(|o| {
            let o2 = o.clone();
            view! {
                <button class="opt jpo" style="min-width:64px" class:wrong={let o3 = o.clone(); move || wrong.get().contains(&o3)} disabled=move || fb.get() != Fb::None
                    on:click=move |_| {
                        let total = blanks.with_value(|b| b.len());
                        if at.get_untracked() >= total { return; }
                        let (want, r) = s.with_value(|s| { let k = blanks.with_value(|b| b[at.get_untracked()]); (s.c[k].p.clone().unwrap_or_default(), s.c[k].r.clone()) });
                        if o2 == want {
                            wrong.set(vec![]);
                            at.update(|a| *a += 1);
                            if at.get_untracked() >= total { reveal(miss.get_untracked() == 0); }
                        } else {
                            miss.update(|m| *m += 1);
                            wrong.update(|w| w.push(o2.clone()));
                            let x = role(&r);
                            toast(&format!("Aqui o papel é {} ({}).", x.1, x.3), "");
                        }
                    }>{o.clone()}</button>
            }
        })
        .collect_view();
    view! {
        <div class="sent big" style="text-align:center" inner_html=sentence></div>
        <div class="en" style="text-align:center">{s.with_value(|s| s.en.clone())}</div>
        <div class="qbtns">{btns}</div>
    }
    .into_any()
}

/// `showXray`: find the predicate, then the chunk that answers each question.
fn xray(s: Sentence, fb: RwSignal<Fb>, reveal: impl Fn(bool) + Copy + 'static) -> AnyView {
    let n = s.c.len();
    let mut cands: Vec<usize> =
        shuffle(&(0..n).filter(|k| s.c[*k].r != "PRED").collect::<Vec<_>>()).into_iter().take(2).collect();
    cands.insert(0, n - 1);
    let qn = cands.len();
    let cands = StoredValue::new(cands);
    let qi = RwSignal::new(0usize);
    let miss = RwSignal::new(0u32);
    let shake = RwSignal::new(None::<usize>);
    let s = StoredValue::new(s);
    let question = move || {
        let q = qi.get().min(qn - 1);
        let t = cands.with_value(|c| c[q]);
        if q == 0 {
            "Ache o PREDICADO (o que acontece / o que é)".to_string()
        } else {
            let r = s.with_value(|s| role(&s.c[t].r));
            format!(r#"Qual pedaço responde: <span class="role-{}" style="color:var(--rc)">{}</span>"#, r.0, esc(r.3))
        }
    };
    let tiles = (0..n)
        .map(|k| {
            let html = s.with_value(|s| chunk_tile_html(&s.c[k]));
            view! {
                <button class="tile" class:wrong=move || shake.get() == Some(k) disabled=move || fb.get() != Fb::None inner_html=html on:click=move |_| {
                    if qi.get_untracked() >= qn { return; }
                    let target = cands.with_value(|c| c[qi.get_untracked()]);
                    let same = s.with_value(|s| k == target || s.c[k].r == s.c[target].r);
                    if same {
                        qi.update(|q| *q += 1);
                        if qi.get_untracked() >= qn { reveal(miss.get_untracked() == 0); }
                    } else {
                        miss.update(|m| *m += 1);
                        shake.set(Some(k));
                        set_timeout(move || { if shake.try_get_untracked().is_some() { shake.set(None) } }, std::time::Duration::from_millis(600));
                        let r = s.with_value(|s| role(&s.c[k].r));
                        toast(&format!("Esse pedaço é {}: responde “{}”.", r.1, r.3), "");
                    }
                }></button>
            }
        })
        .collect_view();
    view! {
        <div style="text-align:center;font-family:var(--f-sign);font-size:22px;font-weight:700" inner_html=question></div>
        <div class="qbtns" style="font-size:22px">{tiles}<span class="jp" style="align-self:center">{s.with_value(crate::jp::end_of)}</span></div>
        <div class="small muted" style="text-align:center">{move || format!("{}/{}", (qi.get() + 1).min(qn), qn)}</div>
    }
    .into_any()
}

/* ---------------- graded reading ---------------- */

#[component]
fn Reading(rid: String, page: RwSignal<Page>) -> impl IntoView {
    let c = data::content().unwrap_or_default();
    let Some(r) = c.readings.iter().find(|x| x.id == rid).cloned() else {
        return view! { <div class="empty">"Texto não encontrado."</div> }.into_any();
    };
    let Some(pt) = r.points.first().cloned() else { return view! { <div></div> }.into_any() };
    let furi = RwSignal::new(store::read(|s| s.p.furi != "never"));
    let anat = RwSignal::new(false);
    let shown = RwSignal::new(vec![false; pt.s.len()]);
    let lit = RwSignal::new(None::<usize>);
    let qi = RwSignal::new(0usize);
    let score = RwSignal::new(0usize);
    let picked = RwSignal::new(None::<usize>);
    let pt = StoredValue::new(pt);
    let vocab: String = r
        .vocab
        .iter()
        .map(|v| {
            format!(
                r#"<button class="btn sm jp" data-say="{}">{} <span class="tiny muted">{}</span></button>"#,
                esc(&plain_j(&v.0)),
                ruby(&v.0, "always"),
                esc(&v.1)
            )
        })
        .collect();
    let has_vocab = !r.vocab.is_empty();
    let n_q = pt.with_value(|p| p.q.len());
    let read_all = move |_| {
        spawn_local(async move {
            let n = pt.with_value(|p| p.s.len());
            for k in 0..n {
                if lit.try_get_untracked().is_none() {
                    return;
                }
                lit.set(Some(k));
                let t = pt.with_value(|p| sent_plain(&p.s[k]));
                say_wait(&t, None).await;
            }
            if lit.try_get_untracked().is_some() {
                lit.set(None);
            }
        });
    };
    let lines = move || {
        let (f, a) = (furi.get(), anat.get());
        let sh = shown.get();
        let l = lit.get();
        pt.with_value(|p| {
            p.s.iter()
                .enumerate()
                .map(|(k, s)| {
                    let html = sent_html(s, a, false, if f { "always" } else { "never" });
                    let en = s.en.clone();
                    view! {
                        <div>
                            <div class="sent" style=if l == Some(k) { "cursor:pointer;background:var(--line-soft)" } else { "cursor:pointer" } inner_html=html
                                on:click=move |_| { shown.update(|v| v[k] = true); say(&pt.with_value(|p| sent_plain(&p.s[k])), None); }></div>
                            {sh[k].then(|| view! { <div class="en">{en}</div> })}
                        </div>
                    }
                })
                .collect_view()
        })
    };
    let questions = move || {
        let q = qi.get();
        if q >= n_q {
            let first = store::read(|s| {
                s.j.points.get(&pt.with_value(|p| p.id.clone())).map(|x| x.read > 0.0).unwrap_or(false)
            });
            if !first {
                let id = pt.with_value(|p| p.id.clone());
                store::write(&["journey"], |s| s.j.points.entry(id).or_default().read = now_ms());
                store::add_xp(25.0);
                store::with_today(|d| d.sent += 5.0);
            }
            return view! { <div class="banner ok"><span class="bico">"📖"</span><div class="grow">{format!("Compreensão: {}/{}.", score.get_untracked(), n_q)}</div></div> }.into_any();
        }
        let item = pt.with_value(|p| p.q[q].clone());
        let ch = picked.get();
        let opts = item
            .opts
            .iter()
            .enumerate()
            .map(|(k, o)| {
                let a = item.a;
                view! {
                    <button class="opt" style="font-size:15px" class:right=ch.is_some() && k == a class:wrong=ch == Some(k) && k != a disabled=ch.is_some()
                        on:click=move |_| {
                            if picked.get_untracked().is_some() { return; }
                            picked.set(Some(k));
                            let good = k == a;
                            if good { score.update(|s| *s += 1); }
                            set_timeout(move || { if qi.try_get_untracked().is_some() { picked.set(None); qi.update(|q| *q += 1); } },
                                std::time::Duration::from_millis(if good { 600 } else { 1400 }));
                        }>{o.clone()}</button>
                }
            })
            .collect_view();
        view! {
            <div class="eyebrow">{format!("Pergunta {}/{}", q + 1, n_q)}</div>
            <div style="font-weight:650">{item.q.clone()}</div>
            <div class="opts" style="grid-template-columns:1fr">{opts}</div>
        }
        .into_any()
    };
    view! {
        <div class="row-between">
            <button class="btn ghost sm" on:click=move |_| page.set(Page::Hub)>"← Frases"</button>
            <span class="pill">{format!("N{}", r.jlpt)}</span>
        </div>
        <div class="card stack">
            <h1>{r.title.clone()}</h1><div class="jp muted">{r.title_jp.clone()}</div>
            <div class="row">
                <button class="btn sm" on:click=read_all>"🔊 Ler tudo"</button>
                <label class="switch small"><input type="checkbox" prop:checked=move || furi.get() on:change=move |ev| furi.set(ev_checked(&ev))/>" furigana"</label>
                <label class="switch small"><input type="checkbox" prop:checked=move || anat.get() on:change=move |ev| anat.set(ev_checked(&ev))/>" cores dos papéis"</label>
            </div>
            <div class="small muted" inner_html=pt.with_value(|p| md(&p.explain))></div>
            <div class="stack">{lines}</div>
            <div class="tiny faint">"Toque numa frase para ouvir e ver a tradução."</div>
        </div>
        {has_vocab.then(|| view! { <div class="card stack"><div class="eyebrow">"Vocabulário"</div><div class="row" style="gap:6px" inner_html=vocab></div></div> })}
        {(n_q > 0).then(|| view! { <div class="card stack">{questions}</div> })}
    }
    .into_any()
}

/* ---------------- 🎤 speaking practice (15b-fala.js) ---------------- */

fn spk_punct(c: char) -> bool {
    c.is_whitespace() || "。、，,．.！!？?・「」『』（）()［］[]〜~…‥:：;；\"'“”‘’—–-".contains(c)
}

fn nfkc(s: &str) -> String {
    js_sys::JsString::from(s).normalize("NFKC").into()
}

fn digits_to_kanji(s: &str) -> String {
    const K: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
    s.chars().map(|c| c.to_digit(10).map(|d| K[d as usize]).unwrap_or(c)).collect()
}

/// `spkNorm`: hiragana, no punctuation, digits as kanji numerals.
fn spk_norm(s: &str) -> String {
    digits_to_kanji(&to_hira(&nfkc(s)).chars().filter(|c| !spk_punct(*c)).collect::<String>())
}

fn ruby_pairs(w: &str) -> Vec<(String, String)> {
    segments(w).into_iter().filter_map(|s| if let Seg::Ruby(k, r) = s { Some((k, r)) } else { None }).collect()
}

thread_local! {
    static SPK_DICT: RefCell<Option<Rc<HashMap<char, Vec<(String, String)>>>>> = const { RefCell::new(None) };
}

/// kanji → kana: every furigana run in the app's sentences + N5–N3 vocabulary.
fn spk_dict() -> Rc<HashMap<char, Vec<(String, String)>>> {
    if let Some(d) = SPK_DICT.with(|d| d.borrow().clone()) {
        return d;
    }
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut add = |k: &str, r: &str| {
        if !k.is_empty() && !r.is_empty() && crate::jp::has_kanji(k) && !seen.contains_key(k) {
            seen.insert(k.to_string(), r.to_string());
        }
    };
    if let Some(v) = data::vocab() {
        for x in v.iter() {
            if !x.0.contains(['～', '〜', '(', '（']) {
                add(&x.0, &x.1);
            }
        }
    }
    if let Some(c) = data::content() {
        for p in c.points() {
            for s in &p.s {
                for ch in &s.c {
                    for (k, r) in ruby_pairs(&ch.w) {
                        add(&k, &r);
                    }
                }
            }
        }
    }
    let mut by: HashMap<char, Vec<(String, String)>> = HashMap::new();
    for (k, r) in seen {
        if let Some(c) = k.chars().next() {
            by.entry(c).or_default().push((k, r));
        }
    }
    for l in by.values_mut() {
        l.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));
    }
    let rc = Rc::new(by);
    SPK_DICT.with(|d| *d.borrow_mut() = Some(rc.clone()));
    rc
}

/// `spkToKana`: best-effort reading of the transcript; the sentence's own furigana wins.
fn spk_to_kana(text: &str, s: &Sentence) -> String {
    let mut own: Vec<(String, String)> = s.c.iter().flat_map(|c| ruby_pairs(&c.w)).collect();
    own.sort_by(|a, b| b.0.chars().count().cmp(&a.0.chars().count()));
    let dict = spk_dict();
    let t: Vec<char> = text.chars().collect();
    let starts = |i: usize, k: &str| -> bool {
        let kc: Vec<char> = k.chars().collect();
        i + kc.len() <= t.len() && t[i..i + kc.len()] == kc[..]
    };
    let mut out = String::new();
    let mut i = 0;
    while i < t.len() {
        let ch = t[i];
        if !crate::jp::is_kanji(ch) {
            out.push(ch);
            i += 1;
            continue;
        }
        let hit = own
            .iter()
            .find(|(k, _)| starts(i, k))
            .or_else(|| dict.get(&ch).and_then(|l| l.iter().find(|(k, _)| starts(i, k))));
        match hit {
            Some((k, r)) => {
                out.push_str(r);
                i += k.chars().count();
            }
            None => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

fn lcs_len(a: &[char], b: &[char]) -> usize {
    let mut dp = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        let mut prev = 0;
        for j in 1..=b.len() {
            let t = dp[j];
            dp[j] = if a[i - 1] == b[j - 1] { prev + 1 } else { dp[j].max(dp[j - 1]) };
            prev = t;
        }
    }
    dp[b.len()]
}

fn index_of(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == needle[..])
}

#[derive(Clone, Debug, PartialEq)]
struct ChunkRes {
    st: &'static str,
    why: String,
}

#[derive(Clone, Debug, PartialEq)]
struct SpkScore {
    res: Vec<ChunkRes>,
    score: f64,
    pass: bool,
}

/// `spkScore(S, heard)`: per chunk ok · part · order · miss, compared in kana.
fn spk_score(s: &Sentence, heard: &str) -> SpkScore {
    let digits = digits_to_kanji(&nfkc(heard));
    let kana: Vec<char> = spk_norm(&spk_to_kana(&digits, s)).chars().collect();
    let mut parts: Vec<&str> = CORE_P.iter().copied().chain(["ね", "よ", "か"]).collect();
    parts.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()));
    let conv = |t: &str| spk_norm(&spk_to_kana(t, s));
    let find_from = |cands: &[String], from: usize| -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize)> = None;
        for c in cands {
            let cc: Vec<char> = c.chars().collect();
            if let Some(a) = index_of(&kana, &cc, from) {
                if best.map(|b| a < b.0).unwrap_or(true) {
                    best = Some((a, a + cc.len()));
                }
            }
        }
        best
    };
    let mut cur = 0usize;
    let res: Vec<ChunkRes> =
        s.c.iter()
            .map(|c| {
                let p = c.p.clone().unwrap_or_default();
                let mut fulls = vec![spk_norm(&(crate::jp::kana_j(&c.w) + &p)), conv(&(plain_j(&c.w) + &p))];
                fulls.dedup();
                fulls.retain(|x| !x.is_empty());
                let mut words = vec![spk_norm(&crate::jp::kana_j(&c.w)), conv(&plain_j(&c.w))];
                words.dedup();
                words.retain(|x| !x.is_empty());
                if let Some((_, end)) = find_from(&fulls, cur) {
                    cur = end;
                    return ChunkRes { st: "ok", why: String::new() };
                }
                if !p.is_empty() {
                    if let Some((_, end)) = find_from(&words, cur) {
                        cur = end;
                        let rest: String = kana[end.min(kana.len())..].iter().collect();
                        let said = parts.iter().find(|x| rest.starts_with(**x));
                        if let Some(x) = said {
                            cur += x.chars().count();
                        }
                        return ChunkRes {
                            st: "part",
                            why: match said {
                                Some(x) => format!("você disse {x} no lugar de {p}"),
                                None => format!("a partícula {p} não saiu"),
                            },
                        };
                    }
                }
                if let Some(f0) = fulls.first() {
                    let nd: Vec<char> = f0.chars().collect();
                    let mut best = (0.0f64, cur);
                    let lo = nd.len().saturating_sub(1).max(1);
                    for l in lo..=nd.len() + 1 {
                        let mut i = cur;
                        while i + l <= kana.len() {
                            let sim = 2.0 * lcs_len(&kana[i..i + l], &nd) as f64 / (l + nd.len()) as f64;
                            if sim > best.0 {
                                best = (sim, i + l);
                            }
                            i += 1;
                        }
                    }
                    if best.0 >= 0.75 {
                        cur = best.1;
                        return ChunkRes { st: "part", why: "quase".into() };
                    }
                }
                if find_from(&fulls, 0).is_some() {
                    return ChunkRes { st: "order", why: "fora de ordem".into() };
                }
                ChunkRes { st: "miss", why: "não ouvi".into() }
            })
            .collect();
    let w = |st: &str| match st {
        "ok" => 1.0,
        "part" => 0.5,
        "order" => 0.6,
        _ => 0.0,
    };
    let score = res.iter().map(|r| w(r.st)).sum::<f64>() / res.len().max(1) as f64;
    let pred_ok = res.last().map(|r| r.st != "miss").unwrap_or(false);
    SpkScore { pass: score >= 0.8 && pred_ok, res, score }
}

#[derive(Deserialize, Clone, Debug, Default)]
struct SttStatus {
    available: bool,
    #[serde(default)]
    model: bool,
    #[serde(default)]
    downloading: bool,
    #[serde(default)]
    size_mb: u64,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct DlProgress {
    done: u64,
    total: u64,
    finished: bool,
    error: Option<String>,
}

/// Download button + progress for the Whisper model.
fn model_download(stt: RwSignal<SttStatus>) -> AnyView {
    let progress = RwSignal::new(None::<f64>);
    let refresh = move || {
        spawn_local(async move {
            if let Ok(s) = tauri::invoke::<SttStatus, _>("stt_status", &NoArgs {}).await {
                stt.set(s);
            }
        })
    };
    tauri::listen::<DlProgress>("michi://stt-progress", move |p| {
        if progress.try_get_untracked().is_none() {
            return;
        }
        if p.finished || p.error.is_some() {
            progress.set(None);
            if let Some(e) = p.error {
                toast(&format!("Não consegui baixar: {e}"), "bad");
            } else {
                toast("Modelo de voz pronto! Agora o Falar ouve você.", "ok");
            }
            refresh();
        } else if p.total > 0 {
            progress.set(Some(p.done as f64 / p.total as f64));
        }
    });
    (move || {
        let s = stt.get();
        if s.available {
            return view! { <span></span> }.into_any();
        }
        let busy = s.downloading || progress.get().is_some();
        view! {
            <div class="card tint stack">
                <div class="small">{s.message.clone()}</div>
                {if busy {
                    let f = progress.get().unwrap_or(0.0);
                    view! { <div class="small muted">{format!("Baixando… {}%", (f * 100.0).round())}</div><div inner_html=crate::app::bar(f, "")></div> }.into_any()
                } else {
                    view! { <button class="btn sm primary" style="align-self:flex-start" on:click=move |_| {
                        progress.set(Some(0.0));
                        tauri::call("stt_download", NoArgs {});
                    }>{format!("⬇️ Baixar o modelo de voz (~{} MB)", if s.size_mb > 0 { s.size_mb } else { 190 })}</button> }.into_any()
                }}
                <div class="tiny faint">"O reconhecimento roda no seu PC (Whisper); o áudio não sai do computador."</div>
            </div>
        }
        .into_any()
    })
    .into_any()
}

fn chunks_html(s: &Sentence, sc: Option<&SpkScore>) -> String {
    let parts: String =
        s.c.iter()
            .enumerate()
            .map(|(k, c)| {
                let r = sc.and_then(|sc| sc.res.get(k));
                format!(
                    r#"<span class="spk-c {}" {}>{}{}</span>"#,
                    r.map(|r| r.st).unwrap_or(""),
                    r.filter(|r| !r.why.is_empty()).map(|r| format!(r#"title="{}""#, esc(&r.why))).unwrap_or_default(),
                    ruby(&c.w, "always"),
                    esc(c.p.as_deref().unwrap_or(""))
                )
            })
            .collect();
    format!(r#"<div class="sent big spk-out">{}{}</div>"#, parts, esc(&crate::jp::end_of(s)))
}

fn skeleton(s: &Sentence) -> String {
    s.c.iter()
        .map(|c| {
            format!(
                r#"<span class="pill role-{}" style="color:var(--rc);border-color:var(--rc)">{}{}</span>"#,
                role(&c.r).0,
                esc(role(&c.r).1),
                c.p.as_ref().map(|p| format!(r#" <span class="jp">{}</span>"#, esc(p))).unwrap_or_default()
            )
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Ready,
    Listening,
    Finishing,
    Done,
}

#[component]
fn Speak(pids: Vec<String>, page: RwSignal<Page>, gen: RwSignal<u32>) -> impl IntoView {
    let c = data::content().unwrap_or_default();
    let pts = pool_points(&c, &pids);
    let bank: Vec<Sentence> =
        shuffle(&pts.iter().flat_map(|p| p.s.iter().cloned()).collect::<Vec<_>>()).into_iter().take(8).collect();
    if bank.is_empty() {
        return view! {
            <button class="btn ghost sm" style="align-self:flex-start" on:click=move |_| page.set(Page::Hub)>"← Frases"</button>
            <div class="empty">"Sem frases para praticar ainda. Estude alguns pontos na Jornada primeiro."</div>
        }
        .into_any();
    }
    spawn_local(async {
        let _ = data::need_vocab().await;
    });
    let total = bank.len();
    let bank = StoredValue::new(bank);
    let stt = RwSignal::new(SttStatus::default());
    spawn_local(async move {
        if let Ok(s) = tauri::invoke::<SttStatus, _>("stt_status", &NoArgs {}).await {
            stt.set(s);
        }
    });
    let saved_mode = store::read(|s| s.p.extra.get("speakMode").and_then(Value::as_str).unwrap_or("ler").to_string());
    let mode = RwSignal::new(saved_mode);
    let i = RwSignal::new(0usize);
    let right = RwSignal::new(0usize);
    let heard = RwSignal::new(String::new());
    let phase = RwSignal::new(Phase::Ready);
    let best = RwSignal::new(None::<SpkScore>);
    let self_ok = RwSignal::new(false);
    let hint = RwSignal::new(false);
    let revealed = RwSignal::new(false);
    let msg = RwSignal::new(String::new());
    let finished = RwSignal::new(false);
    let eff_mode = move || if stt.get().available { mode.get() } else { "sombra".to_string() };
    let cur = move || bank.with_value(|b| b[i.get().min(total - 1)].clone());

    let reset = move || {
        heard.set(String::new());
        best.set(None);
        self_ok.set(false);
        hint.set(false);
        revealed.set(false);
        msg.set(String::new());
        phase.set(Phase::Ready);
    };
    let evaluate = move |text: String| {
        let t = text.trim().to_string();
        heard.set(t.clone());
        if t.is_empty() {
            phase.set(Phase::Ready);
            msg.set("Não ouvi nada. Fale um pouco mais perto do microfone.".into());
            return;
        }
        let s = bank.with_value(|b| b[i.get_untracked()].clone());
        let sc = spk_score(&s, &t);
        if best.get_untracked().map(|b| sc.score >= b.score).unwrap_or(true) {
            best.set(Some(sc.clone()));
        }
        phase.set(Phase::Done);
        msg.set(String::new());
        if best.get_untracked().map(|b| b.pass).unwrap_or(false) {
            say(&sent_plain(&s), None);
        }
    };
    let mic = move |_| match phase.get_untracked() {
        Phase::Listening => {
            phase.set(Phase::Finishing);
            spawn_local(async move {
                match tauri::invoke::<String, _>("stt_stop", &NoArgs {}).await {
                    Ok(t) => evaluate(t),
                    Err(e) => {
                        phase.set(Phase::Ready);
                        msg.set(format!("O reconhecimento de voz falhou: {e}"));
                    }
                }
            });
        }
        Phase::Ready | Phase::Done => {
            crate::tts::stop();
            heard.set(String::new());
            msg.set(String::new());
            spawn_local(async move {
                match tauri::invoke::<(), _>("stt_start", &NoArgs {}).await {
                    Ok(()) => phase.set(Phase::Listening),
                    Err(e) => {
                        phase.set(Phase::Ready);
                        msg.set(format!("Não consegui abrir o microfone: {e}"));
                    }
                }
            });
        }
        _ => {}
    };
    let next = move || {
        let ok = best.get_untracked().map(|b| b.pass).unwrap_or(false) || self_ok.get_untracked();
        let s = bank.with_value(|b| b[i.get_untracked()].clone());
        if ok && !s.id.is_empty() {
            store::write(&["journey"], |st| {
                st.j.spoke.insert(s.id.clone(), now_ms());
            });
        }
        record_point(&s.pid, ok);
        if ok {
            right.update(|r| *r += 1);
        }
        if i.get_untracked() + 1 >= total {
            store::with_today(|d| d.games += 1.0);
            store::add_xp(5.0 + right.get_untracked() as f64 * 3.0);
            finished.set(true);
            return;
        }
        i.update(|x| *x += 1);
        reset();
    };
    on_cleanup(|| tauri::call("stt_cancel", NoArgs {}));

    view! {
        <div class="row-between">
            <button class="btn ghost sm" on:click=move |_| page.set(Page::Hub)>"← Frases"</button>
            <h2>"🎤 Falar"</h2>
            <span class="pill">{move || format!("{}/{} · ✔ {}", (i.get() + 1).min(total), total, right.get())}</span>
        </div>
        {move || {
            if finished.get() {
                let r = right.get_untracked();
                return view! {
                    <div class="card stack" style="text-align:center;align-items:center">
                        <h1>{format!("{r}/{total}")}</h1>
                        <p class="muted">{if r as f64 >= total as f64 * 0.8 { "Sua boca já acompanha a estrutura. 素晴らしい！" } else { "Falar é o último passo de uma frase: quanto mais você monta e ouve, mais fácil sai." }}</p>
                        <div class="row"><button class="btn" on:click=move |_| gen.update(|g| *g += 1)>"Outra rodada"</button><button class="btn primary" on:click=move |_| page.set(Page::Hub)>"Voltar"</button></div>
                    </div>
                }.into_any();
            }
            let m = eff_mode();
            let available = stt.get().available;
            let seg = [("ler", "Ler em voz alta"), ("traduzir", "Dizer em japonês"), ("sombra", "Sombra (sem microfone)")]
                .into_iter()
                .map(|(v, t)| {
                    let pressed = m == v;
                    view! { <button aria-pressed=pressed.to_string() disabled=v != "sombra" && !available on:click=move |_| {
                        mode.set(v.to_string());
                        if v != "sombra" { store::write(&["profile"], |s| { s.p.extra.insert("speakMode".into(), Value::String(v.into())); }); }
                        reset();
                    }>{t}</button> }
                })
                .collect_view();
            let s = cur();
            let ph = phase.get();
            let sc = best.get();
            let plain = sent_plain(&s);
            if m == "sombra" && !revealed.get_untracked() && ph == Phase::Ready {
                let p2 = plain.clone();
                set_timeout(move || say(&p2, None), std::time::Duration::from_millis(250));
            }
            let prompt = match m.as_str() {
                "traduzir" => {
                    let sk = skeleton(&s);
                    view! {
                        <div style="font-size:20px;font-weight:700;text-align:center">{format!("“{}”", s.en)}</div>
                        <div class="small muted" style="text-align:center">"Diga isso em japonês."</div>
                        <div style="text-align:center">
                            {move || if hint.get() { view! { <div class="row" style="justify-content:center;gap:6px" inner_html=sk.clone()></div> }.into_any() }
                                else { view! { <button class="btn sm ghost" on:click=move |_| hint.set(true)>"💡 Dica: mostrar a estrutura"</button> }.into_any() }}
                        </div>
                    }.into_any()
                }
                "sombra" => {
                    let html = chunks_html(&s, None);
                    let en = s.en.clone();
                    view! {
                        <div class="small muted" style="text-align:center">"Ouça, repita em voz alta junto com o áudio e depois confira."</div>
                        {move || if revealed.get() {
                            view! { <div inner_html=html.clone()></div><div class="en" style="text-align:center">{en.clone()}</div> }.into_any()
                        } else {
                            view! { <div style="text-align:center"><button class="btn sm ghost" on:click=move |_| revealed.set(true)>"Mostrar a frase"</button></div> }.into_any()
                        }}
                    }.into_any()
                }
                _ => view! { <div inner_html=chunks_html(&s, if ph == Phase::Done { sc.as_ref() } else { None })></div><div class="en" style="text-align:center">{s.en.clone()}</div> }.into_any(),
            };
            let controls = if m == "sombra" {
                view! {
                    <div class="row" style="justify-content:center">
                        {move || revealed.get().then(|| view! {
                            <button class="btn" on:click=move |_| { self_ok.set(false); best.set(Some(SpkScore { res: vec![], score: 0.0, pass: false })); next(); }>"😵 Não consegui"</button>
                            <button class="btn" on:click=move |_| { self_ok.set(false); best.set(Some(SpkScore { res: vec![], score: 0.5, pass: false })); next(); }>"😅 Travei"</button>
                            <button class="btn primary" on:click=move |_| { self_ok.set(true); best.set(Some(SpkScore { res: vec![], score: 1.0, pass: true })); next(); }>"😎 Saiu fácil"</button>
                        })}
                    </div>
                }.into_any()
            } else {
                let live = heard.get();
                view! {
                    <div class="spk-mic">
                        <button class="mic" class:on=ph == Phase::Listening disabled=ph == Phase::Finishing on:click=mic aria-label=if ph == Phase::Listening { "Terminei de falar" } else { "Falar" }>
                            {if ph == Phase::Listening { "■" } else { "🎤" }}
                        </button>
                        <div class="small" class:muted=ph != Phase::Listening aria-live="polite">{match ph {
                            Phase::Listening => "Ouvindo… toque de novo quando terminar.".to_string(),
                            Phase::Finishing => "Conferindo…".to_string(),
                            Phase::Done => String::new(),
                            Phase::Ready => if live.is_empty() { "Toque no microfone e fale. Toque de novo quando terminar.".to_string() } else { String::new() },
                        }}</div>
                    </div>
                }.into_any()
            };
            let result = (ph == Phase::Done).then(|| sc.clone()).flatten().map(|sc| {
                let ok = sc.pass || self_ok.get();
                let pct = (sc.score * 100.0).round();
                let line = if sc.pass { "Muito bem!" } else if self_ok.get() { "Contado como certo." } else if sc.score >= 0.5 { "Quase. Veja os pedaços em amarelo e vermelho." } else if m == "traduzir" { "Não bateu com a resposta modelo. Se você usou outra forma, marque \"Eu disse certo\"." } else { "Ainda não. Ouça o modelo e tente de novo." };
                let why: String = sc.res.iter().enumerate().filter(|(_, r)| !r.why.is_empty()).map(|(k, r)| format!(r#"<li><span class="jp spk-c {}">{}</span> {}</li>"#, r.st, esc(&chunk_text(&s.c[k])), esc(&r.why))).collect();
                let has_why = !why.is_empty();
                view! {
                    <div class=format!("banner {}", if ok { "ok" } else { "warn" })><span class="bico">{if ok { "⭕" } else { "△" }}</span>
                        <div class="grow"><b>{format!("{pct}% ")}</b>{line}<div class="small">"Ouvi: "<span class="jp">{format!("「{}」", if heard.get().is_empty() { "…".to_string() } else { heard.get() })}</span></div></div>
                    </div>
                    {(m == "traduzir").then(|| view! { <div inner_html=chunks_html(&s, Some(&sc))></div><div class="en">{s.en.clone()}</div> })}
                    {(m != "traduzir").then(|| view! { <div class="small muted">"Marquei na frase acima o que ouvi de cada pedaço."</div> })}
                    <div class="tiny muted row" style="gap:10px"><span class="spk-c ok">"certo"</span><span class="spk-c part">"quase / partícula"</span><span class="spk-c order">"fora de ordem"</span><span class="spk-c miss">"não ouvi"</span></div>
                    {has_why.then(|| view! { <ul class="small spk-why" inner_html=why></ul> })}
                }
            });
            view! {
                <div class="seg" role="group" aria-label="Tipo de treino">{seg}</div>
                <div class="card stack">
                    {prompt}
                    <div class="row" style="justify-content:center">
                        <span inner_html=spk(&plain, "btn sm")></span>
                        <button class="btn sm ghost" on:click=move |_| say(&sent_plain(&cur()), Some(0.6))>"🐢 devagar"</button>
                    </div>
                    {controls}
                    {move || (!msg.get().is_empty()).then(|| view! { <div class="small" style="color:var(--warn);text-align:center">{msg.get()}</div> })}
                    {result}
                    {(ph == Phase::Done).then(|| view! {
                        <div class="row" style="justify-content:center">
                            {(!(best.get_untracked().map(|b| b.pass).unwrap_or(false)) && !self_ok.get_untracked()).then(|| view! {
                                <button class="btn sm ghost" on:click=move |_| self_ok.set(true)>"✓ Eu disse certo"</button>
                            })}
                            <button class="btn sm" on:click=move |_| { heard.set(String::new()); msg.set(String::new()); phase.set(Phase::Ready); }>"🔁 Tentar de novo"</button>
                            <button class="btn sm primary" on:click=move |_| next()>"Próxima →"</button>
                        </div>
                    })}
                </div>
                {(!available).then(|| model_download(stt))}
            }
            .into_any()
        }}
    }
    .into_any()
}
