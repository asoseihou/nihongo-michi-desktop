//! Jornada (port of `11-journey.js` + `11b-trip.js`): the line map, the
//! Wagotabi line, the trip line, the stamp book, the station page, the boss
//! and the sign quiz.

use crate::app::{bar, go, take_intent, toast, SENSEI_ON};
use crate::data::{self, Content, Station};
use crate::jp::{chunk_text, kana_j, kana_set, plain_j, role, role_legend, ruby, sent_html, sent_plain, spk};
use crate::logic::{
    city_ready, code_badge, current_station, line, main_line, set_position, station_kanji, station_points,
    station_progress, station_ready, trip_countdown, trip_dates, trip_headline, trip_range, trip_ready, trip_stations,
};
use crate::store::{self, sigs};
use crate::util::{esc, ev_checked, ev_value, pick, safe_url, shuffle, yt_search};
use crate::views::lesson::{open_lesson, point_card};
use crate::views::quiz::{open_quiz, result_buttons, Mc};
use leptos::prelude::*;
use michi_core::time::{local, now_ms};
use michi_core::trip::{city_jp, CITIES};
use serde_json::json;
use std::sync::Arc;
use wasm_bindgen_futures::spawn_local;

#[derive(Clone, PartialEq)]
enum Page {
    Map(String),
    Station(String),
}

#[component]
pub fn Jornada() -> impl IntoView {
    let s = sigs();
    let intent = take_intent();
    let start = match intent.as_ref() {
        Some(i) if i.get("station").is_some() => Page::Station(i["station"].as_str().unwrap_or("").to_string()),
        Some(i) if i.get("wline").is_some() => Page::Map("wago".into()),
        Some(i) if i.get("tab").is_some() => Page::Map(i["tab"].as_str().unwrap_or("main").to_string()),
        _ => Page::Map("main".into()),
    };
    let page = RwSignal::new(start);
    let ready = RwSignal::new(data::content().is_some() && data::kanji().is_some());
    if !ready.get_untracked() {
        spawn_local(async move {
            if let Err(e) = data::need_content().await {
                toast(&format!("Falha ao carregar dados: {e}"), "bad");
            }
            let _ = data::need_kanji().await;
            ready.set(true);
        });
    }
    let rev = RwSignal::new(0u64);
    move || {
        s.view.get();
        rev.get();
        if !ready.get() {
            return view! { <div class="loading"><div class="spin"></div>"Carregando o mapa…"</div> }.into_any();
        }
        let c = data::content().unwrap_or_default();
        match page.get() {
            Page::Map(tab) => map_view(&c, page, &tab),
            Page::Station(id) => {
                crate::util::scroll_top();
                station_view(&c, page, rev, &id)
            }
        }
    }
}

/* ---------------- stamp ---------------- */

/// `stampSVG(st, got)`: an eki-stamp with the station code and name.
pub fn stamp_svg(st: &Station, got: Option<f64>) -> String {
    let c = if got.is_some() { line(&st.line).lc } else { "var(--rule-2)" };
    let rot = (st.id.chars().last().map(|c| c as i32).unwrap_or(0) % 9) - 4;
    let jp: String = st.jp.chars().take(9).collect();
    let fs = if st.jp.chars().count() > 6 { 11 } else { 15 };
    let date = got.map(|t| local(t).format("%d/%m/%Y").to_string()).unwrap_or_else(|| "未".into());
    format!(
        r#"<svg viewBox="0 0 120 120" width="100%" role="img" aria-label="Selo {id}" style="transform:rotate({rot}deg)">
<circle cx="60" cy="60" r="54" fill="none" stroke="{c}" stroke-width="5"/><circle cx="60" cy="60" r="45" fill="none" stroke="{c}" stroke-width="1.5" stroke-dasharray="3 3"/>
<text x="60" y="44" text-anchor="middle" font-family="Barlow Condensed, sans-serif" font-weight="700" font-size="15" fill="{c}">{id}</text>
<text x="60" y="70" text-anchor="middle" font-family="Klee One, serif" font-weight="600" font-size="{fs}" fill="{c}">{jp}</text>
<text x="60" y="92" text-anchor="middle" font-family="Barlow Condensed, sans-serif" font-size="10" letter-spacing="2" fill="{c}">{date}</text></svg>"#,
        id = esc(&st.id),
        jp = esc(&jp),
    )
}

/* ---------------- map ---------------- */

fn station_row(st: &Station, page: RwSignal<Page>, right: String, cur: bool, done: bool, extra: String) -> AnyView {
    let id = st.id.clone();
    let sub = format!("{}{}", esc(&st.jp), extra);
    view! {
        <button class="station" class:done=done class:cur=cur on:click=move |_| page.set(Page::Station(id.clone()))>
            <span inner_html=code_badge(st, false)></span>
            <div style="min-width:0"><div class="sn">{st.name.clone()}{st.talk.then_some(" 🗣️")}</div><div class="sj" inner_html=sub></div></div>
            <div class="sb" inner_html=right></div>
        </button>
    }
    .into_any()
}

