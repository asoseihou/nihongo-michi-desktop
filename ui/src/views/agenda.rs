//! Agenda — the app's own Google-Calendar-like calendar: month, week, day
//! and list views; events with recurrence (RRULE), skips and moves of one
//! occurrence, several reminders, drag to move and drag to resize, colours
//! per kind. Marking an occurrence done feeds Hoje, the streak and the heat
//! map (the backend writes the day document and replays it here).

use crate::app::{close_modal, detached, go, intent_for_block, open_modal, toast};
use crate::store::{self, sigs};
use crate::tauri::{self, NoArgs};
use crate::util::{esc, ev_checked, ev_value};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use leptos::prelude::*;
use michi_core::agenda::{Event, Kind, Occ};
use michi_core::ops::Op;
use michi_core::routine::{days_label, routine_of, Block, WEEK, WEEK1, WEEK_ORDER};
use michi_core::rrule::{js_of_weekday, weekday_of_js, Freq, RRule};
use michi_core::time::{key_add, key_of, minutes_of, now_ms, parse_key, today_key, MONTH_PT, MONTH_PT_LONG, WD_PT};
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

const HOUR_H: f64 = 48.0;
const SNAP: i64 = 15;

/* ---------------- backend calls ---------------- */

#[derive(Serialize)]
struct Range {
    from: String,
    to: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Mark {
    event_id: String,
    occ: String,
    done: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OccArg {
    event_id: String,
    occ: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MoveArg {
    event_id: String,
    occ: String,
    date: String,
    time: String,
    dur: i64,
}

#[derive(Serialize)]
struct SaveArg {
    event: Event,
}

#[derive(Serialize)]
struct IdArg {
    id: String,
}

pub fn mark(o: &Occ, done: bool) {
    let args = Mark { event_id: o.event_id.clone(), occ: o.occ.clone(), done };
    spawn_local(async move {
        if let Err(e) = tauri::invoke::<Vec<Op>, _>("occ_mark", &args).await {
            toast(&format!("Não consegui marcar: {e}"), "bad");
        }
    });
}

fn after<T: 'static>(r: Result<T, String>, ok: &str) {
    match r {
        Ok(_) => {
            if !ok.is_empty() {
                toast(ok, "ok");
            }
            store::refresh();
        }
        Err(e) => toast(&format!("Não deu: {e}"), "bad"),
    }
}

fn occ_move(o: &Occ, date: String, time: String, dur: i64) {
    let a = MoveArg { event_id: o.event_id.clone(), occ: o.occ.clone(), date, time, dur };
    spawn_local(async move { after(tauri::invoke::<(), _>("occ_move", &a).await, "") });
}

fn occ_skip(o: &Occ) {
    let a = OccArg { event_id: o.event_id.clone(), occ: o.occ.clone() };
    spawn_local(async move { after(tauri::invoke::<(), _>("occ_skip", &a).await, "Ocorrência pulada.") });
}

fn event_save(e: Event, msg: &'static str) {
    spawn_local(async move { after(tauri::invoke::<Event, _>("event_save", &SaveArg { event: e }).await, msg) });
}

async fn event_get(id: &str) -> Option<Event> {
    tauri::invoke::<Option<Event>, _>("event_get", &IdArg { id: id.into() }).await.ok().flatten()
}

/* ---------------- dates ---------------- */

fn d(k: &str) -> NaiveDate {
    parse_key(k).unwrap_or_default()
}

fn monday_of(x: NaiveDate) -> NaiveDate {
    x - Duration::days(x.weekday().num_days_from_monday() as i64)
}

fn hhmm_of(min: i64) -> String {
    let m = min.rem_euclid(24 * 60);
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub fn day_title(k: &str) -> String {
    let x = d(k);
    let rel = if k == today_key() {
        "Hoje · ".to_string()
    } else if k == key_add(&today_key(), 1) {
        "Amanhã · ".to_string()
    } else {
        String::new()
    };
    format!(
        "{rel}{}, {} de {}",
        WD_PT[x.weekday().num_days_from_sunday() as usize],
        x.day(),
        MONTH_PT_LONG[x.month0() as usize]
    )
}

fn when_of(o: &Occ) -> String {
    let x = d(&o.date);
    let base = format!(
        "{}, {} de {} de {}",
        WD_PT[x.weekday().num_days_from_sunday() as usize],
        x.day(),
        MONTH_PT_LONG[x.month0() as usize],
        x.year()
    );
    if o.all_day {
        format!("{base} · dia todo")
    } else {
        format!("{base} · {}–{}", o.time, hhmm_of(minutes_of(&o.time) + o.dur))
    }
}

fn reminder_label(m: i64) -> String {
    match m {
        0 => "na hora".into(),
        m if m % 1440 == 0 => format!("{} dia(s) antes", m / 1440),
        m if m % 60 == 0 => format!("{} h antes", m / 60),
        m => format!("{m} min antes"),
    }
}

/* ---------------- view state ---------------- */

#[derive(Clone, PartialEq)]
struct Drag {
    key: String,
    resize: bool,
    sx: f64,
    sy: f64,
    colw: f64,
    dmin: i64,
    dday: i64,
    moved: bool,
}

#[component]
pub fn Agenda() -> impl IntoView {
    let s = sigs();
    let saved = store::read(|st| st.p.extra.get("agendaView").and_then(|v| v.as_str()).unwrap_or("semana").to_string());
    let mode = RwSignal::new(saved);
    let cursor = RwSignal::new(today_key());
    let occs = RwSignal::new(None::<Vec<Occ>>);
    let hidden = RwSignal::new(Vec::<Kind>::new());
    let list_days = RwSignal::new(14i64);
    let rev = RwSignal::new(0u64);
    let drag = RwSignal::new(None::<Drag>);

    let range = move || -> (String, String) {
        let c = d(&cursor.get());
        match mode.get().as_str() {
            "mes" => {
                let first = NaiveDate::from_ymd_opt(c.year(), c.month(), 1).unwrap_or(c);
                let start = monday_of(first);
                (key_of(start), key_of(start + Duration::days(41)))
            }
            "dia" => (key_of(c), key_of(c)),
            "lista" => (key_of(c), key_of(c + Duration::days(list_days.get() - 1))),
            _ => {
                let m = monday_of(c);
                (key_of(m), key_of(m + Duration::days(6)))
            }
        }
    };
    Effect::new(move |_| {
        s.view.get();
        rev.get();
        let (from, to) = range();
        spawn_local(async move {
            match tauri::invoke::<Vec<Occ>, _>("agenda_range", &Range { from, to }).await {
                Ok(v) => {
                    if occs.try_get_untracked().is_some() {
                        occs.set(Some(v));
                    }
                }
                Err(e) => toast(&format!("Não consegui ler a agenda: {e}"), "bad"),
            }
        });
    });
    tauri::listen::<serde_json::Value>("michi://agenda-changed", move |_| {
        if rev.try_get_untracked().is_some() {
            rev.update(|r| *r += 1);
        }
    });

    // drag & resize (week/day grid)
    let up = window_event_listener(leptos::ev::pointerup, move |e| {
        let Some(dr) = drag.get_untracked() else { return };
        drag.set(None);
        let Some(o) = occs.get_untracked().unwrap_or_default().into_iter().find(|o| o.key() == dr.key) else { return };
        if !dr.moved {
            open_details(o);
            return;
        }
        let _ = e;
        commit_drag(o, &dr);
    });
    let mv = window_event_listener(leptos::ev::pointermove, move |e| {
        let Some(mut dr) = drag.get_untracked() else { return };
        let (dx, dy) = (e.client_x() as f64 - dr.sx, e.client_y() as f64 - dr.sy);
        if !dr.moved && dx.abs() < 4.0 && dy.abs() < 4.0 {
            return;
        }
        dr.moved = true;
        dr.dmin = ((dy / (HOUR_H / 60.0)) / SNAP as f64).round() as i64 * SNAP;
        dr.dday = if dr.resize || dr.colw <= 0.0 { 0 } else { (dx / dr.colw).round() as i64 };
        drag.set(Some(dr));
    });
    on_cleanup(move || {
        up.remove();
        mv.remove();
    });

    let step = move |dir: i64| {
        let c = d(&cursor.get_untracked());
        let n = match mode.get_untracked().as_str() {
            "mes" => {
                let m = if dir > 0 {
                    c.checked_add_months(chrono::Months::new(1))
                } else {
                    c.checked_sub_months(chrono::Months::new(1))
                };
                cursor.set(key_of(m.unwrap_or(c)));
                return;
            }
            "dia" => dir,
            "lista" => dir * 14,
            _ => dir * 7,
        };
        cursor.set(key_of(c + Duration::days(n)));
    };
    let set_mode = move |m: &'static str| {
        mode.set(m.to_string());
        store::write(&["profile"], |st| {
            st.p.extra.insert("agendaView".into(), serde_json::Value::String(m.into()));
        });
    };
    let title = move || {
        let c = d(&cursor.get());
        match mode.get().as_str() {
            "mes" => format!("{} de {}", MONTH_PT_LONG[c.month0() as usize], c.year()),
            "dia" => day_title(&key_of(c)),
            "lista" => format!("A partir de {}", day_title(&key_of(c))),
            _ => {
                let m = monday_of(c);
                let e = m + Duration::days(6);
                if m.month() == e.month() {
                    format!("{}–{} de {} de {}", m.day(), e.day(), MONTH_PT_LONG[m.month0() as usize], m.year())
                } else {
                    format!(
                        "{} {} – {} {} {}",
                        m.day(),
                        MONTH_PT[m.month0() as usize],
                        e.day(),
                        MONTH_PT[e.month0() as usize],
                        e.year()
                    )
                }
            }
        }
    };
    let modes = [("mes", "Mês"), ("semana", "Semana"), ("dia", "Dia"), ("lista", "Lista")]
        .into_iter()
        .map(|(m, l)| view! { <button aria-pressed=move || (mode.get() == m).to_string() on:click=move |_| set_mode(m)>{l}</button> })
        .collect_view();
    let legend = Kind::ALL
        .iter()
        .map(|k| {
            let k = *k;
            view! {
                <button class="ag-leg" class:off=move || hidden.get().contains(&k) style=format!("--kc:{}", k.color()) title="Mostrar/esconder"
                    on:click=move |_| hidden.update(|h| if h.contains(&k) { h.retain(|x| *x != k) } else { h.push(k) })>
                    <i></i>{k.label()}
                </button>
            }
        })
        .collect_view();
    let new_event = move |_| open_editor(None, Some((cursor.get_untracked(), "19:00".to_string())));
    let export = move |_| {
        spawn_local(async {
            match tauri::invoke::<Option<String>, _>("agenda_export_ics", &NoArgs {}).await {
                Ok(Some(p)) => toast(&format!("Agenda exportada: {p}"), "ok"),
                Ok(None) => {}
                Err(e) => toast(&format!("Não deu: {e}"), "bad"),
            }
        })
    };

    view! {
        <div class="row-between">
            <h1>"Agenda"</h1>
            <div class="row">
                <button class="btn sm primary" on:click=new_event>"＋ Evento"</button>
                <button class="btn sm ghost" title="Arquivo .ics para Outlook, Calendário do Windows ou Apple" on:click=export>"🗓️ Exportar .ics"</button>
            </div>
        </div>
        <div class="ag-bar">
            <button class="btn sm" on:click=move |_| cursor.set(today_key())>"Hoje"</button>
            <button class="icon-btn" aria-label="Anterior" on:click=move |_| step(-1)>"‹"</button>
            <button class="icon-btn" aria-label="Próximo" on:click=move |_| step(1)>"›"</button>
            <h2>{title}</h2>
            <div class="grow"></div>
            <div class="seg" role="group" aria-label="Visão">{modes}</div>
        </div>
        <div class="row ag-legend">{legend}</div>
        {move || {
            let all = occs.get();
            let Some(all) = all else { return view! { <div class="loading"><div class="spin"></div>"Carregando a agenda…"</div> }.into_any() };
            let h = hidden.get();
            let list: Vec<Occ> = all.into_iter().filter(|o| !h.contains(&o.kind)).collect();
            match mode.get().as_str() {
                "mes" => month_view(list, cursor, mode),
                "dia" => time_grid(vec![cursor.get_untracked()], list, drag, mode, cursor),
                "lista" => list_view(list, list_days),
                _ => {
                    let m = monday_of(d(&cursor.get_untracked()));
                    time_grid((0..7).map(|i| key_of(m + Duration::days(i))).collect(), list, drag, mode, cursor)
                }
            }
        }}
        <div class="tiny faint">"Arraste um evento para mudar o horário; puxe a borda de baixo para mudar a duração. Clique num horário vazio para criar. Os blocos da rotina e os dias da viagem entram aqui sozinhos."</div>
    }
}

/* ---------------- month ---------------- */

fn month_view(list: Vec<Occ>, cursor: RwSignal<String>, mode: RwSignal<String>) -> AnyView {
    let c = d(&cursor.get_untracked());
    let first = NaiveDate::from_ymd_opt(c.year(), c.month(), 1).unwrap_or(c);
    let start = monday_of(first);
    let today = today_key();
    let heads = WEEK_ORDER.iter().map(|w| view! { <div class="wd">{WEEK[*w as usize]}</div> }).collect_view();
    let cells = (0..42)
        .map(|i| {
            let day = start + Duration::days(i);
            let k = key_of(day);
            let mut items: Vec<Occ> = list
                .iter()
                .filter(|o| {
                    let s = d(&o.date);
                    let e = s + Duration::days(if o.all_day { ((o.dur.max(1440) + 1439) / 1440) - 1 } else { 0 });
                    day >= s && day <= e
                })
                .cloned()
                .collect();
            items.sort_by(|a, b| (!a.all_day, a.time.clone()).cmp(&(!b.all_day, b.time.clone())));
            let n = items.len();
            let chips = items
                .into_iter()
                .take(3)
                .map(|o| {
                    let label = if o.all_day { o.title.clone() } else { format!("{} {}", o.time, o.title) };
                    let o2 = o.clone();
                    let tip = label.clone();
                    view! {
                        <button class="ag-chip" class:allday=o.all_day class:done=o.done style=format!("--kc:{}", o.color) title=tip
                            on:click=move |ev: web_sys::MouseEvent| { ev.stop_propagation(); open_details(o2.clone()); }>{label}</button>
                    }
                })
                .collect_view();
            let k2 = k.clone();
            let k3 = k.clone();
            view! {
                <div class="ag-cell" class:out=day.month() != c.month() class:today=k == today on:click=move |_| { cursor.set(k2.clone()); mode.set("dia".into()); }>
                    <span class="dn">{day.day()}</span>
                    {chips}
                    {(n > 3).then(|| view! { <button class="ag-more" on:click=move |ev: web_sys::MouseEvent| { ev.stop_propagation(); cursor.set(k3.clone()); mode.set("dia".into()); }>{format!("+ {} mais", n - 3)}</button> })}
                </div>
            }
        })
        .collect_view();
    view! { <div class="ag-month">{heads}{cells}</div> }.into_any()
}

/* ---------------- week / day time grid ---------------- */

/// Places overlapping events side by side: (column, columns) per item.
fn layout(items: &[(i64, i64)]) -> Vec<(usize, usize)> {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    idx.sort_by_key(|i| (items[*i].0, -items[*i].1));
    let mut out = vec![(0usize, 1usize); items.len()];
    let mut cluster: Vec<usize> = Vec::new();
    let mut cols: Vec<i64> = Vec::new();
    let mut cluster_end = i64::MIN;
    let finish = |cluster: &mut Vec<usize>, cols: &mut Vec<i64>, out: &mut Vec<(usize, usize)>| {
        let n = cols.len().max(1);
        for i in cluster.drain(..) {
            out[i].1 = n;
        }
        cols.clear();
    };
    for i in idx {
        let (s, dur) = items[i];
        let e = s + dur.max(SNAP);
        if s >= cluster_end && !cluster.is_empty() {
            finish(&mut cluster, &mut cols, &mut out);
        }
        let col = match cols.iter().position(|end| *end <= s) {
            Some(c) => {
                cols[c] = e;
                c
            }
            None => {
                cols.push(e);
                cols.len() - 1
            }
        };
        out[i].0 = col;
        cluster.push(i);
        cluster_end = cluster_end.max(e);
    }
    finish(&mut cluster, &mut cols, &mut out);
    out
}

fn time_grid(
    days: Vec<String>,
    list: Vec<Occ>,
    drag: RwSignal<Option<Drag>>,
    mode: RwSignal<String>,
    cursor: RwSignal<String>,
) -> AnyView {
    let n = days.len();
    let today = today_key();
    let now_min = minutes_of(&michi_core::time::hhmm(now_ms()));
    let heads = days
        .iter()
        .map(|k| {
            let x = d(k);
            let k2 = k.clone();
            view! {
                <div class:today=*k == today on:click=move |_| { cursor.set(k2.clone()); mode.set("dia".into()); }>
                    {WD_PT[x.weekday().num_days_from_sunday() as usize]}<div class="hd">{x.day()}</div>
                </div>
            }
        })
        .collect_view();
    let all_day = days
        .iter()
        .map(|k| {
            let day = d(k);
            let chips = list
                .iter()
                .filter(|o| o.all_day && {
                    let s = d(&o.date);
                    day >= s && day <= s + Duration::days(((o.dur.max(1440) + 1439) / 1440) - 1)
                })
                .map(|o| {
                    let o2 = o.clone();
                    view! { <button class="ag-chip allday" class:done=o.done style=format!("--kc:{}", o.color) title=o.title.clone() on:click=move |_| open_details(o2.clone())>{o.title.clone()}</button> }
                })
                .collect_view();
            view! { <div>{chips}</div> }
        })
        .collect_view();
    let hours = (0..24)
        .map(|h| view! { <div>{if h == 0 { String::new() } else { format!("{h:02}:00") }}</div> })
        .collect_view();
    let cols = days
        .iter()
        .enumerate()
        .map(|(ci, k)| {
            let items: Vec<Occ> = list.iter().filter(|o| !o.all_day && o.date == *k).cloned().collect();
            let pos = layout(&items.iter().map(|o| (minutes_of(&o.time), o.dur)).collect::<Vec<_>>());
            let k2 = k.clone();
            let blocks = items
                .into_iter()
                .zip(pos)
                .map(|(o, (col, ncol))| event_block(o, col, ncol, ci, n, drag))
                .collect_view();
            let is_today = *k == today;
            view! {
                <div class="ag-col" class:today=is_today style=format!("height:{}px", HOUR_H * 24.0)
                    on:pointerdown=move |ev: web_sys::PointerEvent| {
                        let Some(t) = ev.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok()) else { return };
                        if !t.class_list().contains("ag-col") { return; }
                        let y = ev.offset_y() as f64;
                        let min = ((y / (HOUR_H / 60.0)) as i64 / 30) * 30;
                        open_editor(None, Some((k2.clone(), hhmm_of(min))));
                    }>
                    {blocks}
                    {is_today.then(|| view! { <div class="ag-now" style=format!("top:{}px", now_min as f64 * HOUR_H / 60.0)></div> })}
                </div>
            }
        })
        .collect_view();
    let scroll = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        if let Some(el) = scroll.get() {
            let h = (now_min / 60 - 2).clamp(0, 16).min(7.max(now_min / 60 - 2));
            el.set_scroll_top((h as f64 * HOUR_H) as i32);
        }
    });
    view! {
        <div class="ag-week" style=format!("--n:{n};--hh:{HOUR_H}px")>
            <div class="ag-head"><div></div>{heads}</div>
            <div class="ag-allday"><div>"dia todo"</div>{all_day}</div>
            <div class="ag-scroll" node_ref=scroll>
                <div class="ag-grid">
                    <div class="ag-hours">{hours}</div>
                    {cols}
                </div>
            </div>
        </div>
    }
    .into_any()
}

