//! Updates: the card in Ajustes, the "new version ready" banner and the
//! "what's new" note shown once after the app updated itself.

use crate::app::toast;
use crate::store;
use crate::tauri::{self, NoArgs};
use crate::util::{esc, ev_checked};
use crate::views::dados::when;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct JustUpdated {
    pub from: String,
    pub to: String,
    pub notes: String,
}

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct UpdStatus {
    pub current: String,
    pub state: String,
    pub version: String,
    pub notes: String,
    pub progress: f64,
    pub error: String,
    pub checked_at: f64,
    pub auto_check: bool,
    pub auto_install: bool,
    pub just_updated: Option<JustUpdated>,
}

/// Release notes (a markdown list) as HTML; only the part before the
/// install instructions ("---").
pub fn notes_html(notes: &str) -> String {
    let main = notes.split("\n---").next().unwrap_or("").trim();
    let mut items: Vec<String> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    for line in main.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("- ") {
            items.push(rest.to_string());
        } else if !t.is_empty() && line.starts_with("  ") && !items.is_empty() {
            let last = items.last_mut().unwrap();
            last.push(' ');
            last.push_str(t);
        } else if !t.is_empty() && !t.starts_with('#') {
            plain.push(t.to_string());
        }
    }
    let inline = |s: &str| {
        // **bold** and `code`
        let html = crate::util::md(s);
        let mut out = String::new();
        for (i, part) in html.split('`').enumerate() {
            if i % 2 == 1 {
                out.push_str("<code>");
                out.push_str(part);
                out.push_str("</code>");
            } else {
                out.push_str(part);
            }
        }
        out
    };
    let mut out = String::new();
    for p in &plain {
        out.push_str(&format!("<p>{}</p>", inline(p)));
    }
    if !items.is_empty() {
        out.push_str("<ul class=\"notes\">");
        for it in &items {
            out.push_str(&format!("<li>{}</li>", inline(it)));
        }
        out.push_str("</ul>");
    }
    if out.is_empty() {
        out = format!("<p>{}</p>", esc("Correções e melhorias."));
    }
    out
}

/// Saves what the window holds, then installs (the app closes and reopens).
pub fn install_now() {
    spawn_local(async {
        store::flush_now().await;
        if let Err(e) = tauri::invoke::<(), _>("update_install", &NoArgs {}).await {
            toast(&format!("Não consegui instalar: {e}"), "bad");
        }
    });
}

/// After an update: a toast and, when the release has notes, what changed.
pub fn show_just_updated(j: &JustUpdated) {
    toast(&format!("Nihongo Michi atualizado para a versão {}.", j.to), "ok");
    if !j.notes.trim().is_empty() {
        let html = notes_html(&j.notes);
        let title = format!("Novidades da versão {}", esc(&j.to));
        crate::app::open_modal(&title, false, move || {
            let html = html.clone();
            view! { <div class="stack" inner_html=html></div> }.into_any()
        });
    }
    tauri::call("update_seen", NoArgs {});
}

#[derive(Serialize)]
struct UpdSet {
    #[serde(rename = "autoCheck")]
    auto_check: Option<bool>,
    #[serde(rename = "autoInstall")]
    auto_install: Option<bool>,
}

#[component]
pub fn Updates() -> impl IntoView {
    let st = RwSignal::new(None::<UpdStatus>);
    let busy = RwSignal::new(false);
    spawn_local(async move {
        if let Ok(s) = tauri::invoke::<UpdStatus, _>("update_status", &NoArgs {}).await {
            st.set(Some(s));
        }
    });
    tauri::listen::<UpdStatus>("michi://update", move |s| {
        if st.try_get_untracked().is_some() {
            st.set(Some(s));
        }
    });
    let set = move |a: UpdSet| {
        spawn_local(async move {
            match tauri::invoke::<UpdStatus, _>("update_set", &a).await {
                Ok(s) => st.set(Some(s)),
                Err(e) => toast(&format!("Não consegui salvar: {e}"), "bad"),
            }
        });
    };
    let check = move |_| {
        busy.set(true);
        spawn_local(async move {
            match tauri::invoke::<UpdStatus, _>("update_check", &NoArgs {}).await {
                Ok(s) => {
                    if s.state == "latest" {
                        toast("Você já está na versão mais nova.", "ok");
                    }
                    if st.try_get_untracked().is_some() {
                        st.set(Some(s));
                    }
                }
                Err(e) => toast(&format!("Não consegui procurar: {e}"), "bad"),
            }
            busy.set(false);
        });
    };
    view! {
        <div class="card stack" id="atualizacoes">
            <h2>"Atualizações"</h2>
            {move || st.get().map(|s| {
                let line = match s.state.as_str() {
                    "checking" => "Procurando versão nova…".to_string(),
                    "downloading" => format!("Baixando a versão {}… {}%", s.version, (s.progress * 100.0).round()),
                    "ready" => format!("A versão {} está pronta para instalar.", s.version),
                    "installing" => format!("Instalando a versão {}…", s.version),
                    "latest" => format!("Você está na versão mais nova (verificado {}).", when(s.checked_at)),
                    "error" => format!("Não consegui verificar: {}", s.error),
                    _ => "Ainda não procurou nesta sessão.".to_string(),
                };
                let ready = s.state == "ready";
                let notes = (ready && !s.notes.trim().is_empty()).then(|| notes_html(&s.notes));
                view! {
                    <div class="small">"Versão instalada: "<b>{s.current.clone()}</b></div>
                    <div class="small muted">{line}</div>
                    {notes.map(|h| view! { <div class="card tint small" inner_html=h></div> })}
                    <label class="switch"><input type="checkbox" prop:checked=s.auto_check on:change=move |ev| set(UpdSet { auto_check: Some(ev_checked(&ev)), auto_install: None })/>" Procurar versões novas sozinho"</label>
                    <label class="switch"><input type="checkbox" prop:checked=s.auto_install on:change=move |ev| set(UpdSet { auto_check: None, auto_install: Some(ev_checked(&ev)) })/>" Instalar sozinho quando a janela estiver fechada"</label>
                    <div class="row">
                        {if ready {
                            view! { <button class="btn sm primary" on:click=|_| install_now()>"Reiniciar e atualizar"</button> }.into_any()
                        } else {
                            view! { <button class="btn sm" disabled=move || busy.get() on:click=check>"Procurar agora"</button> }.into_any()
                        }}
                        <a class="btn sm ghost" href="https://github.com/asoseihou/nihongo-michi-desktop/releases">"Ver versões no GitHub"</a>
                    </div>
                    <div class="tiny faint">"As versões vêm das releases do projeto no GitHub e só instalam se a assinatura delas conferir. A instalação leva alguns segundos: o app fecha e abre de novo sozinho."</div>
                }
            })}
        </div>
    }
}