fn position_picker(c: &Content) -> AnyView {
    let cur = current_station(c).map(|s| s.id.clone()).unwrap_or_default();
    let sel = RwSignal::new(cur.clone());
    let mark = RwSignal::new(false);
    let opts = main_line(c)
        .into_iter()
        .map(|s| {
            let selected = s.id == cur;
            view! { <option value=s.id.clone() selected=selected>{format!("{} · {}", s.id, s.name)}</option> }
        })
        .collect_view();
    view! {
        <div class="row">
            <select aria-label="Estação atual" on:change=move |ev| sel.set(ev_value(&ev))>{opts}</select>
            <label class="switch small"><input type="checkbox" on:change=move |ev| mark.set(ev_checked(&ev))/>" marcar kanji anteriores como já vistos"</label>
            <button class="btn sm primary" on:click=move |_| {
                if let Some(msg) = set_position(&sel.get_untracked(), mark.get_untracked()) { toast(&msg, "ok"); }
                store::refresh();
            }>"Definir"</button>
        </div>
    }
    .into_any()
}

fn map_view(c: &Content, page: RwSignal<Page>, tab: &str) -> AnyView {
    let (ts, _) = trip_dates();
    let tabs = [("main", "Linha principal".to_string()), ("wago", "Linha Wagotabi".into()), ("trip", format!("Viagem {}", &ts[..4.min(ts.len())])), ("stamps", "Selos 🎴".into())]
        .into_iter()
        .map(|(t, l)| {
            let sel = t == tab;
            view! { <button role="tab" aria-selected=sel.to_string() on:click=move |_| page.set(Page::Map(t.to_string()))>{l}</button> }
        })
        .collect_view();
    let body = match tab {
        "wago" => wago_tab(c),
        "trip" => trip_tab(c, page),
        "stamps" => stamps_tab(c, page),
        _ => main_tab(c, page),
    };
    view! {
        <div class="row-between"><h1>"Jornada"</h1><div class="small muted">"Avance por domínio, não por calendário. Qualquer estação pode ser aberta no modo livre."</div></div>
        <div class="tabs" role="tablist">{tabs}</div>
        <div>{body}</div>
    }
    .into_any()
}

fn main_tab(c: &Content, page: RwSignal<Page>) -> AnyView {
    let cur = current_station(c).cloned();
    let onboarded = store::read(|s| s.p.onboarded);
    let done = store::read(|s| s.j.done.clone());
    let lines = ["K", "N5", "N4", "N3", "N2", "N1"]
        .into_iter()
        .map(|l| {
            let sts: Vec<&Station> = c.stations.iter().filter(|s| s.line == l).collect();
            let ln = line(l);
            let done_n = sts.iter().filter(|s| done.contains_key(&s.id)).count();
            let open = cur.as_ref().map(|s| s.line == l).unwrap_or(false);
            let rows = sts
                .iter()
                .map(|s| {
                    let pr = station_progress(c, s);
                    let is_cur = cur.as_ref().map(|x| x.id == s.id).unwrap_or(false);
                    let right = if pr.done {
                        r#"<span class="pill ok">✓ 済</span>"#.to_string()
                    } else if is_cur {
                        r#"<span class="pill line">▶ atual</span>"#.to_string()
                    } else {
                        bar(pr.frac, ln.lc)
                    };
                    let extra = s
                        .genki
                        .as_ref()
                        .filter(|g| !g.is_empty())
                        .map(|g| format!(" · {}", esc(g)))
                        .unwrap_or_default();
                    station_row(s, page, right, is_cur, pr.done, extra)
                })
                .collect_view();
            view! {
                <details class="card flat" open=open style=format!("--lc:{}", ln.lc)>
                    <summary class="linehead" style="cursor:pointer">
                        <span class="sw"></span>
                        <div class="grow"><h2>{ln.name}</h2><div class="jp small muted">{ln.jp}</div></div>
                        <span class="pill">{format!("{}/{}", done_n, sts.len())}</span>
                    </summary>
                    <div class="stations" style="margin-top:10px">{rows}</div>
                </details>
            }
        })
        .collect_view();
    view! {
        <details class="card tint" style="margin-bottom:14px" open=!onboarded>
            <summary style="cursor:pointer;font-weight:700">"📍 Minha posição na linha"</summary>
            <div class="stack" style="margin-top:10px">
                <div class="small muted">"Já estudou parte do caminho (kana, algumas lições do Genki)? Coloque o trem na estação onde você está. As anteriores ficam marcadas como concluídas — o selo 🎴 continua sendo só por chefão vencido."</div>
                {position_picker(c)}
            </div>
        </details>
        <div class="lines">{lines}</div>
    }
    .into_any()
}

fn lesson_buttons(c: &Content, ids: &[String], title_of: impl Fn(&crate::data::Lesson) -> String) -> AnyView {
    ids.iter()
        .filter_map(|l| c.lesson(l).map(|x| (l.clone(), title_of(x))))
        .map(|(lid, t)| view! { <button class="btn sm" on:click=move |_| open_lesson(&lid)>{t}</button> })
        .collect_view()
        .into_any()
}

