//! The focus window ("foco"): a small always-on-top board in the top-right
//! corner with the timer of the current session, and a summary when it
//! ends. The backend creates and places it; this view reports its height.

use crate::tauri::{self, NoArgs};
use crate::util::{by_id, document};
use leptos::prelude::*;
use michi_core::focus::{clock, Focus};
use michi_core::time::{hhmm, now_ms};
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

#[derive(Serialize)]
struct Fit {
    height: f64,
    place: bool,
}

#[derive(Serialize)]
struct Minutes {
    minutes: f64,
}

thread_local! {
    /// Session id the window was last placed for (it is placed once per session).
    static PLACED: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

fn fit_later(id: String) {
    set_timeout(
        move || {
            let h = by_id("foco-root").map(|e| e.scroll_height() as f64).unwrap_or(0.0);
            let place = PLACED.with(|p| {
                let mut p = p.borrow_mut();
                let first = *p != id;
                *p = id.clone();
                first
            });
            tauri::call("foco_fit", Fit { height: h, place });
        },
        std::time::Duration::from_millis(40),
    );
}

#[component]
pub fn FocoApp() -> impl IntoView {
    if let Some(el) = document().document_element() {
        let _ = el.class_list().add_1("aviso-mode");
    }
    let st = RwSignal::new(None::<Focus>);
    let now = RwSignal::new(now_ms());
    let apply = move |f: Option<Focus>| {
        match &f {
            Some(x) => fit_later(x.id.clone()),
            None => tauri::call("foco_fit", Fit { height: 0.0, place: false }),
        }
        st.set(f);
    };
    spawn_local(async move {
        if let Ok(f) = tauri::invoke::<Option<Focus>, _>("foco_state", &NoArgs {}).await {
            apply(f);
        }
    });
    tauri::listen::<Option<Focus>>("michi://foco", move |f| apply(f));
    let _ = set_interval_with_handle(move || now.set(now_ms()), std::time::Duration::from_millis(500));

    view! {
        <div id="foco-root" class="aviso foco">
            <div class="board-h" data-tauri-drag-region>
                <span class="bj" data-tauri-drag-region>"集中"</span>
                <span data-tauri-drag-region>"MODO FOCO"</span>
                <span class="num" data-tauri-drag-region>{move || hhmm(now.get())}</span>
            </div>
            {move || st.get().map(|f| body(f, now))}
        </div>
    }
}

fn body(f: Focus, now: RwSignal<f64>) -> AnyView {
    let color = if f.color.is_empty() { "#0a72c0".to_string() } else { f.color.clone() };
    let title = view! {
        <div class="fo-title"><span class="j">{f.jp.clone()}</span>{f.title.clone()}</div>
    };
    if f.state == "done" {
        let line = if f.credited_min >= 1.0 {
            format!(
                "{} min de foco{}",
                f.credited_min,
                if f.xp > 0.0 { format!(" · +{} XP", f.xp) } else { String::new() }
            )
        } else {
            "Menos de 1 minuto: não contou.".to_string()
        };
        let can_mark = f.linked() && !f.marked && f.credited_min >= 1.0;
        return view! {
            <div class="fo-main" style=format!("--kc:{color}")>
                {title}
                <div class="fo-done">{if f.completed { "✓ Tempo concluído" } else { "■ Foco encerrado" }}</div>
                <div class="fo-sum">{line}</div>
                {f.marked.then(|| view! { <div class="fo-sum ok">"Marcado como feito no quadro de hoje."</div> })}
            </div>
            <div class="av-btns">
                {can_mark.then(|| view! { <button class="av-b ok" on:click=|_| tauri::call("foco_mark", NoArgs {})>"✓ Marcar feito"</button> })}
                <button class="av-b" on:click=|_| tauri::call("foco_close", NoArgs {})>"Fechar"</button>
            </div>
        }
        .into_any();
    }
    let paused = f.state == "paused";
    let f2 = f.clone();
    let f3 = f.clone();
    view! {
        <div class="fo-main" style=format!("--kc:{color}")>
            {title}
            <div class="fo-time num" class:paused=paused>{move || clock(f2.remaining_ms(now.get()))}</div>
            <div class="fo-bar"><i style=move || {
                let p = if f3.planned_ms > 0.0 { f3.elapsed_ms(now.get()) / f3.planned_ms } else { 0.0 };
                format!("width:{:.1}%", (p * 100.0).clamp(0.0, 100.0))
            }></i></div>
            {paused.then(|| view! { <div class="fo-sum">"Pausado"</div> })}
        </div>
        <div class="av-btns">
            {if paused {
                view! { <button class="av-b ok" on:click=|_| tauri::call("foco_resume", NoArgs {})>"▶ Continuar"</button> }.into_any()
            } else {
                view! { <button class="av-b" on:click=|_| tauri::call("foco_pause", NoArgs {})>"⏸ Pausar"</button> }.into_any()
            }}
            <button class="av-b" title="Mais 5 minutos" on:click=|_| tauri::call("foco_add", Minutes { minutes: 5.0 })>"+5"</button>
            <button class="av-b" on:click=|_| tauri::call("foco_stop", NoArgs {})>"■ Encerrar"</button>
        </div>
    }
    .into_any()
}