fn event_block(o: Occ, col: usize, ncol: usize, day_idx: usize, ndays: usize, drag: RwSignal<Option<Drag>>) -> AnyView {
    let key = o.key();
    let start = minutes_of(&o.time);
    let dur = o.dur;
    let key2 = key.clone();
    let style = move || {
        let dr = drag.get().filter(|d| d.key == key2);
        let (mut s, mut du, mut dd) = (start, dur, 0i64);
        if let Some(dr) = &dr {
            if dr.resize {
                du = (dur + dr.dmin).max(SNAP);
            } else {
                s = (start + dr.dmin).clamp(0, 24 * 60 - SNAP);
                dd = dr.dday.clamp(-(day_idx as i64), (ndays - 1 - day_idx) as i64);
            }
        }
        let w = 100.0 / ncol as f64;
        format!(
            "--kc:{};top:{}px;height:{}px;left:calc({}% + {}% * {});width:calc({}% - 3px)",
            o.color,
            s as f64 * HOUR_H / 60.0,
            (du.max(SNAP) as f64 * HOUR_H / 60.0 - 2.0).max(14.0),
            col as f64 * w,
            100.0,
            dd,
            w
        )
    };
    let key3 = key.clone();
    let time_label = move || {
        let dr = drag.get().filter(|d| d.key == key3);
        let (s, du) = match &dr {
            Some(d) if d.resize => (start, (dur + d.dmin).max(SNAP)),
            Some(d) => ((start + d.dmin).clamp(0, 24 * 60 - SNAP), dur),
            None => (start, dur),
        };
        format!("{}–{}", hhmm_of(s), hhmm_of(s + du))
    };
    let begin = move |ev: web_sys::PointerEvent, resize: bool, key: String| {
        ev.stop_propagation();
        ev.prevent_default();
        let colw = ev
            .target()
            .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok())
            .and_then(|t| t.closest(".ag-col").ok().flatten())
            .map(|c| c.get_bounding_client_rect().width())
            .unwrap_or(0.0);
        drag.set(Some(Drag {
            key,
            resize,
            sx: ev.client_x() as f64,
            sy: ev.client_y() as f64,
            colw,
            dmin: 0,
            dday: 0,
            moved: false,
        }));
    };
    let key4 = key.clone();
    let key5 = key.clone();
    let key6 = key.clone();
    view! {
        <div class="ag-ev" class:done=o.done class:dragging=move || drag.get().map(|d| d.key == key6 && d.moved).unwrap_or(false) style=style
            title=format!("{} · {}", o.title, o.kind.label())
            on:pointerdown=move |ev| begin(ev, false, key4.clone())>
            <b>{if o.jp.is_empty() { o.title.clone() } else { format!("{} {}", o.jp, o.title) }}</b>
            <span class="tm">{time_label}{if o.recurring { " ↻" } else { "" }}</span>
            <div class="rs" on:pointerdown=move |ev| begin(ev, true, key5.clone())></div>
        </div>
    }
    .into_any()
}

