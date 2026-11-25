//! Central de avisos: history of every reminder (shown, done, snoozed,
//! missed while the app was closed, silenced by the quiet hours).

use crate::app::toast;
use crate::store::sigs;
use crate::tauri::{self, NoArgs};
use leptos::prelude::*;
use michi_core::agenda::Aviso;
use michi_core::time::{local, now_ms, today_key};
use serde::Serialize;
use wasm_bindgen_futures::spawn_local;

#[derive(Serialize)]
struct Limit {
    limit: i64,
}

pub fn status_label(s: &str) -> (&'static str, &'static str) {
    match s {
        "shown" => ("line", "na tela"),
        "done" => ("ok", "✓ feito"),
        "snoozed" => ("warn", "⏰ adiado"),
        "dismissed" => ("", "dispensado"),
        "missed" => ("bad", "perdido (app fechado)"),
        "silenced" => ("", "🌙 silenciado"),
        "opened" => ("ok", "aberto"),
        _ => ("", "—"),
    }
}

#[component]
pub fn Avisos() -> impl IntoView {
    let list = RwSignal::new(None::<Vec<Aviso>>);
    let filter = RwSignal::new("all".to_string());
    let s = sigs();
    let load = move || {
        spawn_local(async move {
            match tauri::invoke::<Vec<Aviso>, _>("avisos_list", &Limit { limit: 400 }).await {
                Ok(v) => list.set(Some(v)),
                Err(e) => {
                    list.set(Some(vec![]));
                    toast(&format!("Não consegui ler os avisos: {e}"), "bad");
                }
            }
        });
    };
    load();
    // Opening the centre reads everything.
    set_timeout(
        move || {
            spawn_local(async move {
                let _ = tauri::invoke::<(), _>("avisos_read_all", &NoArgs {}).await;
                crate::app::refresh_unread();
            });
        },
        std::time::Duration::from_millis(800),
    );
    // Live updates while open.
    Effect::new(move |_| {
        s.unread.get();
        load();
    });
    let tabs = [("all", "Todos"), ("missed", "Perdidos"), ("silenced", "Silenciados"), ("done", "Feitos")]
        .into_iter()
        .map(|(id, l)| {
            view! { <button role="tab" aria-selected=move || (filter.get() == id).to_string() on:click=move |_| filter.set(id.to_string())>{l}</button> }
        })
        .collect_view();
    view! {
        <div class="row-between">
            <h1>"Central de avisos"</h1>
            <div class="row">
                <button class="btn sm" on:click=|_| tauri::call("aviso_test", NoArgs {})>"🔔 Testar aviso"</button>
                <button class="btn sm ghost" on:click=move |_| {
                    spawn_local(async move {
                        let _ = tauri::invoke::<(), _>("avisos_clear", &NoArgs {}).await;
                        load();
                        toast("Histórico limpo (os avisos ativos ficam).", "");
                    });
                }>"Limpar histórico"</button>
            </div>
        </div>
        <p class="small muted">"Tudo o que o Michi avisou fica aqui: o que você fez, adiou, o que chegou no horário de silêncio e o que passou enquanto o app estava fechado."</p>
        <div class="tabs" role="tablist">{tabs}</div>
        {move || match list.get() {
            None => view! { <div class="loading"><div class="spin"></div>"Carregando…"</div> }.into_any(),
            Some(v) => {
                let f = filter.get();
                let items: Vec<Aviso> = v.into_iter().filter(|a| f == "all" || a.status == f).collect();
                if items.is_empty() {
                    return view! { <div class="empty">"Nenhum aviso aqui ainda. Eles aparecem nos horários da rotina e dos eventos da agenda."</div> }.into_any();
                }
                let today = today_key();
                let mut last_day = String::new();
                let rows = items.into_iter().map(|a| {
                    let day = local(a.fire_at).format("%Y-%m-%d").to_string();
                    let head = (day != last_day).then(|| {
                        last_day = day.clone();
                        let label = if day == today { "Hoje".to_string() } else { local(a.fire_at).format("%d/%m/%Y").to_string() };
                        view! { <div class="eyebrow av-day">{label}</div> }
                    });
                    let (cls, lab) = status_label(&a.status);
                    let go = a.go.clone();
                    let block = a.event_id.strip_prefix("rotina-").map(str::to_string);
                    view! {
                        {head}
                        <div class="av-row" style=format!("--kc:{}", a.color)>
                            <div class="av-t num">{local(a.fire_at).format("%H:%M").to_string()}</div>
                            <div class="grow" style="min-width:0">
                                <div class="av-title"><span class="jp faint">{a.jp.clone()}</span>" "{a.title.clone()}</div>
                                <div class="tiny muted">{a.body.clone()}</div>
                            </div>
                            <span class=format!("pill {cls}")>{lab}</span>
                            {go.map(|g| view! { <button class="btn xs" on:click=move |_| crate::app::go(&g, crate::app::intent_for_block(block.as_deref()))>"Abrir"</button> })}
                        </div>
                    }
                }).collect_view();
                view! { <div class="card stack av-list">{rows}</div> }.into_any()
            }
        }}
        <div class="tiny faint">{format!("Atualizado às {}", michi_core::time::hhmm(now_ms()))}</div>
    }
}
