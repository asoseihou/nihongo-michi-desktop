//! Focus mode in the main window: the card on Today (start a free session,
//! or follow the running one) and the helpers the board rows and the agenda
//! use to start a session for a block or an item.

use crate::app::{focus_sig, toast};
use crate::tauri::{self, NoArgs};
use crate::util::ev_value;
use leptos::prelude::*;
use michi_core::agenda::Kind;
use michi_core::focus::{clock, Focus, Spec};
use michi_core::time::now_ms;
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

#[derive(Serialize)]
struct SpecArg {
    spec: Spec,
}

/// Starts a session (replacing a running one: its minutes are counted).
pub fn start(spec: Spec) {
    spawn_local(async move {
        match tauri::invoke::<Focus, _>("foco_start", &SpecArg { spec }).await {
            Ok(f) => {
                focus_sig().set(Some(f.clone()));
                toast(
                    &format!(
                        "Foco de {} min: {}. A janelinha fica no canto da tela.",
                        (f.planned_ms / 60_000.0).round(),
                        f.title
                    ),
                    "ok",
                );
            }
            Err(e) => toast(&format!("Não consegui começar o foco: {e}"), "bad"),
        }
    });
}

/// A routine block of the Today board.
pub fn start_block(id: &str, name: &str, jp: &str, minutes: f64) {
    start(Spec {
        title: name.to_string(),
        jp: jp.to_string(),
        color: Kind::Rotina.color().to_string(),
        minutes: if minutes > 0.0 { minutes } else { 25.0 },
        block: Some(id.to_string()),
        ..Default::default()
    });
}

/// An agenda occurrence.
pub fn start_occ(o: &michi_core::agenda::Occ) {
    let block = o.block_id().map(str::to_string);
    start(Spec {
        title: o.title.clone(),
        jp: o.jp.clone(),
        color: o.color.clone(),
        minutes: if o.dur > 0 && !o.all_day { o.dur as f64 } else { 25.0 },
        event_id: if block.is_some() { None } else { Some(o.event_id.clone()) },
        occ: if block.is_some() { None } else { Some(o.occ.clone()) },
        block,
        ..Default::default()
    });
}

fn call(cmd: &'static str) {
    tauri::call(cmd, NoArgs {});
}

#[component]
pub fn FocusCard() -> impl IntoView {
    let sig = focus_sig();
    let now = RwSignal::new(now_ms());
    let h = set_interval_with_handle(move || now.set(now_ms()), std::time::Duration::from_millis(1000)).ok();
    on_cleanup(move || {
        if let Some(h) = h {
            h.clear();
        }
    });
    let what = RwSignal::new(String::new());
    let mins = RwSignal::new(25.0);
    move || match sig.get().filter(Focus::active) {
        Some(f) => {
            let paused = f.state == "paused";
            let (f2, f3) = (f.clone(), f.clone());
            view! {
                <div class="card focus-card running stack" style=format!("--kc:{}", if f.color.is_empty() { "#0a72c0" } else { f.color.as_str() })>
                    <div class="row-between">
                        <div class="eyebrow">"集中 Modo foco"</div>
                        <span class="pill line">{if paused { "pausado" } else { "contando" }}</span>
                    </div>
                    <div class="row">
                        <div class="grow"><b>{f.title.clone()}</b>" "<span class="faint small">{f.jp.clone()}</span></div>
                        <div class="focus-time num">{move || clock(f2.remaining_ms(now.get()))}</div>
                    </div>
                    <div class="bar"><i style=move || {
                        let p = if f3.planned_ms > 0.0 { f3.elapsed_ms(now.get()) / f3.planned_ms } else { 0.0 };
                        format!("width:{:.1}%", (p * 100.0).clamp(0.0, 100.0))
                    }></i></div>
                    <div class="row">
                        {if paused {
                            view! { <button class="btn sm primary" on:click=|_| call("foco_resume")>"▶ Continuar"</button> }.into_any()
                        } else {
                            view! { <button class="btn sm" on:click=|_| call("foco_pause")>"⏸ Pausar"</button> }.into_any()
                        }}
                        <button class="btn sm" on:click=|_| tauri::call("foco_add", AddArg { minutes: 5.0 })>"+5 min"</button>
                        <button class="btn sm" on:click=|_| call("foco_stop")>"■ Encerrar e contar"</button>
                        <button class="btn sm ghost" on:click=|_| call("foco_show")>"Mostrar a janelinha"</button>
                    </div>
                </div>
            }
            .into_any()
        }
        None => {
            let chips = [15.0, 25.0, 45.0, 60.0]
                .into_iter()
                .map(|m| {
                    view! {
                        <button class="chip-btn" class:on=move || mins.get() == m on:click=move |_| mins.set(m)>{format!("{m} min")}</button>
                    }
                })
                .collect_view();
            view! {
                <div class="card focus-card stack">
                    <div class="row-between">
                        <div class="eyebrow">"集中 Modo foco"</div>
                        <span class="tiny faint">"o ▶ de cada bloco do quadro também começa um foco"</span>
                    </div>
                    <div class="small muted">"Um cronômetro com uma janelinha sempre visível, para estudar fora do app (Genki, Wagotabi, caderno). Os minutos contam no quadro de hoje, na meta e no mapa de calor; cada 3 minutos dão 1 XP. Os avisos esperam o foco acabar."</div>
                    <div class="row">{chips}</div>
                    <div class="row">
                        <input type="text" class="grow" maxlength="40" placeholder="O que vai estudar? (opcional)" prop:value=move || what.get()
                            on:input=move |ev| what.set(ev_value(&ev))/>
                        <button class="btn sm primary" on:click=move |_| {
                            start(Spec { title: what.get_untracked(), minutes: mins.get_untracked(), ..Default::default() });
                            what.set(String::new());
                        }>"▶ Começar"</button>
                    </div>
                </div>
            }
            .into_any()
        }
    }
}

#[derive(Serialize)]
struct AddArg {
    minutes: f64,
}