/// After a drag: one occurrence, or the whole series (asked for recurring events).
fn commit_drag(o: Occ, dr: &Drag) {
    let start = minutes_of(&o.time);
    let (new_start, new_dur, dday) = if dr.resize {
        (start, (o.dur + dr.dmin).max(SNAP), 0)
    } else {
        ((start + dr.dmin).clamp(0, 24 * 60 - SNAP), o.dur, dr.dday)
    };
    let new_date = key_add(&o.date, dday);
    if new_start == start && new_dur == o.dur && dday == 0 {
        return;
    }
    let new_time = hhmm_of(new_start);
    if !o.recurring {
        let (date, time) = (new_date, new_time);
        let id = o.event_id.clone();
        spawn_local(async move {
            if let Some(mut e) = event_get(&id).await {
                e.date = date;
                e.time = time;
                e.dur = new_dur;
                event_save(e, "");
            }
        });
        return;
    }
    // Recurring: ask like Google Calendar does.
    let title = "Mudar o horário";
    let resize = dr.resize;
    let o2 = o.clone();
    let (nd, nt) = (new_date.clone(), new_time.clone());
    detached(move || {
        let mid = StoredValue::new(0u32);
        let id = open_modal(title, false, move || {
            let (o3, o4) = (o2.clone(), o2.clone());
            let (nd, nt, nt2) = (nd.clone(), nt.clone(), nt.clone());
            let series_note = if dday != 0 {
                "Toda a série muda só o horário e a duração; para mudar os dias, use Editar."
            } else {
                ""
            };
            view! {
                <p>{format!("“{}” se repete. Aplicar {} · {} min a:", o2.title, nt.clone(), new_dur)}</p>
                <div class="row">
                    <button class="btn primary" on:click=move |_| { occ_move(&o3, nd.clone(), nt.clone(), new_dur); close_modal(mid.get_value()); }>"Só esta ocorrência"</button>
                    <button class="btn" on:click=move |_| { apply_series_time(&o4, (!resize).then_some(nt2.as_str()), new_dur); close_modal(mid.get_value()); }>"Toda a série"</button>
                </div>
                {(!series_note.is_empty()).then(|| view! { <div class="tiny faint">{series_note}</div> })}
            }
            .into_any()
        });
        mid.set_value(id);
    });
}

