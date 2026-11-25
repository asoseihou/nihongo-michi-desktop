//! Imersão (port of `16-imersao.js`): vlog of the day, immersion log,
//! last 7 days chart and the verified resource lists.

use crate::app::{take_intent, toast};
use crate::data;
use crate::docs::MinedItem;
use crate::logic::vlog_of_day;
use crate::store::{self, sigs};
use crate::util::{clamp, esc, ev_value, safe_url, yt_search};
use crate::views::kanji::open_mine;
use leptos::prelude::*;
use michi_core::docs::ImmEntry;
use michi_core::time::{dow, key_add, now_ms, parse_key, today_key, WD_PT};
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;

/// (key, icon, label, colour)
const IMM: [(&str, &str, &str, &str); 7] = [
    ("vlog", "📺", "Vlog em japonês", "var(--line)"),
    ("podcast", "🎧", "Podcast", "var(--n3)"),
    ("wagotabi", "🎮", "Wagotabi", "var(--kana)"),
    ("anime", "🎬", "Anime / drama", "var(--n2)"),
    ("reading", "📖", "Leitura", "var(--n5)"),
    ("aula", "🧑‍🏫", "Vídeo-aula", "var(--n4)"),
    ("outro", "✨", "Outro", "var(--ink-3)"),
];
const RES_TYPES: [(&str, &str); 6] = [
    ("vlog", "Vlogs em japonês"),
    ("channel", "Canais de aula"),
    ("podcast", "Podcasts"),
    ("site", "Sites"),
    ("tool", "Ferramentas"),
    ("deck", "Anki / decks"),
];

fn imm(t: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    IMM.iter().find(|x| x.0 == t).copied().unwrap_or(IMM[6])
}

/// `logImmersion(t, min, title)`: counts for the heat map and the routine.
pub fn log_immersion(t: &str, min: f64, title: &str) {
    let e = ImmEntry { t: t.into(), min, title: title.into(), note: String::new(), at: now_ms(), ..Default::default() };
    store::with_today(|d| d.imm.push(e));
    store::add_xp(min.round().min(30.0));
}

#[component]
pub fn Imersao() -> impl IntoView {
    let s = sigs();
    let intent = take_intent();
    let log_pref = intent.as_ref().and_then(|i| i.get("log").and_then(Value::as_str)).map(str::to_string);
    let tab =
        RwSignal::new(intent.as_ref().and_then(|i| i.get("tab").and_then(Value::as_str)).unwrap_or("hoje").to_string());
    let filt = RwSignal::new("all".to_string());
    let ready = RwSignal::new(data::content().is_some());
    spawn_local(async move {
        let _ = data::need_content().await;
        ready.set(true);
    });
    let tabs = std::iter::once(("hoje", "Hoje"))
        .chain(RES_TYPES)
        .map(|(t, l)| view! { <button role="tab" aria-selected=move || (tab.get() == t).to_string() on:click=move |_| tab.set(t.to_string())>{l}</button> })
        .collect_view();
    view! {
        <div class="row-between"><h1>"Imersão"</h1><div class="small muted">"Japonês de verdade, todo dia, mesmo sem entender tudo."</div></div>
        <div class="tabs" role="tablist">{tabs}</div>
        {move || {
            s.view.get();
            if !ready.get() {
                return view! { <div class="loading"><div class="spin"></div></div> }.into_any();
            }
            let t = tab.get();
            if t == "hoje" { today_tab(log_pref.clone()) } else { resources(&t, filt) }
        }}
    }
}