fn wago_tab(c: &Content) -> AnyView {
    let w = &c.wagotabi;
    let mine = store::read(|s| s.p.wagotabi.clone());
    let mut prefs: Vec<String> = Vec::new();
    for a in &w.areas {
        if !prefs.contains(&a.pref) {
            prefs.push(a.pref.clone());
        }
    }
    let groups = prefs
        .into_iter()
        .map(|pf| {
            let cards = w
                .areas
                .iter()
                .filter(|a| a.pref == pf)
                .map(|a| {
                    let here = a.id == mine;
                    let id = a.id.clone();
                    let lessons: Vec<String> = a.lessons.iter().filter(|l| c.lesson(l).is_some()).cloned().collect();
                    let has = !lessons.is_empty();
                    view! {
                        <div class="card stack" style=if here { "border-color:var(--line);box-shadow:0 0 0 2px var(--line-soft)" } else { "" }>
                            <div class="row-between">
                                <div><h3>{a.name.clone()}" "<span class="jp muted">{a.jp.clone()}</span></h3>
                                    {(!a.verified).then(|| view! { <span class="tiny faint">"ordem exata não documentada"</span> })}</div>
                                <button class="btn sm" class:primary=here on:click=move |_| {
                                    store::write(&["profile"], |s| s.p.wagotabi = id.clone());
                                    toast("Anotado! O app vai sugerir reforços dessa área.", "ok");
                                    store::refresh();
                                }>{if here { "📍 Estou aqui" } else { "Estou aqui" }}</button>
                            </div>
                            <div class="small"><b>"No jogo: "</b>{format!("{}.", a.topics.join("; "))}</div>
                            <div class="small"><b>"Reforce no app:"</b></div>
                            <div class="row" style="gap:6px">
                                {if has { lesson_buttons(c, &lessons, |l| l.title.clone()) } else { view! { <span class="small muted">"conteúdo ainda sendo preparado"</span> }.into_any() }}
                            </div>
                            <div class="tiny faint">{format!("Treino sugerido: {}", a.drills.join(" · "))}</div>
                        </div>
                    }
                })
                .collect_view();
            view! { <div class="stack"><div class="eyebrow">{pf}</div>{cards}</div> }
        })
        .collect_view();
    let anywhere = w.anywhere.as_ref().map(|a| {
        let btns = a
            .items
            .iter()
            .filter(|x| c.lesson(&x.1).is_some())
            .map(|x| {
                let lid = x.1.clone();
                view! { <button class="btn sm" on:click=move |_| open_lesson(&lid)>{x.0.clone()}</button> }
            })
            .collect_view();
        view! { <div class="card tint stack"><div class="eyebrow">"Ao longo do jogo"</div><div class="small">{a.text.clone()}</div><div class="row" style="gap:6px">{btns}</div></div> }
    });
    view! {
        <div class="stack">
            <div class="banner line"><span class="bico">"🎮"</span><div class="grow small">{w.note.clone()}</div></div>
            {groups}
            {anywhere}
        </div>
    }
    .into_any()
}