/// Series time/duration change: routine → the routine block; others → the
/// event. `time` None = keep the series' start (a resize).
fn apply_series_time(o: &Occ, time: Option<&str>, dur: i64) {
    if let Some(bid) = o.block_id() {
        let (bid, time) = (bid.to_string(), time.map(str::to_string));
        store::write(&["profile"], |s| {
            let mut r = routine_of(&s.p.routine);
            if let Some(b) = r.iter_mut().find(|b| b.id == bid) {
                if let Some(t) = time {
                    b.time = t;
                }
                b.min = dur as f64;
            }
            r.sort_by_key(|b| minutes_of(&b.time));
            s.p.routine = Some(r);
        });
        store::flush();
        toast("Rotina atualizada.", "ok");
        return;
    }
    let (id, time) = (o.event_id.clone(), time.map(str::to_string));
    spawn_local(async move {
        if let Some(mut e) = event_get(&id).await {
            if let Some(t) = time {
                e.time = t;
            }
            e.dur = dur;
            event_save(e, "Série atualizada.");
        }
    });
}

/* ---------------- list ---------------- */

fn list_view(list: Vec<Occ>, list_days: RwSignal<i64>) -> AnyView {
    if list.is_empty() {
        return view! {
            <div class="empty">"Nada marcado nesses dias."</div>
            <button class="btn sm" style="align-self:flex-start" on:click=move |_| list_days.update(|d| *d += 14)>"Mostrar mais 14 dias"</button>
        }
        .into_any();
    }
    let mut groups: Vec<(String, Vec<Occ>)> = Vec::new();
    for o in list {
        match groups.last_mut() {
            Some((k, l)) if *k == o.date => l.push(o),
            _ => groups.push((o.date.clone(), vec![o])),
        }
    }
    let cards = groups
        .into_iter()
        .map(|(k, l)| {
            let rows = l
                .into_iter()
                .map(|o| {
                    let o2 = o.clone();
                    let o3 = o.clone();
                    let when = if o.all_day { "dia todo".to_string() } else { o.time.clone() };
                    let sub = if o.all_day { o.notes.clone() } else { format!("{} min{}", o.dur, if o.notes.is_empty() { String::new() } else { format!(" · {}", o.notes) }) };
                    view! {
                        <div class="ag-row" class:done=o.done style=format!("--kc:{};cursor:pointer", o.color) on:click=move |_| open_details(o3.clone())>
                            <div class="ag-t num">{when}</div>
                            <div class="grow" style="min-width:0">
                                <div class="ag-title"><span class="jp faint">{o.jp.clone()}</span>" "{o.title.clone()}{o.recurring.then_some(" ↻")}</div>
                                <div class="tiny muted">{sub}</div>
                            </div>
                            <span class="pill" style=format!("color:{};", o.color)>{o.kind.label()}</span>
                            <label class="switch small" title="Marcar como feito" on:click=|ev: web_sys::MouseEvent| ev.stop_propagation()>
                                <input type="checkbox" prop:checked=o.done on:change=move |ev| mark(&o2, ev_checked(&ev))/>" feito"
                            </label>
                        </div>
                    }
                })
                .collect_view();
            view! { <div class="card stack ag-day"><div class="eyebrow">{day_title(&k)}</div>{rows}</div> }
        })
        .collect_view();
    view! {
        {cards}
        <div class="row"><button class="btn sm" on:click=move |_| list_days.update(|d| *d += 14)>"Mostrar mais 14 dias"</button></div>
    }
    .into_any()
}

