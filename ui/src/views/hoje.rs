//! Hoje — the daily departure board (port of `Views.hoje` in `10-today.js`).

use crate::app::{bar, go, intent_for_block, stat_tile, toast};
use crate::data;
use crate::logic::{
    block_done_today, code_badge, current_station, line, main_line, station_progress, station_ready, total_due,
    trip_countdown, trip_headline, trip_range, trip_ready, trip_stations, vlog_of_day,
};
use crate::store::{self, sigs};
use crate::util::{esc, ev_checked, ev_value, safe_url};
use leptos::prelude::*;
use michi_core::agenda::Occ;
use michi_core::routine::active_blocks;
use michi_core::time::{dow, hhmm, key_add, local, minutes_of, now_ms, parse_key, study_dow, today_key};
use serde_json::json;
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Hoje() -> impl IntoView {
    let s = sigs();
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
    // Today's agenda items that are not routine blocks (own events, trip day).
    let agenda = RwSignal::new(Vec::<Occ>::new());
    let arev = RwSignal::new(0u64);
    crate::tauri::listen::<serde_json::Value>("michi://agenda-changed", move |_| {
        if arev.try_get_untracked().is_some() {
            arev.update(|v| *v += 1);
        }
    });
    Effect::new(move |_| {
        s.view.get();
        arev.get();
        #[derive(serde::Serialize)]
        struct R {
            from: String,
            to: String,
        }
        let k = today_key();
        spawn_local(async move {
            if let Ok(v) = crate::tauri::invoke::<Vec<Occ>, _>("agenda_range", &R { from: k.clone(), to: k }).await {
                if agenda.try_get_untracked().is_some() {
                    agenda.set(v.into_iter().filter(|o| o.source != "rotina").collect());
                }
            }
        });
    });
    let clock = RwSignal::new(hhmm(now_ms()));
    let h = set_interval_with_handle(move || clock.set(hhmm(now_ms())), std::time::Duration::from_secs(20)).ok();
    on_cleanup(move || {
        if let Some(h) = h {
            h.clear();
        }
    });
    move || {
        s.view.get();
        if !ready.get() {
            return view! { <div class="loading"><div class="spin"></div>"Preparando o dia…"</div> }.into_any();
        }
        page(clock, agenda.get())
    }
}

