//! The reminder popup window ("aviso"): a small always-on-top departure
//! board in the screen corner with Feito / Adiar / Abrir. The backend
//! places it; this view reports its height after every redraw.

use crate::tauri::{self, NoArgs};
use crate::util::{by_id, document};
use leptos::prelude::*;
use michi_core::agenda::Aviso;
use michi_core::time::{hhmm, local, now_ms};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;

#[derive(Deserialize, Clone, Debug, Default)]
struct AvisoState {
    active: Vec<Aviso>,
    missed: Vec<Aviso>,
    snooze_min: f64,
}

#[derive(Serialize)]
struct Fit {
    height: f64,
}

#[derive(Serialize)]
struct Action {
    id: i64,
    action: &'static str,
    minutes: Option<f64>,
}

#[derive(Serialize)]
struct Open {
    open: bool,
}

fn fit_later() {
    set_timeout(
        || {
            let h = by_id("aviso-root").map(|e| e.scroll_height() as f64).unwrap_or(0.0);
            tauri::call("aviso_fit", Fit { height: h });
        },
        std::time::Duration::from_millis(40),
    );
}

fn act(id: i64, action: &'static str, minutes: Option<f64>) {
    tauri::call("aviso_action", Action { id, action, minutes });
}

#[component]
pub fn AvisoApp() -> impl IntoView {
    if let Some(el) = document().document_element() {
        let _ = el.class_list().add_1("aviso-mode");
    }
    let st = RwSignal::new(None::<AvisoState>);
    let clock = RwSignal::new(hhmm(now_ms()));
    let load = move || {
        spawn_local(async move {
            match tauri::invoke::<AvisoState, _>("aviso_state", &NoArgs {}).await {
                Ok(s) => {
                    clock.set(hhmm(now_ms()));
                    let empty = s.active.is_empty() && s.missed.is_empty();
                    st.set(Some(s));
                    if empty {
                        tauri::call("aviso_fit", Fit { height: 0.0 });
                    } else {
                        fit_later();
                    }
                }
                Err(e) => web_sys::console::warn_1(&e.into()),
            }
        });
    };
    load();
    tauri::listen::<Value>("michi://avisos", move |_| load());
    let _ = set_interval_with_handle(move || clock.set(hhmm(now_ms())), std::time::Duration::from_secs(15));

    view! {
        <div id="aviso-root" class="aviso">
            <div class="board-h" data-tauri-drag-region>
                <span class="bj" data-tauri-drag-region>"発車案内"</span>
                <span data-tauri-drag-region>"NIHONGO MICHI · AVISO"</span>
                <span class="num" data-tauri-drag-region>{move || clock.get()}</span>
            </div>
            {move || st.get().map(|s| body(s))}
        </div>
    }
}

fn body(s: AvisoState) -> AnyView {
    let snooze = if s.snooze_min > 0.0 { s.snooze_min } else { 10.0 };
    let missed = (!s.missed.is_empty()).then(|| {
        let n = s.missed.len();
        let items = s
            .missed
            .iter()
            .take(4)
            .map(|a| {
                view! {
                    <div class="av-mrow">
                        <span class="av-mt num">{local(a.fire_at).format("%H:%M").to_string()}</span>
                        <span class="av-mn">{a.title.clone()}</span>
                    </div>
                }
            })
            .collect_view();
        view! {
            <div class="av-missed">
                <div class="av-label"><span class="st">"未 PERDIDOS"</span>
                    {format!(" Enquanto o app estava fechado: {} {}", n, if n == 1 { "lembrete" } else { "lembretes" })}
                </div>
                {items}
                {(n > 4).then(|| view! { <div class="av-more">{format!("+ {} na central", n - 4)}</div> })}
                <div class="av-btns">
                    <button class="av-b" on:click=|_| tauri::call("aviso_missed_ok", Open { open: true })>"Ver na central"</button>
                    <button class="av-b ok" on:click=|_| tauri::call("aviso_missed_ok", Open { open: false })>"OK"</button>
                </div>
            </div>
        }
    });
    let mut active = s.active.into_iter();
    let main = active.next().map(|a| {
        let id = a.id;
        let when = if a.at.is_empty() { local(a.fire_at).format("%H:%M").to_string() } else { a.at.clone() };
        let snooze_chips = [5.0, 15.0, 30.0, 60.0]
            .into_iter()
            .filter(|m| *m != snooze)
            .map(|m| view! { <button class="av-chip" on:click=move |_| act(id, "snooze", Some(m))>{format!("{m}")}</button> })
            .collect_view();
        view! {
            <div class="av-main" style=format!("--kc:{}", a.color)>
                <div class="av-time num">{when}</div>
                <div class="av-info">
                    <div class="av-title"><span class="j">{a.jp.clone()}</span>{a.title.clone()}</div>
                    <div class="av-body">{a.body.clone()}</div>
                </div>
                <span class="st now">"発車 AGORA"</span>
            </div>
            <div class="av-btns">
                <button class="av-b ok" on:click=move |_| act(id, "done", None)>"✓ Feito"</button>
                <button class="av-b" title="Começar o modo foco com este bloco" on:click=move |_| act(id, "focus", None)>"▶ Foco"</button>
                <button class="av-b" on:click=move |_| act(id, "snooze", Some(snooze))>{format!("⏰ Adiar {snooze} min")}</button>
                <span class="av-chips" title="Adiar por outros minutos">{snooze_chips}</span>
                <button class="av-b" on:click=move |_| act(id, "open", None)>"↗ Abrir"</button>
                <button class="av-x" title="Dispensar" aria-label="Dispensar" on:click=move |_| act(id, "dismiss", None)>"✕"</button>
            </div>
        }
    });
    let rest: Vec<Aviso> = active.collect();
    let others = (!rest.is_empty()).then(|| {
        let rows = rest
            .into_iter()
            .map(|a| {
                let id = a.id;
                let when = if a.at.is_empty() { local(a.fire_at).format("%H:%M").to_string() } else { a.at.clone() };
                view! {
                    <div class="av-other" style=format!("--kc:{}", a.color)>
                        <span class="av-mt num">{when}</span>
                        <span class="av-mn"><span class="j">{a.jp.clone()}</span>{a.title.clone()}</span>
                        <button class="av-mini ok" title="Feito" on:click=move |_| act(id, "done", None)>"✓"</button>
                        <button class="av-mini" title="Adiar" on:click=move |_| act(id, "snooze", Some(snooze))>"⏰"</button>
                        <button class="av-mini" title="Dispensar" on:click=move |_| act(id, "dismiss", None)>"✕"</button>
                    </div>
                }
            })
            .collect_view();
        view! { <div class="av-others">{rows}</div> }
    });
    view! { {missed}{main}{others} }.into_any()
}