/* ---------------- details ---------------- */

pub fn open_details(o: Occ) {
    let title = format!(
        r#"<span class="ag-dot" style="--kc:{}"></span>{}"#,
        esc(&o.color),
        esc(&if o.jp.is_empty() { o.title.clone() } else { format!("{} {}", o.jp, o.title) })
    );
    detached(move || {
        let mid = StoredValue::new(0u32);
        let ev = RwSignal::new(None::<Event>);
        let id2 = o.event_id.clone();
        spawn_local(async move {
            let e = event_get(&id2).await;
            if ev.try_get_untracked().is_some() {
                ev.set(e);
            }
        });
        let moving = RwSignal::new(false);
        let id = open_modal(&title, false, move || {
            let o = o.clone();
            let close = move || close_modal(mid.get_value());
            let rule = ev.get().and_then(|e| e.rule()).map(|r| r.describe());
            let rem = if o.reminders.is_empty() {
                "sem lembrete".to_string()
            } else {
                o.reminders.iter().map(|m| reminder_label(*m)).collect::<Vec<_>>().join(", ")
            };
            let kind_line = match o.source.as_str() {
                "rotina" => "Bloco da rotina".to_string(),
                "viagem" => "Viagem ao Japão".to_string(),
                _ => o.kind.label().to_string(),
            };
            let (o_done, o_edit, o_skip, o_del, o_go, o_mv) =
                (o.clone(), o.clone(), o.clone(), o.clone(), o.clone(), o.clone());
            let user = o.source == "user";
            view! {
                <div class="stack" style="gap:6px">
                    <div class="small muted">{kind_line}</div>
                    <div style="font-weight:650">{when_of(&o)}{o.moved.then_some(" (movida)")}</div>
                    {rule.map(|r| view! { <div class="small">{format!("↻ {r}")}</div> })}
                    <div class="small">{format!("🔔 {rem}")}</div>
                    {(!o.notes.is_empty()).then(|| view! { <div class="small muted">{o.notes.clone()}</div> })}
                </div>
                <div class="row">
                    <button class="btn sm" class:primary=!o.done on:click=move |_| { mark(&o_done, !o_done.done); close(); }>
                        {if o.done { "↺ Desmarcar" } else { "✓ Marcar como feito" }}
                    </button>
                    {o.go.clone().map(|g| view! { <button class="btn sm" on:click=move |_| { close(); go(&g, intent_for_block(o_go.block_id())); }>"↗ Abrir"</button> })}
                    <button class="btn sm" on:click=move |_| { close(); open_editor(Some(o_edit.clone()), None); }>"✏️ Editar"</button>
                    {(!o.all_day).then(|| view! { <button class="btn sm" on:click=move |_| moving.set(!moving.get_untracked())>"📅 Mover esta"</button> })}
                    {o.recurring.then(|| view! { <button class="btn sm ghost" on:click=move |_| { occ_skip(&o_skip); close(); }>"⏭ Pular esta"</button> })}
                    {user.then(|| view! { <button class="btn sm danger" on:click=move |_| {
                        let id = o_del.event_id.clone();
                        spawn_local(async move { after(tauri::invoke::<(), _>("event_delete", &IdArg { id }).await, "Evento apagado.") });
                        close();
                    }>{if o.recurring { "🗑 Apagar a série" } else { "🗑 Apagar" }}</button> })}
                </div>
                {move || moving.get().then(|| {
                    let date = RwSignal::new(o_mv.date.clone());
                    let time = RwSignal::new(o_mv.time.clone());
                    let dur = RwSignal::new(o_mv.dur.to_string());
                    let o5 = o_mv.clone();
                    view! {
                        <div class="card tint row">
                            <input type="date" prop:value=date.get_untracked() on:change=move |ev| date.set(ev_value(&ev))/>
                            <input type="time" prop:value=time.get_untracked() on:change=move |ev| time.set(ev_value(&ev))/>
                            <label class="rt-min"><input type="number" min="5" max="600" step="5" prop:value=dur.get_untracked() on:change=move |ev| dur.set(ev_value(&ev))/><span class="tiny faint">"min"</span></label>
                            <button class="btn sm primary" on:click=move |_| {
                                occ_move(&o5, date.get_untracked(), time.get_untracked(), dur.get_untracked().parse().unwrap_or(o5.dur));
                                close();
                            }>"Mover só esta"</button>
                        </div>
                    }
                })}
            }
            .into_any()
        });
        mid.set_value(id);
    });
}