fn today_tab(log_pref: Option<String>) -> AnyView {
    let c = data::content().unwrap_or_default();
    let v = vlog_of_day(&c);
    let d = store::today();
    let vlog_time = store::read(|s| if s.p.vlog_time.is_empty() { "21:30".to_string() } else { s.p.vlog_time.clone() });
    let last7: Vec<String> = (0..7).map(|i| key_add(&today_key(), i - 6)).collect();
    let per: Vec<Vec<(String, f64)>> = last7
        .iter()
        .map(|k| {
            let mut m: Vec<(String, f64)> = Vec::new();
            if let Some(dd) = store::day(k) {
                for e in &dd.imm {
                    match m.iter_mut().find(|x| x.0 == e.t) {
                        Some(x) => x.1 += e.min,
                        None => m.push((e.t.clone(), e.min)),
                    }
                }
            }
            m
        })
        .collect();
    let max = per.iter().map(|m| m.iter().map(|x| x.1).sum::<f64>()).fold(30.0, f64::max);
    let total: f64 = per.iter().flat_map(|m| m.iter().map(|x| x.1)).sum();
    let kind = RwSignal::new(log_pref.clone().unwrap_or_else(|| "vlog".into()));
    let mins = RwSignal::new("15".to_string());
    let what = RwSignal::new(String::new());
    let placeholder = if log_pref.as_deref() == Some("wagotabi") {
        "ex.: Okayama · Kōrakuen, lição de から/ので"
    } else {
        "ex.: vlog de um dia em Tóquio"
    };
    let opts = IMM
        .iter()
        .map(|(k, i, l, _)| {
            let sel = kind.get_untracked() == *k;
            view! { <option value=*k selected=sel>{format!("{i} {l}")}</option> }
        })
        .collect_view();
    let quick = [10, 15, 20, 30, 45]
        .into_iter()
        .map(|n| view! { <button class="btn xs" on:click=move |_| mins.set(n.to_string())>{n}</button> })
        .collect_view();
    let spark: String = per
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let bars: String = m
                .iter()
                .map(|(t, n)| {
                    format!(r#"<i style="height:{}px;background:{}"></i>"#, (n / max * 70.0).round(), imm(t).3)
                })
                .collect();
            let wd = parse_key(&last7[i]).map(|d| WD_PT[dow(d) as usize]).unwrap_or("");
            format!(r#"<div class="col" title="{}">{}<span>{}</span></div>"#, last7[i], bars, wd)
        })
        .collect();
    let legend: String = IMM.iter().map(|(_, _, l, c)| format!(r#"<span style="--rc:{c}">{l}</span>"#)).collect();
    let today_rows = if d.imm.is_empty() {
        view! { <div class="small muted">"Nada registrado ainda hoje."</div> }.into_any()
    } else {
        d.imm
            .iter()
            .map(|e| {
                let x = imm(&e.t);
                view! { <div class="row small"><span>{x.1}</span><b>{format!("{} min", e.min)}</b><span class="muted">{if e.title.is_empty() { x.2.to_string() } else { e.title.clone() }}</span></div> }
            })
            .collect_view()
            .into_any()
    };
    view! {
        {v.map(|v| view! {
            <div class="card stack" style="border-left:5px solid var(--line)">
                <div class="eyebrow">{format!("Vlog do dia · lembrete às {vlog_time}")}</div>
                <div class="row-between"><div><h2>{v.name.clone()}</h2><div class="small muted">{v.best_for.clone()}</div></div>
                    <a class="btn primary" href=safe_url(&v.url) target="_blank" rel="noopener">"Abrir no YouTube ↗"</a></div>
                <div class="small muted">"Dica: assista com legenda em japonês (ou nenhuma). Não pause para tudo; anote 1–2 palavras para minerar."</div>
            </div>
        })}
        <div class="card stack"><h2>"Registrar imersão"</h2>
            <div class="grid2">
                <div class="field"><label>"Tipo"</label><select on:change=move |ev| kind.set(ev_value(&ev))>{opts}</select></div>
                <div class="field"><label>"Minutos"</label><div class="row">
                    <input type="number" min="1" max="300" style="width:90px" prop:value=move || mins.get() on:input=move |ev| mins.set(ev_value(&ev))/>{quick}</div></div>
            </div>
            <div class="field"><label>"O quê? (canal, episódio, onde parou no Wagotabi)"</label>
                <input type="text" placeholder=placeholder prop:value=move || what.get() on:input=move |ev| what.set(ev_value(&ev))/></div>
            <div class="row">
                <button class="btn primary" on:click=move |_| {
                    let t = kind.get_untracked();
                    let m = clamp(mins.get_untracked().parse::<f64>().unwrap_or(0.0).floor(), 1.0, 300.0);
                    log_immersion(&t, m, what.get_untracked().trim());
                    toast(&format!("{m} min de {} registrados.", imm(&t).2), "ok");
                    store::refresh();
                }>"＋ Registrar"</button>
                <button class="btn" on:click=move |_| {
                    let src = if kind.get_untracked() == "wagotabi" { "Wagotabi" } else { "Vlog" };
                    open_mine(MinedItem { source: src.into(), ..MinedItem::default() }, None);
                }>"⛏️ Minerar uma palavra"</button>
            </div>
        </div>
        <div class="card stack"><div class="row-between"><h2>"Últimos 7 dias"</h2><span class="pill line">{format!("{total} min")}</span></div>
            <div class="spark" inner_html=spark></div>
            <div class="legend" inner_html=legend></div>
        </div>
        <div class="card stack"><h2>"Hoje"</h2>{today_rows}</div>
    }
    .into_any()
}

fn resources(t: &str, filt: RwSignal<String>) -> AnyView {
    let c = data::content().unwrap_or_default();
    let f = filt.get();
    let list: Vec<data::Resource> =
        c.resources.items.iter().filter(|x| x.kind == t && (f == "all" || x.level.contains(&f))).cloned().collect();
    let seg = [("all", "Todos"), ("kana", "Kana"), ("N5", "N5"), ("N4", "N4"), ("N3", "N3+")]
        .into_iter()
        .map(|(v, l)| view! { <button aria-pressed=move || (filt.get() == v).to_string() on:click=move |_| filt.set(v.to_string())>{l}</button> })
        .collect_view();
    let cards = list
        .into_iter()
        .map(|x| {
            let searches: String = x
                .searches
                .iter()
                .take(3)
                .map(|q| format!(r#"<a class="btn xs" href="{}" target="_blank" rel="noopener">▶ {}</a>"#, esc(&yt_search(q)), esc(&q.chars().take(38).collect::<String>())))
                .collect();
            let watch = matches!(x.kind.as_str(), "vlog" | "podcast" | "channel").then(|| {
                let kind = match x.kind.as_str() {
                    "podcast" => "podcast",
                    "vlog" => "vlog",
                    _ => "aula",
                };
                let name = x.name.clone();
                view! { <button class="btn xs" on:click=move |_| { log_immersion(kind, 15.0, &name); toast("15 min registrados. お疲れ！", "ok"); }>"✓ Assisti 15 min"</button> }
            });
            view! {
                <div class="res">
                    <div style="min-width:0">
                        <div class="rn">{x.name.clone()}</div><div class="small muted">{x.best_for.clone()}</div>
                        <div class="row tiny" style="margin-top:4px;gap:6px">
                            {(!x.lang.is_empty()).then(|| view! { <span class="pill">{x.lang.clone()}</span> })}
                            {(!x.level.is_empty()).then(|| view! { <span class="pill">{x.level.clone()}</span> })}
                        </div>
                        {(!searches.is_empty()).then(|| view! { <div class="row" style="gap:6px;margin-top:6px" inner_html=searches></div> })}
                    </div>
                    <div class="stack" style="gap:6px;align-items:flex-end">
                        <a class="btn sm" href=safe_url(&x.url) target="_blank" rel="noopener">"Abrir ↗"</a>
                        {watch}
                    </div>
                </div>
            }
        })
        .collect::<Vec<_>>();
    let empty = cards.is_empty();
    view! {
        <div class="row"><div class="seg">{seg}</div></div>
        {if empty {
            view! { <div class="empty">"A lista verificada de canais e sites ainda está sendo montada. Volte depois de atualizar o app."</div> }.into_any()
        } else {
            view! { <div class="grid2">{cards}</div> }.into_any()
        }}
    }
    .into_any()
}