fn trip_tab(c: &Content, page: RwSignal<Page>) -> AnyView {
    let t = trip_stations(c);
    let cd = trip_countdown();
    let overall = trip_ready(c);
    let done = store::read(|s| s.j.done.clone());
    let cities: Vec<&str> =
        CITIES.iter().copied().filter(|ci| t.iter().any(|s| s.city.as_deref() == Some(*ci))).collect();
    let city_bars = cities
        .iter()
        .map(|ci| {
            let r = city_ready(c, ci);
            view! {
                <div>
                    <div class="row-between small"><b><span class="jp">{city_jp(ci)}</span>" "{ci.to_string()}</b><span class="num">{format!("{}%", (r * 100.0).round())}</span></div>
                    <div inner_html=bar(r, "var(--trip)")></div>
                </div>
            }
        })
        .collect_view();
    let city_lists = cities
        .iter()
        .map(|ci| {
            let rows = t
                .iter()
                .filter(|s| s.city.as_deref() == Some(*ci))
                .map(|s| {
                    let r = station_ready(c, s);
                    let d = done.contains_key(&s.id);
                    let right = if d {
                        r#"<span class="pill ok">✓ 済</span>"#.to_string()
                    } else {
                        bar(r.frac, "var(--trip)")
                    };
                    let extra = String::new();
                    let mut st = (*s).clone();
                    st.jp = format!("{} · {}", s.days.clone().unwrap_or_default(), s.jp);
                    station_row(&st, page, right, false, d, extra)
                })
                .collect_view();
            view! {
                <div class="stack">
                    <div class="eyebrow"><span class="jp">{city_jp(ci)}</span>" "{ci.to_string()}</div>
                    <div class="stations" style="--lc:var(--trip)">{rows}</div>
                </div>
            }
        })
        .collect_view();
    let v_rows = c
        .stations
        .iter()
        .filter(|s| s.line == "V")
        .map(|s| {
            let pr = station_progress(c, s);
            let right =
                if pr.done { r#"<span class="pill ok">✓</span>"#.to_string() } else { bar(pr.frac, "var(--trip)") };
            station_row(s, page, right, false, pr.done, String::new())
        })
        .collect_view();
    let has_cities = !cities.is_empty();
    view! {
        <div class="stack">
            <div class="card stack" style="border-top:6px solid var(--trip)">
                <div class="row-between">
                    <div><div class="eyebrow">{format!("✈️ Japão · {} · {} dias", trip_range(), cd.total)}</div><h2 inner_html=trip_headline(&cd)></h2></div>
                    <div class="trip-pct"><b class="num">{format!("{}%", (overall * 100.0).round())}</b><span class="tiny muted">"pronto"</span></div>
                </div>
                {has_cities.then(|| view! { <div class="trip-cities">{city_bars}</div> })}
                <div class="tiny faint">"Prontidão = gramática estudada + chefão + frases que você já falou no 🎤 + kanji de placa lidos. As datas da viagem ficam em Ajustes."</div>
            </div>
            {if has_cities { city_lists.into_any() } else { view! { <div class="empty">"O roteiro da viagem ainda está sendo preparado."</div> }.into_any() }}
            <details class="card flat">
                <summary style="cursor:pointer;font-weight:700">"Situações básicas · V01–V08"</summary>
                <div class="small muted" style="margin:8px 0">"A base que serve em qualquer parada: aeroporto, trem, hotel, restaurante, konbini, direções, farmácia, templos e onsen."</div>
                <div class="stations" style="--lc:var(--trip)">{v_rows}</div>
            </details>
            <div class="tiny faint">"🗣️ = parada onde falar japonês faz mais diferença: balcão, cardápio só em japonês, conversa com locais."</div>
        </div>
    }
    .into_any()
}

fn stamps_tab(c: &Content, page: RwSignal<Page>) -> AnyView {
    let stamps = store::read(|s| s.j.stamps.clone());
    let got = c.stations.iter().filter(|s| stamps.contains_key(&s.id)).count();
    let items = c
        .stations
        .iter()
        .map(|s| {
            let id = s.id.clone();
            let svg = stamp_svg(s, stamps.get(&s.id).copied());
            view! { <button class="station" style="display:block;padding:4px" title=s.name.clone() on:click=move |_| page.set(Page::Station(id.clone())) inner_html=svg></button> }
        })
        .collect_view();
    view! {
        <div class="stack">
            <p class="muted">"Cada estação concluída (chefão com 80%+) carimba um selo, como os "<span class="jp">"駅スタンプ"</span>{format!(" das estações de trem do Japão. {}/{} coletados.", got, c.stations.len())}</p>
            <div style="display:grid;grid-template-columns:repeat(auto-fill,minmax(96px,1fr));gap:12px">{items}</div>
        </div>
    }
    .into_any()
}

/* ---------------- station page ---------------- */

fn resource_links(c: &Content, line_id: &str) -> String {
    let lvl = match line_id {
        "K" => "kana",
        "N5" | "V" | "T" => "N5",
        "N4" => "N4",
        _ => "N3+",
    };
    let items: Vec<String> = c
        .resources
        .items
        .iter()
        .filter(|x| (x.kind == "site" || x.kind == "tool") && x.level.contains(lvl))
        .take(6)
        .map(|x| {
            format!(
                r#"<a class="res" href="{}" target="_blank" rel="noopener" style="text-decoration:none;color:inherit"><div><div class="rn">{}</div><div class="small muted">{}</div></div><span class="pill">{}</span></a>"#,
                esc(&safe_url(&x.url)),
                esc(&x.name),
                esc(&x.best_for),
                esc(&x.kind)
            )
        })
        .collect();
    if items.is_empty() {
        String::new()
    } else {
        format!(r#"<div class="stack">{}</div>"#, items.concat())
    }
}

fn station_view(c: &Content, page: RwSignal<Page>, rev: RwSignal<u64>, id: &str) -> AnyView {
    let Some(st) = c.station(id).cloned() else {
        return view! { <div class="empty">"Estação não encontrada."</div> }.into_any();
    };
    let ln = line(&st.line);
    let pr = station_progress(c, &st);
    let pts = station_points(c, &st);
    let pids: Vec<String> = pts.iter().map(|p| p.id.clone()).collect();
    let ks = station_kanji(&st);
    let kn = st.kana.as_deref().map(kana_set).unwrap_or_default();
    let vocab: Vec<(String, String)> =
        st.lessons.iter().filter_map(|l| c.lesson(l)).flat_map(|l| l.vocab.clone()).collect();
    let wg: Vec<String> = c
        .wagotabi
        .areas
        .iter()
        .filter(|a| a.lessons.iter().any(|l| st.lessons.contains(l)))
        .map(|a| format!("{} · {}", a.pref, a.name))
        .collect();
    let stamp = store::read(|s| s.j.stamps.get(&st.id).copied());
    let back_tab = if st.line == "V" || st.line == "T" { "trip" } else { "main" };

    let mut pills = Vec::new();
    if pr.n_kana > 0 {
        pills.push((
            pr.ka.unwrap_or(0.0) >= 1.0,
            format!("Kana firmes {}/{}", (pr.ka.unwrap_or(0.0) * pr.n_kana as f64).round(), pr.n_kana),
        ));
    }
    if pr.n_pts > 0 {
        pills.push((
            pr.g.unwrap_or(0.0) >= 1.0,
            format!("Gramática {}/{}", (pr.g.unwrap_or(0.0) * pr.n_pts as f64).round(), pr.n_pts),
        ));
    }
    if pr.n_k > 0 {
        pills.push((
            pr.k.unwrap_or(0.0) >= 1.0,
            format!("Kanji iniciados {}/{}", (pr.k.unwrap_or(0.0) * pr.n_k as f64).round(), pr.n_k),
        ));
    }
    pills.push((
        pr.best >= 80.0,
        format!("Chefão {} (meta 80%)", if pr.best > 0.0 { format!("{}%", pr.best) } else { "—".into() }),
    ));
    let pills = pills.into_iter().map(|(ok, t)| view! { <span class="pill" class:ok=ok>{t}</span> }).collect_view();

    let st_boss = st.clone();
    let pids_b = pids.clone();
    let pids_p = pids.clone();
    let has_pts = !pts.is_empty();
    let header = view! {
        <button class="btn ghost sm" style="align-self:flex-start" on:click=move |_| page.set(Page::Map(back_tab.to_string()))>"← Mapa"</button>
        <div class="card stack" style=format!("border-top:6px solid {}", ln.lc)>
            <div class="row">
                <span inner_html=code_badge(&st, false)></span>
                <div class="grow"><div class="eyebrow">{ln.name}</div><h1>{st.name.clone()}</h1>
                    <div class="jp muted">{format!("{}{}", st.jp, st.genki.as_ref().filter(|g| !g.is_empty()).map(|g| format!(" · {g}")).unwrap_or_default())}</div></div>
                <div style="width:96px" inner_html=stamp_svg(&st, stamp)></div>
            </div>
            <div inner_html=bar(pr.frac, ln.lc)></div>
            <div class="row small">{pills}</div>
            {(!wg.is_empty()).then(|| view! { <div class="small muted">{format!("🎮 No Wagotabi isto aparece em: {}.", wg.join("; "))}</div> })}
            <div class="row">
                <button class="btn primary" on:click=move |_| boss_quiz(&st_boss, rev)>"⚔️ Enfrentar o chefão"</button>
                {has_pts.then(|| view! {
                    <button class="btn" on:click=move |_| go("frases", Some(json!({"mode": "builder", "pids": pids_b.clone()})))>"🧩 Montar frases"</button>
                    <button class="btn" on:click=move |_| go("frases", Some(json!({"mode": "particles", "pids": pids_p.clone()})))>"を Partículas"</button>
                })}
            </div>
        </div>
    };

    let trip = (st.line == "T").then(|| trip_block(c, &st, page, rev, pids.clone()));

    let kana_card = (!kn.is_empty()).then(|| {
        let cells: String = store::read(|s| {
            kn.iter()
                .map(|x| {
                    let sv = s.ks.strength(&x.k);
                    let cls = if !s.ks.s.contains_key(&x.k) {
                        ""
                    } else if sv >= 0.8 {
                        "w3"
                    } else if sv >= 0.5 {
                        "w2"
                    } else {
                        "w1"
                    };
                    format!(r#"<div class="kcell {cls}" data-say="{k}"><div class="kc">{k}</div><div class="kr">{r}</div></div>"#, k = esc(&x.k), r = esc(&x.r))
                })
                .collect()
        });
        let spec = st.kana.clone().unwrap_or_default();
        let game = move |g: &'static str| {
            let spec = spec.clone();
            move |_| go("jogos", Some(json!({"game": g, "kana": spec.clone()})))
        };
        view! {
            <div class="card stack">
                <div class="row-between"><h2>"Kana desta estação"</h2>
                    <div class="row">
                        <button class="btn sm" on:click=game("flash")>"⚡ Relâmpago"</button>
                        <button class="btn sm" on:click=game("rain")>"🌧 Chuva"</button>
                        <button class="btn sm" on:click=game("write")>"✍️ Escrever"</button>
                    </div>
                </div>
                <div class="kanatable" inner_html=cells></div>
                <div class="tiny faint">"Verde = firme (80%+ de acerto) · amarelo = quase · vermelho = fraco. Toque para ouvir."</div>
            </div>
        }
    });

    let grammar = (!pts.is_empty()).then(|| {
        let cards = pts.iter().map(|p| point_card(p, false, move || rev.update(|v| *v += 1))).collect_view();
        view! { <div class="stack"><div class="row-between"><h2>"Gramática"</h2><div inner_html=role_legend(&[])></div></div>{cards}</div> }
    });

    let kanji_card = (!ks.is_empty()).then(|| {
        let ids: Vec<String> = ks.iter().map(|k| format!("k:{}", k.k)).collect();
        let title = format!("{} · kanji", st.id);
        let tiles = ks
            .iter()
            .map(|k| {
                let ch = k.k.clone();
                let cls = format!("ktile s{}", michi_core::Card::status(store::card(&format!("k:{}", k.k)).as_ref()));
                view! { <button class=cls title={k.en.join(", ")} on:click=move |_| crate::views::kanji::open_kanji(&ch)>{k.k.clone()}</button> }
            })
            .collect_view();
        view! {
            <div class="card stack">
                <div class="row-between"><h2>"Kanji da estação "<span class="small muted">{format!("({})", ks.len())}</span></h2>
                    <button class="btn sm primary" on:click=move |_| go("revisar", Some(json!({"custom": ids.clone(), "title": title.clone()})))>"Estudar estes agora"</button></div>
                <div class="kgrid">{tiles}</div>
            </div>
        }
    });

    let vocab_card = (!vocab.is_empty()).then(|| {
        let items: String = vocab
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
        view! { <div class="card stack"><h2>"Vocabulário"</h2><div class="row" style="gap:6px" inner_html=items></div></div> }
    });

    let videos: String = st
        .yt
        .iter()
        .filter(|q| !q.is_empty())
        .map(|q| {
            format!(
                r#"<a class="btn sm" href="{}" target="_blank" rel="noopener">▶ {}</a>"#,
                esc(&yt_search(q)),
                esc(q)
            )
        })
        .collect();
    let res = resource_links(c, &st.line);

    view! {
        {header}
        {trip}
        {kana_card}
        {grammar}
        {kanji_card}
        {vocab_card}
        <div class="card stack"><h2>"Vídeos e sites"</h2><div class="row" inner_html=videos></div><div inner_html=res></div></div>
    }
    .into_any()
}

/* ---------------- trip stop extras (11b-trip.js) ---------------- */

fn trip_block(c: &Content, st: &Station, page: RwSignal<Page>, rev: RwSignal<u64>, pids: Vec<String>) -> AnyView {
    let r = station_ready(c, st);
    let sg = store::read(|s| s.j.signs.clone());
    let part = |label: &str, v: f64| {
        format!(
            r#"<div><div class="row-between tiny"><span>{}</span><span class="num">{}%</span></div>{}</div>"#,
            label,
            (v * 100.0).round(),
            bar(v, "var(--trip)")
        )
    };
    let parts = [
        part("Gramática estudada", r.g),
        part("Chefão", r.boss),
        part("Frases faladas 🎤", r.spoken),
        part("Kanji de placa", r.signs),
    ]
    .concat();
    let signs_html: String = st
        .signs
        .iter()
        .map(|x| {
            format!(
                r#"<button class="btn sm jp" data-say="{}">{} <span class="tiny muted">{}</span>{}</button>"#,
                esc(&kana_j(&x.0)),
                ruby(&x.0, "always"),
                esc(&x.1),
                if sg.get(&plain_j(&x.0)).copied().unwrap_or(0.0) > 0.0 { " ✓" } else { "" }
            )
        })
        .collect();
    let has_signs = !st.signs.is_empty();
    let st_q = st.clone();
    let base = st
        .base
        .iter()
        .filter_map(|b| c.station(b))
        .map(|v| {
            let id = v.id.clone();
            view! { <button class="btn sm ghost" on:click=move |_| page.set(Page::Station(id.clone()))>{format!("{} · {}", v.id, v.name)}</button> }
        })
        .collect_view();
    let has_base = !st.base.is_empty();
    let roleplay = st.roleplay.clone().filter(|_| SENSEI_ON).map(|rp| {
        format!(
            "<b>🎭 Ensaio:</b> {} <span class=\"muted\">Goal: {}</span>",
            esc(rp.get("setting").and_then(|v| v.as_str()).unwrap_or("")),
            esc(rp.get("goal").and_then(|v| v.as_str()).unwrap_or(""))
        )
    });
    view! {
        <div class="card stack" style="border-left:5px solid var(--trip)">
            <div class="row-between"><h2>"Na viagem"</h2>
                <span class="pill">{format!("{} · {}{}", st.days.clone().unwrap_or_default(), st.city.clone().unwrap_or_default(), if st.talk { " · 🗣️" } else { "" })}</span></div>
            <div class="trip-cities" inner_html=parts></div>
            <div class="row">
                <button class="btn primary" on:click=move |_| go("frases", Some(json!({"mode": "fala", "pids": pids.clone()})))>"🎤 Falar as frases"</button>
                {has_signs.then(|| view! { <button class="btn" on:click=move |_| sign_quiz(&st_q, rev)>"🪧 Kanji de placa"</button> })}
            </div>
            {roleplay.map(|h| view! { <div class="small" inner_html=h></div> })}
            {has_signs.then(|| view! {
                <div class="stack" style="gap:6px"><div class="eyebrow">"O que você vai ver escrito"</div><div class="row" style="gap:6px" inner_html=signs_html></div></div>
            })}
            {has_base.then(|| view! { <div class="small muted row" style="gap:6px">"Base: "{base}</div> })}
        </div>
    }
    .into_any()
}

fn sign_pool(c: &Content) -> Vec<(String, String)> {
    trip_stations(c).into_iter().flat_map(|s| s.signs.clone()).collect()
}

fn sign_options(c: &Content, x: &(String, String)) -> Vec<String> {
    let others: Vec<String> = shuffle(&sign_pool(c).into_iter().filter(|y| y.1 != x.1).collect::<Vec<_>>())
        .into_iter()
        .take(3)
        .map(|y| y.1)
        .collect();
    let mut v = vec![x.1.clone()];
    v.extend(others);
    shuffle(&v)
}

/// `signQuiz`: read the sign (kanji only) → pick the meaning → hear it.
fn sign_quiz(st: &Station, rev: RwSignal<u64>) {
    let Some(c) = data::content() else { return };
    let items = shuffle(&st.signs);
    if items.is_empty() {
        return;
    }
    let qs: Vec<Mc> = items
        .iter()
        .map(|x| Mc {
            prompt: format!(
                r#"<div class="stack" style="align-items:center"><div class="signboard jp">{}</div><div class="small muted">O que esta placa diz?</div></div>"#,
                esc(&plain_j(&x.0))
            ),
            opts: sign_options(&c, x),
            a: x.1.clone(),
            say: None,
            wide: true,
            jp: false,
            reveal: Some(format!(r#"<div class="sent" style="text-align:center">{} {}</div>"#, ruby(&x.0, "always"), spk(&kana_j(&x.0), "btn sm"))),
        })
        .collect();
    let keys: Vec<(String, String)> = items.iter().map(|x| (x.1.clone(), x.0.clone())).collect();
    let st2 = st.clone();
    open_quiz(
        &format!("🪧 Kanji de placa · {}", esc(&st.id)),
        false,
        qs,
        Arc::new(move |q: &Mc, ok: bool| {
            if let Some((_, w)) = keys.iter().find(|(m, _)| *m == q.a) {
                if ok {
                    let k = plain_j(w);
                    store::write(&["journey"], |s| *s.j.signs.entry(k).or_insert(0.0) += 1.0);
                }
                crate::tts::say(&kana_j(w), None);
            }
        }),
        Arc::new(move |right, _| {
            store::add_xp(5.0 + right as f64 * 2.0);
            rev.update(|v| *v += 1);
        }),
        Arc::new(move |right, total, mid| {
            let st3 = st2.clone();
            view! {
                <div class="stack" style="text-align:center">
                    <h2>{format!("{right}/{total}")}</h2>
                    <p class="muted">{if right == total { "Você já lê as placas desta parada." } else { "Repita até acertar todas: cada acerto conta para a prontidão da viagem." }}</p>
                    {result_buttons(mid, move || sign_quiz(&st3, rev))}
                </div>
            }
            .into_any()
        }),
    );
}

/* ---------------- boss ---------------- */

const PARTICLES: [&str; 11] = ["は", "が", "を", "に", "で", "へ", "と", "も", "の", "から", "まで"];

/// `bossQuestions(st)`.
fn boss_questions(c: &Content, st: &Station) -> Vec<Mc> {
    let mut q: Vec<Mc> = Vec::new();
    let kn = st.kana.as_deref().map(kana_set).unwrap_or_default();
    if !kn.is_empty() {
        for _ in 0..10 {
            let Some(x) = pick(&kn) else { break };
            let others: Vec<String> = shuffle(&kn.iter().filter(|y| y.r != x.r).collect::<Vec<_>>())
                .into_iter()
                .take(3)
                .map(|y| y.r.clone())
                .collect();
            let mut opts = vec![x.r.clone()];
            opts.extend(others);
            q.push(Mc {
                prompt: format!(r#"<div class="bigq">{}</div>"#, esc(&x.k)),
                opts: shuffle(&opts),
                a: x.r.clone(),
                say: Some(x.k.clone()),
                ..Mc::default()
            });
        }
    }
    let pts = station_points(c, st);
    let sents = shuffle(&pts.iter().flat_map(|p| p.s.iter().cloned()).collect::<Vec<_>>());
    let ks = station_kanji(st);
    if !ks.is_empty() {
        let n = if pts.is_empty() { 10 } else { 3 };
        let all_k = data::kanji().unwrap_or_default();
        for k in shuffle(&ks).into_iter().take(n) {
            let pool: Vec<_> = if ks.len() >= 4 { ks.clone() } else { all_k.iter().take(300).cloned().collect() };
            let others: Vec<String> = shuffle(&pool)
                .into_iter()
                .filter(|y| y.k != k.k)
                .take(3)
                .map(|o| o.en.first().cloned().unwrap_or_default())
                .collect();
            let a = k.en.first().cloned().unwrap_or_default();
            let mut opts = vec![a.clone()];
            opts.extend(others);
            q.push(Mc {
                prompt: format!(
                    r#"<div class="bigq">{}</div><div class="small muted">Qual o significado?</div>"#,
                    esc(&k.k)
                ),
                opts: shuffle(&opts),
                a,
                ..Mc::default()
            });
        }
    }
    for x in shuffle(&st.signs).into_iter().take(2) {
        q.push(Mc {
            prompt: format!(
                r#"<div class="signboard jp">{}</div><div class="small muted">O que esta placa diz?</div>"#,
                esc(&plain_j(&x.0))
            ),
            opts: sign_options(c, &x),
            a: x.1.clone(),
            say: Some(kana_j(&x.0)),
            wide: true,
            ..Mc::default()
        });
    }
    for s in sents.iter().take(3) {
        let idx: Vec<usize> = s.c.iter().enumerate().filter(|(_, c)| c.p.is_some()).map(|(i, _)| i).collect();
        let Some(i) = pick(&idx) else { continue };
        let right = s.c[i].p.clone().unwrap_or_default();
        let pool: Vec<&str> = PARTICLES.iter().copied().filter(|x| *x != right).collect();
        let html: String =
            s.c.iter()
                .enumerate()
                .map(|(j, ch)| {
                    let p = match &ch.p {
                        Some(p) if j == i => r#"<span class="blank">？</span>"#.to_string(),
                        Some(p) => esc(p),
                        None => String::new(),
                    };
                    format!("{}{}", ruby(&ch.w, ""), p)
                })
                .collect::<String>()
                + &esc(&crate::jp::end_of(s));
        let mut opts = vec![right.clone()];
        opts.extend(shuffle(&pool).into_iter().take(3).map(str::to_string));
        q.push(Mc {
            prompt: format!(r#"<div class="sent">{}</div><div class="en">{}</div>"#, html, esc(&s.en)),
            opts: shuffle(&opts),
            a: right,
            jp: true,
            ..Mc::default()
        });
    }
    for s in sents.iter().skip(3).take(3) {
        let wrong: Vec<String> = shuffle(&sents.iter().filter(|x| x.id != s.id).cloned().collect::<Vec<_>>())
            .into_iter()
            .take(2)
            .map(|x| x.en)
            .collect();
        if wrong.len() < 2 {
            continue;
        }
        let mut opts = vec![s.en.clone()];
        opts.extend(wrong);
        q.push(Mc {
            prompt: format!(
                r#"<div class="sent">{}</div><div class="small muted">O que significa?</div>"#,
                sent_html(s, false, false, "")
            ),
            opts: shuffle(&opts),
            a: s.en.clone(),
            wide: true,
            say: Some(sent_plain(s)),
            ..Mc::default()
        });
    }
    for s in sents.iter().skip(6).take(2) {
        let cands: Vec<_> = s.c.iter().filter(|c| c.r != "PRED").cloned().collect();
        let Some(ch) = pick(&cands) else { continue };
        let a = chunk_text(&ch);
        let mut opts = vec![a.clone()];
        opts.extend(
            shuffle(&s.c.iter().filter(|x| chunk_text(x) != a).cloned().collect::<Vec<_>>())
                .into_iter()
                .take(3)
                .map(|x| chunk_text(&x)),
        );
        q.push(Mc {
            prompt: format!(
                r#"<div class="sent">{}</div><div class="small muted">Qual pedaço responde: <b>{}</b></div>"#,
                sent_html(s, false, false, ""),
                esc(role(&ch.r).3)
            ),
            opts: shuffle(&opts),
            a,
            jp: true,
            ..Mc::default()
        });
    }
    shuffle(&q).into_iter().take(10).collect()
}

/// `bossQuiz(st)`: 80%+ clears the station and stamps it.
fn boss_quiz(st: &Station, rev: RwSignal<u64>) {
    let Some(c) = data::content() else { return };
    let qs = boss_questions(&c, st);
    if qs.is_empty() {
        toast("Esta estação ainda não tem conteúdo para o chefão.", "");
        return;
    }
    let first = crate::app::detached(|| StoredValue::new(false));
    let id = st.id.clone();
    let st2 = st.clone();
    open_quiz(
        &format!("⚔️ Chefão · {}", esc(&st.id)),
        true,
        qs,
        Arc::new(|_, _| {}),
        Arc::new(move |right, total| {
            let pct = (right as f64 / total as f64 * 100.0).round();
            let stars = if pct == 100.0 {
                3.0
            } else if pct >= 90.0 {
                2.0
            } else if pct >= 80.0 {
                1.0
            } else {
                0.0
            };
            let now = now_ms();
            let is_first = store::write(&["journey"], |s| {
                let b = s.j.best.entry(id.clone()).or_insert(0.0);
                *b = b.max(pct);
                let first = pct >= 80.0 && !s.j.done.contains_key(&id);
                if pct >= 80.0 {
                    s.j.done.entry(id.clone()).or_insert(now);
                    s.j.stamps.entry(id.clone()).or_insert(now);
                }
                first
            });
            first.set_value(is_first);
            store::with_today(|d| d.gp += 1.0);
            store::add_xp(20.0 + stars * 15.0 + if is_first { 50.0 } else { 0.0 });
            rev.update(|v| *v += 1);
        }),
        Arc::new(move |right, total, mid| {
            let pct = (right as f64 / total as f64 * 100.0).round();
            let stars = if pct == 100.0 {
                3
            } else if pct >= 90.0 {
                2
            } else if pct >= 80.0 {
                1
            } else {
                0
            };
            let is_first = first.get_value();
            let msg = if pct >= 80.0 {
                if is_first {
                    "Estação concluída! Selo carimbado."
                } else {
                    "Muito bem — recorde atualizado."
                }
            } else {
                "Ainda não chegou em 80%. Revise os pontos e tente de novo — sem pressa."
            };
            let stamp = is_first.then(|| stamp_svg(&st2, Some(now_ms())));
            let st3 = st2.clone();
            view! {
                <div style="text-align:center" class="stack">
                    <div style="font-size:44px;letter-spacing:6px">{format!("{}{}", "★".repeat(stars), "☆".repeat(3 - stars))}</div>
                    <h2>{format!("{pct}% · {right}/{total}")}</h2>
                    <p class="muted">{msg}</p>
                    {stamp.map(|s| view! { <div style="width:140px;margin:0 auto" inner_html=s></div> })}
                    {result_buttons(mid, move || boss_quiz(&st3, rev))}
                </div>
            }
            .into_any()
        }),
    );
}