/* ---------------- editor ---------------- */

#[derive(Clone, PartialEq)]
struct Form {
    title: String,
    kind: Kind,
    all_day: bool,
    date: String,
    time: String,
    dur: i64,
    /// none | daily | weekdays | weekly | monthly | yearly
    rep: String,
    days: Vec<u8>,
    /// never | until | count
    ends: String,
    until: String,
    count: u32,
    reminders: Vec<i64>,
    notes: String,
    go: String,
}

fn form_of(e: &Event) -> Form {
    let r = e.rule();
    let rep = match &r {
        None => "none",
        Some(r) => match r.freq {
            Freq::Daily => "daily",
            Freq::Weekly => {
                let set: Vec<u32> = r.byday.iter().map(|w| w.num_days_from_monday()).collect();
                if set == [0, 1, 2, 3, 4] {
                    "weekdays"
                } else {
                    "weekly"
                }
            }
            Freq::Monthly => "monthly",
            Freq::Yearly => "yearly",
        },
    }
    .to_string();
    let days: Vec<u8> = r.as_ref().map(|r| r.byday.iter().map(|w| js_of_weekday(*w)).collect()).unwrap_or_default();
    let days = if days.is_empty() { vec![michi_core::time::dow(d(&e.date))] } else { days };
    Form {
        title: e.title.clone(),
        kind: e.kind,
        all_day: e.all_day,
        date: e.date.clone(),
        time: if e.time.is_empty() { "19:00".into() } else { e.time.clone() },
        dur: e.dur,
        rep,
        days,
        ends: if r.as_ref().and_then(|r| r.until).is_some() {
            "until".into()
        } else if r.as_ref().and_then(|r| r.count).is_some() {
            "count".into()
        } else {
            "never".into()
        },
        until: r.as_ref().and_then(|r| r.until).map(key_of).unwrap_or_default(),
        count: r.as_ref().and_then(|r| r.count).unwrap_or(10),
        reminders: e.reminders.clone(),
        notes: e.notes.clone(),
        go: e.go.clone().unwrap_or_default(),
    }
}

fn rule_of(f: &Form) -> Option<String> {
    let mut r = match f.rep.as_str() {
        "daily" => RRule::daily(),
        "weekdays" => RRule::for_js_days(&[1, 2, 3, 4, 5]),
        "weekly" => {
            let mut r = RRule::for_js_days(&f.days);
            r.freq = Freq::Weekly;
            if r.byday.is_empty() {
                r.byday = f.days.iter().map(|x| weekday_of_js(*x)).collect();
            }
            r
        }
        "monthly" => RRule { freq: Freq::Monthly, interval: 1, byday: vec![], until: None, count: None },
        "yearly" => RRule { freq: Freq::Yearly, interval: 1, byday: vec![], until: None, count: None },
        _ => return None,
    };
    if f.rep == "weekly" && f.days.len() == 7 {
        r.byday = WEEK_ORDER.iter().map(|x| weekday_of_js(*x)).collect::<Vec<Weekday>>();
    }
    match f.ends.as_str() {
        "until" => r.until = parse_key(&f.until),
        "count" => r.count = Some(f.count.max(1)),
        _ => {}
    }
    Some(r.to_string())
}

const ROUTES_GO: [(&str, &str); 9] = [
    ("", "—"),
    ("hoje", "Hoje"),
    ("revisar", "Revisar"),
    ("jornada", "Jornada"),
    ("jogos", "Jogos"),
    ("frases", "Frases"),
    ("imersao", "Imersão"),
    ("anki", "Anki"),
    ("agenda", "Agenda"),
];
const REMINDER_OPTS: [i64; 8] = [0, 5, 10, 15, 30, 60, 120, 1440];

/// New event (`at` = date + time) or editing the occurrence's series.
pub fn open_editor(o: Option<Occ>, at: Option<(String, String)>) {
    let title = if o.is_some() { "✏️ Editar" } else { "＋ Novo evento" };
    detached(move || {
        let mid = StoredValue::new(0u32);
        let loaded = RwSignal::new(None::<(Option<Event>, Form)>);
        let o2 = o.clone();
        spawn_local(async move {
            let (ev, form) = match &o2 {
                Some(o) => match event_get(&o.event_id).await {
                    Some(e) => {
                        let f = form_of(&e);
                        (Some(e), f)
                    }
                    None => return,
                },
                None => {
                    let (date, time) = at.unwrap_or_else(|| (today_key(), "19:00".into()));
                    let dow = michi_core::time::dow(d(&date));
                    (
                        None,
                        Form {
                            title: String::new(),
                            kind: Kind::Estudo,
                            all_day: false,
                            date,
                            time,
                            dur: 30,
                            rep: "none".into(),
                            days: vec![dow],
                            ends: "never".into(),
                            until: String::new(),
                            count: 10,
                            reminders: vec![10],
                            notes: String::new(),
                            go: String::new(),
                        },
                    )
                }
            };
            if loaded.try_get_untracked().is_some() {
                loaded.set(Some((ev, form)));
            }
        });
        let id = open_modal(title, false, move || {
            let Some((ev, form)) = loaded.get() else {
                return view! { <div class="loading"><div class="spin"></div></div> }.into_any();
            };
            let close = move || close_modal(mid.get_value());
            match ev.as_ref().map(|e| e.source.as_str()) {
                Some("rotina") => routine_editor(ev.unwrap(), close),
                _ => event_editor(ev, form, close),
            }
        });
        mid.set_value(id);
    });
}

