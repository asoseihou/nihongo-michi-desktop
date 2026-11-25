//! Agenda — phase 1 shows the list view (next days, mark as done). The
//! month/week/day views, editing, drag and resize arrive in phase 3.

use crate::app::toast;
use crate::store::{self, sigs};
use crate::tauri;
use leptos::prelude::*;
use michi_core::agenda::{Kind, Occ};
use michi_core::ops::Op;
use michi_core::time::{key_add, parse_key, today_key, MONTH_PT_LONG, WD_PT};
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

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

pub fn mark(o: &Occ, done: bool) {
    let args = Mark { event_id: o.event_id.clone(), occ: o.occ.clone(), done };
    spawn_local(async move {
        match tauri::invoke::<Vec<Op>, _>("occ_mark", &args).await {
            // The backend also sends `michi://patch`, which updates the store.
            Ok(_) => {}
            Err(e) => toast(&format!("Não consegui marcar: {e}"), "bad"),
        }
    });
}

pub fn day_title(k: &str) -> String {
    use chrono::Datelike;
    let Some(d) = parse_key(k) else { return k.to_string() };
    let rel = if k == today_key() {
        "Hoje · ".to_string()
    } else if k == key_add(&today_key(), 1) {
        "Amanhã · ".to_string()
    } else {
        String::new()
    };
    format!(
        "{rel}{}, {} de {}",
        WD_PT[d.weekday().num_days_from_sunday() as usize],
        d.day(),
        MONTH_PT_LONG[d.month0() as usize]
    )
}

#[component]
pub fn Agenda() -> impl IntoView {
    let s = sigs();
    let days = RwSignal::new(14i64);
    let occs = RwSignal::new(None::<Vec<Occ>>);
    let load = move || {
        let from = today_key();
        let to = key_add(&from, days.get_untracked() - 1);
        spawn_local(async move {
            match tauri::invoke::<Vec<Occ>, _>("agenda_range", &Range { from, to }).await {
                Ok(v) => occs.set(Some(v)),
                Err(e) => {
                    occs.set(Some(vec![]));
                    toast(&format!("Não consegui ler a agenda: {e}"), "bad");
                }
            }
        });
    };
    // Reload whenever data changes (marks from here, the popup or the board).
    Effect::new(move |_| {
        s.view.get();
        days.get();
        load();
    });
    let legend = Kind::ALL
        .iter()
        .map(|k| view! { <span class="ag-leg" style=format!("--kc:{}", k.color())><i></i>{k.label()}</span> })
        .collect_view();
    view! {
        <div class="row-between">
            <h1>"Agenda"</h1>
            <div class="row"><span class="pill">"Lista"</span><span class="tiny faint">"Mês, semana e dia chegam na fase 3"</span></div>
        </div>
        <div class="row ag-legend">{legend}</div>
        {move || match occs.get() {
            None => view! { <div class="loading"><div class="spin"></div>"Carregando a agenda…"</div> }.into_any(),
            Some(v) if v.is_empty() => view! { <div class="empty">"Nada marcado nos próximos dias."</div> }.into_any(),
            Some(v) => {
                let mut groups: Vec<(String, Vec<Occ>)> = Vec::new();
                for o in v {
                    match groups.last_mut() {
                        Some((d, list)) if *d == o.date => list.push(o),
                        _ => groups.push((o.date.clone(), vec![o])),
                    }
                }
                groups.into_iter().map(|(d, list)| {
                    let rows = list.into_iter().map(|o| {
                        let o2 = o.clone();
                        let done = o.done;
                        let when = if o.all_day { "dia todo".to_string() } else { o.time.clone() };
                        let sub = if o.all_day { o.notes.clone() } else { format!("{} min{}", o.dur, if o.notes.is_empty() { String::new() } else { format!(" · {}", o.notes) }) };
                        view! {
                            <div class="ag-row" class:done=done style=format!("--kc:{}", o.color)>
                                <div class="ag-t num">{when}</div>
                                <div class="grow" style="min-width:0">
                                    <div class="ag-title"><span class="jp faint">{o.jp.clone()}</span>" "{o.title.clone()}{o.recurring.then_some(" ↻")}</div>
                                    <div class="tiny muted">{sub}</div>
                                </div>
                                <span class="pill" style=format!("color:{};", o.color)>{o.kind.label()}</span>
                                <label class="switch small" title="Marcar como feito">
                                    <input type="checkbox" prop:checked=done on:change=move |ev| mark(&o2, crate::util::ev_checked(&ev))/>
                                    " feito"
                                </label>
                            </div>
                        }
                    }).collect_view();
                    view! {
                        <div class="card stack ag-day">
                            <div class="eyebrow">{day_title(&d)}</div>
                            {rows}
                        </div>
                    }
                }).collect_view().into_any()
            }
        }}
        <div class="row">
            <button class="btn sm" on:click=move |_| days.update(|d| *d += 14)>"Mostrar mais 14 dias"</button>
            <span class="tiny faint">{move || { store::read(|_| ()); format!("{} dias", days.get()) }}</span>
        </div>
    }
}