fn page(clock: RwSignal<String>, agenda: Vec<Occ>) -> AnyView {
    let c = data::content().unwrap_or_default();
    let p = store::p();
    let d = store::today();
    let now = now_ms();
    let lt = local(now);
    let now_m = minutes_of(&lt.format("%H:%M").to_string());
    let blocks = active_blocks(&p.routine, study_dow(now), p.minimal_day);
    let mins = d.study_minutes();
    let due = total_due();
    let st = current_station(&c).cloned();
    let done_of: Vec<bool> = blocks.iter().map(|b| block_done_today(&b.id)).collect();
    let next_idx =
        blocks.iter().enumerate().position(|(i, b)| !done_of[i] && minutes_of(&b.time) + b.min as i64 >= now_m);
    let goal = if p.minimal_day {
        25.0
    } else if p.goal_min > 0.0 {
        p.goal_min
    } else {
        75.0
    };
    let total_min: f64 = blocks.iter().map(|b| b.min).sum();
    let streak = store::streak();

    /* ---- board rows (routine blocks + own agenda events, by time) ---- */
    let mut rows: Vec<(i64, AnyView)> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let done = done_of[i];
            let t0 = minutes_of(&b.time);
            let is_now = !done && now_m >= t0 && now_m <= t0 + b.min as i64 + 30;
            let label = if done {
                "済 FEITO"
            } else if is_now {
                "発車 AGORA"
            } else if Some(i) == next_idx {
                "次 PRÓXIMO"
            } else if now_m > t0 + b.min as i64 {
                "未 PENDENTE"
            } else {
                "—"
            };
            let (go_route, bid) = (b.go.clone(), b.id.clone());
            let bid2 = b.id.clone();
            let name = b.name.clone();
            let (fid, fname, fjp, fmin) = (b.id.clone(), b.name.clone(), b.jp.clone(), b.min);
            let row = view! {
                <div class="dep" class:done=done class:now=is_now role="listitem"
                    on:click=move |_| go(&go_route, intent_for_block(Some(&bid)))>
                    <div class="t">{b.time.clone()}</div>
                    <div style="min-width:0">
                        <div class="n"><span class="j">{b.jp.clone()}</span>{b.name.clone()}<span class="w">{format!(" · {} min", b.min)}</span></div>
                        <div class="w">{b.what.clone()}</div>
                    </div>
                    <div class="row" style="gap:8px">
                        <span class="st">{label}</span>
                        <button class="foc-b" title=format!("Modo foco: {} min", fmin) aria-label=format!("Começar foco em {}", fname)
                            on:click=move |ev: web_sys::MouseEvent| {
                                ev.stop_propagation();
                                crate::views::focar::start_block(&fid, &fname, &fjp, fmin);
                            }>"▶"</button>
                        <input type="checkbox" class="chk" prop:checked=done aria-label=format!("Marcar {name} como feito")
                            on:click=move |ev: web_sys::MouseEvent| {
                                ev.stop_propagation();
                                let checked = ev.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlInputElement>(t).ok()).map(|i| i.checked()).unwrap_or(false);
                                store::with_today(|d| { d.done.insert(bid2.clone(), checked); });
                                if checked { store::add_xp_at(10.0, Some(&ev)); }
                                set_timeout(store::refresh, std::time::Duration::from_millis(350));
                            }/>
                    </div>
                </div>
            }
            .into_any();
            (t0, row)
        })
        .collect();
    for o in agenda.iter().filter(|o| !o.all_day) {
        let t0 = minutes_of(&o.time);
        let done = o.done;
        let is_now = !done && now_m >= t0 && now_m <= t0 + o.dur + 30;
        let label = if done {
            "済 FEITO"
        } else if is_now {
            "発車 AGORA"
        } else if now_m > t0 + o.dur {
            "未 PENDENTE"
        } else {
            "—"
        };
        let (o2, o3, o4) = (o.clone(), o.clone(), o.clone());
        let row = view! {
            <div class="dep" class:done=done class:now=is_now role="listitem" on:click=move |_| crate::views::agenda::open_details(o2.clone())>
                <div class="t">{o.time.clone()}</div>
                <div style="min-width:0">
                    <div class="n"><span class="kd" style=format!("--kc:{}", o.color)></span>{o.title.clone()}<span class="w">{format!(" · {} min · {}", o.dur, o.kind.label())}</span></div>
                    <div class="w">{o.notes.clone()}</div>
                </div>
                <div class="row" style="gap:8px">
                    <span class="st">{label}</span>
                    <button class="foc-b" title=format!("Modo foco: {} min", o.dur) aria-label=format!("Começar foco em {}", o.title)
                        on:click=move |ev: web_sys::MouseEvent| {
                            ev.stop_propagation();
                            crate::views::focar::start_occ(&o4);
                        }>"▶"</button>
                    <input type="checkbox" class="chk" prop:checked=done aria-label=format!("Marcar {} como feito", o.title)
                        on:click=move |ev: web_sys::MouseEvent| {
                            ev.stop_propagation();
                            let checked = ev.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlInputElement>(t).ok()).map(|i| i.checked()).unwrap_or(false);
                            if checked { crate::util::xp_float(10.0, Some(&ev)); }
                            crate::views::agenda::mark(&o3, checked);
                        }/>
                </div>
            </div>
        }
        .into_any();
        rows.push((t0, row));
    }
    rows.sort_by_key(|r| r.0);
    let n_rows = rows.len();
    let rows = rows.into_iter().map(|r| r.1).collect_view();

    /* ---- banners ---- */
    let mut banners: Vec<AnyView> = Vec::new();
    if let Some(t) = agenda.iter().find(|o| o.all_day && o.source == "viagem") {
        let t2 = t.clone();
        banners.push(view! {
            <div class="banner" style=format!("border-left:6px solid {}", t.color)><span class="bico">"✈️"</span>
                <div class="grow"><b>{t.jp.clone()}" "</b>{t.title.clone()}</div>
                <button class="btn sm" on:click=move |_| crate::views::agenda::open_details(t2.clone())>"Detalhes"</button>
            </div>
        }.into_any());
    }
    if !p.onboarded {
        banners.push(onboarding(&c));
    }
    let vb = blocks.iter().find(|b| b.id == "vlog");
    let vlog = vlog_of_day(&c);
    let vlog_done = vb.is_none() || block_done_today("vlog");
    let vlog_time = vb.map(|b| b.time.clone()).unwrap_or_default();
    let vlog_t = minutes_of(if vlog_time.is_empty() { "21:30" } else { &vlog_time });
    if !vlog_done && now_m >= vlog_t - 60 {
        let sug = match &vlog {
            Some(v) => format!("Sugestão de hoje: <b>{}</b> — {}", esc(&v.name), esc(&v.best_for)),
            None => "Escolha um canal na aba Imersão.".into(),
        };
        let url = vlog.as_ref().map(|v| safe_url(&v.url));
        banners.push(view! {
            <div class="banner warn"><span class="bico">"📺"</span>
                <div class="grow"><b>"Hora do vlog em japonês. "</b><span inner_html=sug></span></div>
                {url.map(|u| view! { <a class="btn sm primary" href=u target="_blank" rel="noopener">"Abrir no YouTube"</a> })}
                <button class="btn sm" on:click=|_| go("imersao", None)>"Registrar"</button>
            </div>
        }.into_any());
    } else if !vlog_done {
        let sug = vlog.as_ref().map(|v| format!(" Sugestão: <b>{}</b>.", esc(&v.name))).unwrap_or_default();
        let url = vlog.as_ref().map(|v| safe_url(&v.url));
        let txt = format!("Lembrete: hoje tem <b>vlog em japonês</b> às {}.{}", esc(&vlog_time), sug);
        banners.push(
            view! {
                <div class="banner line"><span class="bico">"📺"</span>
                    <div class="grow" inner_html=txt></div>
                    {url.map(|u| view! { <a class="btn sm" href=u target="_blank" rel="noopener">"Ver canal"</a> })}
                </div>
            }
            .into_any(),
        );
    }
    if now_m >= 21 * 60 && d.xp < 20.0 && streak > 0 {
        banners.push(view! {
            <div class="banner warn"><span class="bico">"🔥"</span>
                <div class="grow">{format!("Seu streak de {streak} dias está em risco. Dez revisões já salvam o dia.")}</div>
                <button class="btn sm primary" on:click=|_| go("revisar", None)>"Revisar agora"</button>
            </div>
        }.into_any());
    }
    if p.anki_done != today_key() {
        banners.push(
            view! {
                <div class="banner"><span class="bico">"🃏"</span>
                    <div class="grow">"Já fez o seu "<b>"Anki"</b>" (Kaishi) hoje?"</div>
                    <button class="btn sm" on:click=move |ev| {
                        store::write(&["profile"], |s| s.p.anki_done = today_key());
                        store::add_xp_at(10.0, Some(&ev));
                        store::refresh();
                    }>"Sim, fiz"</button>
                </div>
            }
            .into_any(),
        );
    }

    /* ---- cards ---- */
    let station_card = st.as_ref().map(|st| {
        let sp = station_progress(&c, st);
        let mut info = Vec::new();
        if sp.n_pts > 0 {
            info.push(format!("Gramática {}/{}", (sp.g.unwrap_or(0.0) * sp.n_pts as f64).round(), sp.n_pts));
        }
        if sp.n_k > 0 {
            info.push(format!("Kanji {}/{}", (sp.k.unwrap_or(0.0) * sp.n_k as f64).round(), sp.n_k));
        }
        if sp.n_kana > 0 {
            info.push(format!("Kana firmes {}/{}", (sp.ka.unwrap_or(0.0) * sp.n_kana as f64).round(), sp.n_kana));
        }
        info.push(format!("Chefão {}", if sp.best > 0.0 { format!("{}%", sp.best) } else { "não feito".into() }));
        let id = st.id.clone();
        view! {
            <div class="card stack">
                <div class="eyebrow">"Sua estação atual"</div>
                <div class="row">
                    <span inner_html=code_badge(st, false)></span>
                    <div class="grow"><h2>{st.name.clone()}</h2><div class="jp muted">{st.jp.clone()}</div></div>
                </div>
                <div inner_html=bar(sp.frac, line(&st.line).lc)></div>
                <div class="small muted">{info.join(" · ")}</div>
                <div class="row">
                    <button class="btn primary" on:click=move |_| go("jornada", Some(json!({"station": id.clone()})))>"Abrir estação"</button>
                    <button class="btn" on:click=|_| go("frases", None)>"Praticar frases"</button>
                </div>
            </div>
        }
    });

    let wg = c.wagotabi.areas.iter().find(|a| a.id == p.wagotabi).cloned();
    let wg_body = match &wg {
        Some(a) => {
            let lessons = a
                .lessons
                .iter()
                .filter_map(|l| c.lesson(l).map(|x| (l.clone(), if x.title_jp.is_empty() { x.title.clone() } else { x.title_jp.clone() })))
                .take(4)
                .map(|(lid, t)| view! { <button class="btn sm" on:click=move |_| crate::views::lesson::open_lesson(&lid)>{t}</button> })
                .collect_view();
            view! {
                <h2>{a.name.clone()}" "<span class="jp muted small">{a.jp.clone()}</span></h2>
                <div class="small muted">{format!("O jogo trabalha aqui: {}.", a.topics.join("; "))}</div>
                <div class="row">{lessons}</div>
            }
            .into_any()
        }
        None => view! { <p class="muted">"Diga onde você está no Wagotabi e o app mostra o que reforçar aqui."</p> }
            .into_any(),
    };
    let wg_opts = c
        .wagotabi
        .areas
        .iter()
        .map(|a| {
            let sel = a.id == p.wagotabi;
            view! { <option value=a.id.clone() selected=sel>{format!("{} · {}", a.pref, a.name)}</option> }
        })
        .collect_view();

    let trip_card = (!trip_stations(&c).is_empty()).then(|| {
        let cd = trip_countdown();
        let r = trip_ready(&c);
        let nx = trip_stations(&c).into_iter().find(|s| station_ready(&c, s).frac < 0.75).cloned();
        let nx_line = nx.as_ref().map(|s| {
            format!("Próxima parada para treinar: <b>{} · {}</b>{}", esc(&s.id), esc(&s.name), if s.talk { " 🗣️" } else { "" })
        });
        let nx_id = nx.map(|s| s.id);
        view! {
            <div class="card stack" style="border-top:6px solid var(--trip)">
                <div class="eyebrow">{format!("✈️ Japão · {}", trip_range())}</div>
                <div class="row-between"><h2 inner_html=trip_headline(&cd)></h2><span class="pill line">{format!("{}% pronto", (r * 100.0).round())}</span></div>
                <div inner_html=bar(r, "var(--trip)")></div>
                {nx_line.map(|t| view! { <div class="small muted" inner_html=t></div> })}
                <div class="row">
                    {nx_id.map(|id| view! { <button class="btn sm primary" on:click=move |_| go("jornada", Some(json!({"station": id.clone()})))>"Treinar esta parada"</button> })}
                    <button class="btn sm" on:click=|_| go("jornada", Some(json!({"tab": "trip"})))>"Ver Linha Viagem"</button>
                </div>
            </div>
        }
    });

    /* ---- heat map: 16 weeks, columns = weeks ---- */
    let k0 = key_add(&today_key(), -(7 * 16 - 1));
    let start_dow = parse_key(&k0).map(dow).unwrap_or(0) as usize;
    let heat = (0..start_dow)
        .map(|_| view! { <i style="visibility:hidden"></i> }.into_any())
        .chain((0..7 * 16).map(|i| {
            let kk = key_add(&k0, i);
            let m = store::day(&kk).map(|d| d.study_minutes()).unwrap_or(0.0);
            let cl = if m >= 90.0 {
                4
            } else if m >= 60.0 {
                3
            } else if m >= 30.0 {
                2
            } else if m >= 5.0 {
                1
            } else {
                0
            };
            view! { <i class=format!("h{cl}") title=format!("{kk}: {m} min")></i> }.into_any()
        }))
        .collect_view();

    let has_rows = n_rows > 0;
    view! {
        <div class="row-between">
            <h1>"Hoje"</h1>
            <div class="row">
                <label class="switch">
                    <input type="checkbox" prop:checked=p.minimal_day on:change=|ev| {
                        let v = ev_checked(&ev);
                        store::write(&["profile"], |s| s.p.minimal_day = v);
                        store::refresh();
                    }/>
                    " Dia mínimo (≈25 min)"
                </label>
                <button class="btn sm" on:click=|_| go("ajustes", Some(json!({"focus": "rotina"})))>"✏️ Ajustar horários"</button>
            </div>
        </div>
        {banners}
        <div class="stats" inner_html=[
            stat_tile(&format!("{}<span class=\"small faint\"> / {} min</span>", mins, goal), "estudados hoje"),
            stat_tile(&(due.rev + due.learn).to_string(), "revisões pendentes"),
            stat_tile(&due.fresh.to_string(), "cartas novas liberadas"),
            stat_tile(&d.xp.to_string(), "XP hoje"),
        ].concat()></div>
        <div class="board" role="list">
            <div class="board-h">
                <span class="bj">"本日の学習"</span>
                <span>{format!("PARTIDAS DE HOJE · {} MIN", total_min)}</span>
                <span class="num">{move || clock.get()}</span>
            </div>
            {rows}
            {(!has_rows).then(|| view! {
                <div class="dep"><div class="t">"—"</div><div class="w">"Nenhum bloco marcado para hoje. Use “Ajustar horários” para escolher os dias."</div></div>
            })}
        </div>
        <crate::views::focar::FocusCard/>
        <div class="grid2">
            {station_card}
            <div class="card stack">
                <div class="eyebrow">"Wagotabi × Michi"</div>
                {wg_body}
                <div class="row">
                    <select aria-label="Área do Wagotabi" on:change=|ev| {
                        let v = ev_value(&ev);
                        store::write(&["profile"], |s| s.p.wagotabi = v);
                        store::refresh();
                    }>
                        <option value="">"Onde estou no Wagotabi…"</option>
                        {wg_opts}
                    </select>
                    <button class="btn sm" on:click=|_| go("jornada", Some(json!({"wline": true})))>"Ver Linha Wagotabi"</button>
                </div>
            </div>
            {trip_card}
        </div>
        <div class="card stack">
            <div class="row-between"><div class="eyebrow">"Últimas 16 semanas"</div><div class="tiny faint">"cada quadrado = um dia · mais escuro = mais minutos"</div></div>
            <div class="heat">{heat}</div>
        </div>
    }
    .into_any()
}

/// First-run banner: pick the current station (`positionPicker`).
fn onboarding(c: &data::Content) -> AnyView {
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
        <div class="banner line" style="align-items:flex-start">
            <span class="bico">"🚉"</span>
            <div class="grow stack" style="gap:8px">
                <div><b>"Bem-vindo ao Michi!"</b>" Onde você está no caminho? Escolha a estação atual (ex.: a lição do Genki em que você está)."</div>
                <div class="row">
                    <select aria-label="Estação atual" on:change=move |ev| sel.set(ev_value(&ev))>{opts}</select>
                    <label class="switch small">
                        <input type="checkbox" on:change=move |ev| mark.set(ev_checked(&ev))/>
                        " marcar kanji anteriores como já vistos"
                    </label>
                    <button class="btn sm primary" on:click=move |_| {
                        if let Some(msg) = crate::logic::set_position(&sel.get_untracked(), mark.get_untracked()) {
                            toast(&msg, "ok");
                        }
                        store::refresh();
                    }>"Definir"</button>
                </div>
            </div>
            <button class="btn xs ghost" on:click=|_| {
                store::write(&["profile"], |s| s.p.onboarded = true);
                store::refresh();
            }>"Depois"</button>
        </div>
    }
    .into_any()
}