fn event_editor(ev: Option<Event>, form: Form, close: impl Fn() + Copy + Send + Sync + 'static) -> AnyView {
    let trip = ev.as_ref().map(|e| e.source == "viagem").unwrap_or(false);
    let f = RwSignal::new(form);
    let ev = StoredValue::new(ev);
    let set = move |g: Box<dyn Fn(&mut Form)>| f.update(|x| g(x));
    let kinds = [Kind::Estudo, Kind::Vlog, Kind::Meu]
        .into_iter()
        .map(|k| view! { <option value=k.css() selected=f.get_untracked().kind == k>{k.label()}</option> })
        .collect_view();
    let reps = [
        ("none", "Não repete"),
        ("daily", "Todo dia"),
        ("weekdays", "Dias úteis (seg a sex)"),
        ("weekly", "Toda semana, nos dias…"),
        ("monthly", "Todo mês"),
        ("yearly", "Todo ano"),
    ]
    .into_iter()
    .map(|(v, l)| view! { <option value=v selected=f.get_untracked().rep == v>{l}</option> })
    .collect_view();
    let gos = ROUTES_GO
        .iter()
        .map(|(v, l)| view! { <option value=*v selected=f.get_untracked().go == *v>{*l}</option> })
        .collect_view();
    let save = move |_| {
        let x = f.get_untracked();
        if x.title.trim().is_empty() {
            toast("Dê um título ao evento.", "");
            return;
        }
        if x.ends == "until" && parse_key(&x.until).map(|u| u < d(&x.date)).unwrap_or(true) {
            toast("A data final precisa ser depois do início.", "");
            return;
        }
        let mut e = ev.get_value().unwrap_or(Event {
            id: String::new(),
            kind: x.kind,
            title: String::new(),
            jp: String::new(),
            notes: String::new(),
            all_day: false,
            date: x.date.clone(),
            time: String::new(),
            dur: 30,
            rrule: None,
            exdates: vec![],
            reminders: vec![],
            color: None,
            go: None,
            source: "user".into(),
            source_id: None,
            overrides: vec![],
            off: false,
        });
        e.title = x.title.trim().to_string();
        e.notes = x.notes.trim().to_string();
        e.reminders = x.reminders.clone();
        if !trip {
            e.kind = x.kind;
            e.all_day = x.all_day;
            e.date = x.date.clone();
            e.time = if x.all_day { String::new() } else { x.time.clone() };
            e.dur = if x.all_day { 1440 } else { x.dur.max(5) };
            e.rrule = rule_of(&x);
            e.go = if x.go.is_empty() { None } else { Some(x.go.clone()) };
        }
        event_save(e, "Evento salvo.");
        close();
    };
    view! {
        <div class="stack">
            <div class="field"><label>"Título"</label>
                <input type="text" maxlength="80" placeholder="ex.: Aula de conversação, Genki L5, revisão do kanji" prop:value=f.get_untracked().title
                    on:input=move |e| { let v = ev_value(&e); set(Box::new(move |x| x.title = v.clone())); }/></div>
            {(!trip).then(|| view! {
                <div class="grid2">
                    <div class="field"><label>"Tipo"</label>
                        <select on:change=move |e| { let v = Kind::parse(&ev_value(&e)); set(Box::new(move |x| x.kind = v)); }>{kinds}</select></div>
                    <div class="field"><label>"Data"</label>
                        <input type="date" prop:value=f.get_untracked().date on:change=move |e| {
                            let v = ev_value(&e);
                            if !v.is_empty() {
                                set(Box::new(move |x| {
                                    // A single repeat day follows the start date (like Google Calendar).
                                    if x.days.len() <= 1 { x.days = vec![michi_core::time::dow(d(&v))]; }
                                    x.date = v.clone();
                                }));
                            }
                        }/></div>
                </div>
                <label class="switch small"><input type="checkbox" prop:checked=f.get_untracked().all_day on:change=move |e| { let v = ev_checked(&e); set(Box::new(move |x| x.all_day = v)); }/>" Dia todo"</label>
                {move || (!f.get().all_day).then(|| view! {
                    <div class="row">
                        <div class="field"><label>"Início"</label><input type="time" prop:value=f.get_untracked().time on:change=move |e| { let v = ev_value(&e); if !v.is_empty() { set(Box::new(move |x| x.time = v.clone())); } }/></div>
                        <div class="field"><label>"Duração (min)"</label><input type="number" min="5" max="600" step="5" style="width:100px" prop:value=f.get_untracked().dur.to_string()
                            on:change=move |e| { let v = ev_value(&e).parse::<i64>().unwrap_or(30).clamp(5, 600); set(Box::new(move |x| x.dur = v)); }/></div>
                        <div class="small muted" style="align-self:flex-end;padding-bottom:10px">{move || { let x = f.get(); format!("até {}", hhmm_of(minutes_of(&x.time) + x.dur)) }}</div>
                    </div>
                })}
                <div class="field"><label>"Repetição"</label>
                    <select on:change=move |e| { let v = ev_value(&e); set(Box::new(move |x| x.rep = v.clone())); }>{reps}</select></div>
                {move || (f.get().rep == "weekly").then(|| {
                    let btns = WEEK_ORDER.iter().map(|w| {
                        let w = *w;
                        view! { <button type="button" aria-pressed=move || f.get().days.contains(&w).to_string() title=WEEK[w as usize] on:click=move |_| {
                            set(Box::new(move |x| { if x.days.contains(&w) { if x.days.len() > 1 { x.days.retain(|d| *d != w); } } else { x.days.push(w); x.days.sort_unstable(); } }));
                        }>{WEEK1[w as usize]}</button> }
                    }).collect_view();
                    view! { <div class="row"><div class="ag-days" role="group">{btns}</div><span class="tiny muted">{move || days_label(&f.get().days)}</span></div> }
                })}
                {move || (f.get().rep != "none").then(|| view! {
                    <div class="row">
                        <label class="small">"Termina"</label>
                        <select on:change=move |e| { let v = ev_value(&e); set(Box::new(move |x| x.ends = v.clone())); }>
                            <option value="never" selected=f.get_untracked().ends == "never">"nunca"</option>
                            <option value="until" selected=f.get_untracked().ends == "until">"em uma data"</option>
                            <option value="count" selected=f.get_untracked().ends == "count">"depois de N vezes"</option>
                        </select>
                        {move || match f.get().ends.as_str() {
                            "until" => view! { <input type="date" prop:value=f.get_untracked().until on:change=move |e| { let v = ev_value(&e); set(Box::new(move |x| x.until = v.clone())); }/> }.into_any(),
                            "count" => view! { <input type="number" min="1" max="500" style="width:90px" prop:value=f.get_untracked().count.to_string() on:change=move |e| { let v = ev_value(&e).parse().unwrap_or(10); set(Box::new(move |x| x.count = v)); }/> }.into_any(),
                            _ => view! { <span></span> }.into_any(),
                        }}
                    </div>
                })}
            })}
            <div class="field"><label>"Lembretes"</label>
                <div class="row">
                    {move || f.get().reminders.into_iter().map(|m| view! {
                        <span class="pill ag-rem">{format!("🔔 {}", reminder_label(m))}
                            <button class="btn xs ghost" aria-label="Tirar" on:click=move |_| set(Box::new(move |x| x.reminders.retain(|r| *r != m)))>"✕"</button></span>
                    }).collect_view()}
                    <select on:change=move |e| {
                        let v: i64 = ev_value(&e).parse().unwrap_or(-1);
                        if v >= 0 { set(Box::new(move |x| if !x.reminders.contains(&v) { x.reminders.push(v); x.reminders.sort_unstable(); })); }
                        if let Some(t) = e.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlSelectElement>(t).ok()) { t.set_value("-1"); }
                    }>
                        <option value="-1">"＋ adicionar lembrete"</option>
                        {REMINDER_OPTS.iter().map(|m| view! { <option value=m.to_string()>{reminder_label(*m)}</option> }).collect_view()}
                    </select>
                </div>
                {f.get_untracked().all_day.then(|| view! { <span class="tiny faint">"Eventos de dia todo avisam em relação às 09:00 do dia."</span> })}
            </div>
            <div class="field"><label>"Notas"</label>
                <textarea style="min-height:60px" prop:value=f.get_untracked().notes on:input=move |e| { let v = ev_value(&e); set(Box::new(move |x| x.notes = v.clone())); }></textarea></div>
            {(!trip).then(|| view! {
                <div class="field"><label>"Botão Abrir leva para"</label>
                    <select on:change=move |e| { let v = ev_value(&e); set(Box::new(move |x| x.go = v.clone())); }>{gos}</select></div>
            })}
            <div class="row"><button class="btn primary" on:click=save>"Salvar"</button><button class="btn ghost" on:click=move |_| close()>"Cancelar"</button></div>
        </div>
    }
    .into_any()
}

/// Routine series are edited as routine blocks (same as Ajustes → Rotina).
fn routine_editor(e: Event, close: impl Fn() + Copy + Send + Sync + 'static) -> AnyView {
    let bid = e.source_id.clone().unwrap_or_default();
    let Some(block) = routine_of(&store::read(|s| s.p.routine.clone())).into_iter().find(|b| b.id == bid) else {
        return view! { <div class="empty">"Bloco não encontrado."</div> }.into_any();
    };
    let b = RwSignal::new(Block { days: Some(block.days_of()), ..block });
    let remind = store::read(|s| s.p.remind_before());
    let save = move |_| {
        let nb = b.get_untracked();
        store::write(&["profile"], |s| {
            let mut r = routine_of(&s.p.routine);
            if let Some(x) = r.iter_mut().find(|x| x.id == nb.id) {
                *x = nb.clone();
            }
            r.sort_by_key(|x| minutes_of(&x.time));
            if nb.id == "vlog" && !nb.off {
                s.p.vlog_time = nb.time.clone();
            }
            s.p.routine = Some(r);
        });
        store::flush();
        toast("Rotina atualizada.", "ok");
        close();
    };
    let days = WEEK_ORDER
        .iter()
        .map(|w| {
            let w = *w;
            view! { <button type="button" aria-pressed=move || b.get().on_day(w).to_string() title=WEEK[w as usize] on:click=move |_| b.update(|x| {
                let mut d = x.days_of();
                if d.contains(&w) { if d.len() > 1 { d.retain(|y| *y != w); } } else { d.push(w); d.sort_unstable(); }
                x.days = Some(d);
            })>{WEEK1[w as usize]}</button> }
        })
        .collect_view();
    view! {
        <div class="stack">
            <div class="small muted">"Este evento é um bloco da rotina: mudar aqui muda a rotina inteira (o quadro de Hoje, a agenda e os avisos)."</div>
            <div class="field"><label>"Nome"</label><input type="text" maxlength="40" prop:value=b.get_untracked().name on:input=move |e| { let v = ev_value(&e); b.update(|x| x.name = v); }/></div>
            <div class="row">
                <div class="field"><label>"Início"</label><input type="time" prop:value=b.get_untracked().time on:change=move |e| { let v = ev_value(&e); if !v.is_empty() { b.update(|x| x.time = v); } }/></div>
                <div class="field"><label>"Duração (min)"</label><input type="number" min="5" max="180" step="5" style="width:100px" prop:value=b.get_untracked().min.to_string()
                    on:change=move |e| { let v = ev_value(&e).parse::<f64>().unwrap_or(15.0).clamp(5.0, 180.0); b.update(|x| x.min = v); }/></div>
            </div>
            <div class="row"><div class="ag-days" role="group">{days}</div><span class="tiny muted">{move || days_label(&b.get().days_of())}</span></div>
            <label class="switch small"><input type="checkbox" prop:checked=b.get_untracked().core on:change=move |e| { let v = ev_checked(&e); b.update(|x| x.core = v); }/>" Essencial (continua no dia mínimo)"</label>
            <label class="switch small"><input type="checkbox" prop:checked=!b.get_untracked().off on:change=move |e| { let v = ev_checked(&e); b.update(|x| x.off = !v); }/>" Bloco ligado"</label>
            <div class="tiny faint">{format!("Lembrete da rotina: {} (muda em Ajustes → Lembretes).", reminder_label(remind))}</div>
            <div class="row"><button class="btn primary" on:click=save>"Salvar"</button><button class="btn ghost" on:click=move |_| close()>"Cancelar"</button></div>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    #[test]
    fn overlapping_events_share_the_width() {
        // 09:00–10:00, 09:30–10:30, 11:00–11:30
        let l = super::layout(&[(540, 60), (570, 60), (660, 30)]);
        assert_eq!(l[0], (0, 2));
        assert_eq!(l[1], (1, 2));
        assert_eq!(l[2], (0, 1));
    }
}
