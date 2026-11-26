//! Ajustes → automatic backups and the sync between PCs.

use crate::app::toast;
use crate::store;
use crate::tauri::{self, NoArgs};
use crate::util::{ev_checked, ev_value};
use leptos::prelude::*;
use michi_core::time::{local, now_ms};
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

/* ---------------- shared ---------------- */

#[derive(Deserialize, Clone, Debug, Default)]
pub struct BackupInfo {
    pub path: String,
    pub at: f64,
    pub name: String,
    pub xp: f64,
    pub days: usize,
    pub cards: usize,
    pub desktop: bool,
}

#[derive(Serialize)]
struct PathArg {
    path: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct SyncBrief {
    on: bool,
}

/// "dd/mm/aaaa hh:mm", "hoje hh:mm" or "ontem hh:mm".
pub fn when(ms: f64) -> String {
    if ms <= 0.0 {
        return "—".into();
    }
    let t = local(ms);
    let today = local(now_ms()).date();
    if t.date() == today {
        format!("hoje {}", t.format("%H:%M"))
    } else if t.date() == today - chrono::Duration::days(1) {
        format!("ontem {}", t.format("%H:%M"))
    } else {
        t.format("%d/%m/%Y %H:%M").to_string()
    }
}

/// After a restore, a reset or a sync: the in-memory copy is re-read.
pub async fn reload_all() {
    let _ = store::load().await;
    crate::tts::pick();
    store::refresh();
}

/// The "replace the progress with this backup?" banner, shared by the manual
/// restore and the list of automatic backups.
pub fn restore_banner(pending: RwSignal<Option<BackupInfo>>) -> impl IntoView {
    let sync_on = RwSignal::new(false);
    spawn_local(async move {
        if let Ok(s) = tauri::invoke::<SyncBrief, _>("sync_status", &NoArgs {}).await {
            sync_on.set(s.on);
        }
    });
    let apply = move |_| {
        let Some(i) = pending.get_untracked() else { return };
        spawn_local(async move {
            store::flush_now().await;
            match tauri::invoke::<(), _>("backup_apply", &PathArg { path: i.path }).await {
                Ok(()) => {
                    pending.set(None);
                    reload_all().await;
                    toast("Backup restaurado. Uma cópia do progresso de antes ficou na pasta dos backups.", "ok");
                    crate::app::go("hoje", None);
                }
                Err(e) => toast(&format!("Não consegui restaurar: {e}"), "bad"),
            }
        });
    };
    move || {
        pending.get().map(|i| {
            let at = if i.at > 0.0 { when(i.at) } else { "data desconhecida".into() };
            let what = format!(
                "Restaurar o backup de {at}{}? {} XP, {} dias de estudo, {} cartas SRS{}. Isso substitui o progresso atual (antes, o Michi guarda uma cópia dele).",
                if i.name.is_empty() { String::new() } else { format!(" ({})", i.name) },
                i.xp,
                i.days,
                i.cards,
                if i.desktop { ", agenda do app desktop" } else { "" }
            );
            view! {
                <div class="banner warn"><span class="bico">"⚠️"</span>
                    <div class="grow">{what}
                        {move || sync_on.get().then(|| view! { <div class="small"><b>"A sincronização está ligada: os outros PCs também voltam para este backup."</b></div> })}
                    </div>
                    <button class="btn sm danger" on:click=apply>"Restaurar"</button>
                    <button class="btn sm ghost" on:click=move |_| pending.set(None)>"Cancelar"</button>
                </div>
            }
        })
    }
}

/* ---------------- automatic backup ---------------- */

#[derive(Deserialize, Clone, Debug, Default)]
struct FileInfo {
    path: String,
    name: String,
    kind: String,
    at: f64,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct AutoStatus {
    on: bool,
    dir: String,
    default_dir: String,
    keep: usize,
    last_at: f64,
    error: String,
    files: Vec<FileInfo>,
}

#[derive(Serialize)]
struct AutoSet {
    on: Option<bool>,
    keep: Option<u32>,
    #[serde(rename = "defaultDir")]
    default_dir: Option<bool>,
}

#[derive(Serialize)]
struct Which {
    which: &'static str,
}

fn kind_label(k: &str) -> &'static str {
    match k {
        "auto" => "diário",
        "safety" => "antes de substituir",
        _ => "exportado",
    }
}

#[component]
pub fn AutoBackup() -> impl IntoView {
    let st = RwSignal::new(None::<AutoStatus>);
    let busy = RwSignal::new(false);
    let pending = RwSignal::new(None::<BackupInfo>);
    let load = move || {
        spawn_local(async move {
            match tauri::invoke::<AutoStatus, _>("backup_auto_status", &NoArgs {}).await {
                Ok(s) => st.set(Some(s)),
                Err(e) => toast(&format!("Backups: {e}"), "bad"),
            }
        });
    };
    load();
    let set = move |a: AutoSet| {
        spawn_local(async move {
            match tauri::invoke::<AutoStatus, _>("backup_auto_set", &a).await {
                Ok(s) => st.set(Some(s)),
                Err(e) => toast(&format!("Não consegui salvar: {e}"), "bad"),
            }
        });
    };
    let pick = move |_| {
        spawn_local(async move {
            match tauri::invoke::<Option<AutoStatus>, _>("backup_auto_pick_dir", &NoArgs {}).await {
                Ok(Some(s)) => {
                    st.set(Some(s));
                    toast("Pasta dos backups trocada.", "ok");
                }
                Ok(None) => {}
                Err(e) => toast(&format!("Não consegui usar essa pasta: {e}"), "bad"),
            }
        });
    };
    let now = move |_| {
        busy.set(true);
        spawn_local(async move {
            store::flush_now().await;
            match tauri::invoke::<AutoStatus, _>("backup_auto_now", &NoArgs {}).await {
                Ok(s) => {
                    st.set(Some(s));
                    toast("Backup de hoje salvo.", "ok");
                }
                Err(e) => toast(&format!("Não consegui fazer o backup: {e}"), "bad"),
            }
            busy.set(false);
        });
    };
    let restore = move |path: String| {
        spawn_local(async move {
            match tauri::invoke::<BackupInfo, _>("backup_inspect", &PathArg { path }).await {
                Ok(i) => pending.set(Some(i)),
                Err(e) => toast(&format!("Não consegui ler: {e}"), "bad"),
            }
        });
    };
    view! {
        <div class="card stack" id="backups">
            <h2>"Backup automático"</h2>
            {move || st.get().map(|s| {
                let keep_opts = [7usize, 14, 30, 60, 90]
                    .into_iter()
                    .map(|k| view! { <option value=k.to_string() selected=k == s.keep>{format!("{k} dias")}</option> })
                    .collect_view();
                let custom = s.dir != s.default_dir;
                let files = s.files.iter().take(6).cloned().map(|f| {
                    let path = f.path.clone();
                    view! {
                        <div class="row">
                            <span class="grow small">{when(f.at)}" · "<span class="faint">{kind_label(&f.kind)}</span></span>
                            <span class="tiny faint">{f.name.clone()}</span>
                            <button class="btn sm ghost" on:click=move |_| restore(path.clone())>"Restaurar…"</button>
                        </div>
                    }
                }).collect_view();
                view! {
                    <p class="small muted">"Uma vez por dia o Michi guarda uma cópia do seu progresso, no mesmo formato do "<b>"💾 Exportar backup"</b>". Antes de restaurar, zerar ou trazer o progresso de outro PC, ele também guarda uma cópia do que havia antes."</p>
                    <label class="switch"><input type="checkbox" prop:checked=s.on on:change=move |ev| set(AutoSet { on: Some(ev_checked(&ev)), keep: None, default_dir: None })/>" Backup diário ligado"</label>
                    <div class="row">
                        <label class="small" for="bkk">"Guardar os últimos"</label>
                        <select id="bkk" on:change=move |ev| {
                            let k = ev_value(&ev).parse::<u32>().unwrap_or(30);
                            set(AutoSet { on: None, keep: Some(k), default_dir: None });
                        }>{keep_opts}</select>
                    </div>
                    <div class="small">"Pasta: "<b class="brk">{s.dir.clone()}</b></div>
                    <div class="row">
                        <button class="btn sm" on:click=pick>"Trocar pasta…"</button>
                        {custom.then(|| view! { <button class="btn sm ghost" on:click=move |_| set(AutoSet { on: None, keep: None, default_dir: Some(true) })>"Voltar para Documentos"</button> })}
                        <button class="btn sm ghost" on:click=|_| tauri::call("open_folder", Which { which: "backup" })>"Abrir pasta"</button>
                        <button class="btn sm primary" disabled=move || busy.get() on:click=now>"Fazer backup agora"</button>
                    </div>
                    <div class="tiny faint">{format!("Último backup: {}", when(s.last_at))}</div>
                    {(!s.error.is_empty()).then(|| view! { <div class="banner warn"><span class="bico">"⚠️"</span><div class="grow small">{format!("O último backup falhou: {}", s.error)}</div></div> })}
                    {(!s.files.is_empty()).then(|| view! { <div class="stack"><div class="small"><b>"Backups na pasta"</b></div>{files}</div> })}
                }
            })}
            {restore_banner(pending)}
        </div>
    }
}

/* ---------------- sync between PCs ---------------- */

#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
struct DeviceInfo {
    name: String,
    at: f64,
    version: String,
    xp: f64,
    days: usize,
    cards: usize,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct SyncStatus {
    on: bool,
    dir: String,
    device_name: String,
    last_at: f64,
    error: String,
    pending: Vec<String>,
    devices: Vec<DeviceInfo>,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct Preview {
    dir: String,
    devices: Vec<DeviceInfo>,
    here: DeviceInfo,
    fresh: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
struct Report {
    received: usize,
}

#[derive(Serialize)]
struct Join {
    dir: String,
    how: &'static str,
}

fn device_line(d: &DeviceInfo) -> String {
    format!("{} XP · {} dias de estudo · {} cartas SRS", d.xp.round(), d.days, d.cards)
}

/// Runs a sync now: saves what the window holds, syncs, re-reads the data.
pub async fn sync_now() -> Result<usize, String> {
    store::flush_now().await;
    let r = tauri::invoke::<Report, _>("sync_now", &NoArgs {}).await?;
    if r.received > 0 {
        reload_all().await;
    }
    Ok(r.received)
}

#[component]
pub fn PcSync() -> impl IntoView {
    let st = RwSignal::new(None::<SyncStatus>);
    let preview = RwSignal::new(None::<Preview>);
    let busy = RwSignal::new(false);
    let load = move || {
        spawn_local(async move {
            if let Ok(s) = tauri::invoke::<SyncStatus, _>("sync_status", &NoArgs {}).await {
                if st.try_get_untracked().is_some() {
                    st.set(Some(s));
                }
            }
        });
    };
    load();
    tauri::listen::<serde_json::Value>("michi://sync-status", move |_| {
        if st.try_get_untracked().is_some() {
            load();
        }
    });
    let pick = move |_| {
        spawn_local(async move {
            store::flush_now().await;
            match tauri::invoke::<Option<Preview>, _>("sync_pick_dir", &NoArgs {}).await {
                Ok(Some(p)) => preview.set(Some(p)),
                Ok(None) => {}
                Err(e) => toast(&format!("Não consegui ler essa pasta: {e}"), "bad"),
            }
        });
    };
    let join = move |how: &'static str| {
        let Some(p) = preview.get_untracked() else { return };
        busy.set(true);
        spawn_local(async move {
            store::flush_now().await;
            match tauri::invoke::<Report, _>("sync_join", &Join { dir: p.dir.clone(), how }).await {
                Ok(r) => {
                    preview.set(None);
                    if r.received > 0 || how != "mine" {
                        reload_all().await;
                    }
                    toast(
                        match how {
                            "adopt" => "Pronto: este PC agora tem o progresso da pasta.",
                            "mine" => "Pronto: o progresso deste PC foi para a pasta. Os outros PCs vão recebê-lo.",
                            _ => "Pronto: sincronização ligada.",
                        },
                        "ok",
                    );
                    load();
                }
                Err(e) => toast(&format!("Não consegui sincronizar: {e}"), "bad"),
            }
            busy.set(false);
        });
    };
    let now = move |_| {
        busy.set(true);
        spawn_local(async move {
            match sync_now().await {
                Ok(0) => toast("Sincronizado. Nada novo dos outros PCs.", "ok"),
                Ok(n) => toast(
                    &format!(
                        "Sincronizado: {n} {} dos outros PCs.",
                        if n == 1 { "registro novo" } else { "registros novos" }
                    ),
                    "ok",
                ),
                Err(e) => toast(&format!("Não consegui sincronizar: {e}"), "bad"),
            }
            load();
            busy.set(false);
        });
    };
    let off = move |_| {
        spawn_local(async move {
            match tauri::invoke::<SyncStatus, _>("sync_off", &NoArgs {}).await {
                Ok(s) => {
                    st.set(Some(s));
                    toast("Sincronização desligada. O progresso continua neste PC.", "");
                }
                Err(e) => toast(&format!("Não consegui desligar: {e}"), "bad"),
            }
        });
    };

    let choice = move || {
        preview.get().map(|p| {
            let others = p
                .devices
                .iter()
                .map(|d| {
                    view! { <li><b>{d.name.clone()}</b>" — "{device_line(d)}<span class="faint">{format!(" · atualizado {}", when(d.at))}</span></li> }
                })
                .collect_view();
            let here = format!("{} (este PC) — {}", p.here.name, device_line(&p.here));
            let body = if p.devices.is_empty() {
                view! {
                    <div class="grow small">"A pasta está vazia: o progresso deste PC vai para lá. Depois, no outro PC, instale o app e escolha a mesma pasta."</div>
                    <button class="btn sm primary" disabled=move || busy.get() on:click=move |_| join("merge")>"Usar esta pasta"</button>
                }
                .into_any()
            } else if p.fresh {
                view! {
                    <div class="grow small">
                        <div>"A pasta tem o progresso de outro PC:"</div><ul>{others}</ul>
                        <div>"Este PC ainda não tem estudo registrado, então o melhor é trazer o progresso da pasta."</div>
                    </div>
                    <button class="btn sm primary" disabled=move || busy.get() on:click=move |_| join("adopt")>"Trazer o progresso da pasta"</button>
                }
                .into_any()
            } else {
                view! {
                    <div class="grow small">
                        <div>"A pasta tem o progresso de outro PC:"</div><ul>{others}</ul>
                        <div>{here}</div>
                        <div class="tiny faint">"Juntar soma o que cada PC fez (dias, cartas, jornada). As outras opções substituem um dos lados; antes, o Michi guarda uma cópia do que havia."</div>
                    </div>
                    <div class="stack">
                        <button class="btn sm primary" disabled=move || busy.get() on:click=move |_| join("merge")>"Juntar os dois (recomendado)"</button>
                        <button class="btn sm" disabled=move || busy.get() on:click=move |_| join("adopt")>"Usar só o da pasta"</button>
                        <button class="btn sm" disabled=move || busy.get() on:click=move |_| join("mine")>"Usar só o deste PC"</button>
                    </div>
                }
                .into_any()
            };
            view! {
                <div class="banner line">
                    <span class="bico">"🔄"</span>
                    {body}
                    <button class="btn sm ghost" on:click=move |_| preview.set(None)>"Cancelar"</button>
                </div>
            }
        })
    };

    view! {
        <div class="card stack" id="sincronizar">
            <h2>"Sincronizar entre PCs"</h2>
            {move || match st.get() {
                None => view! { <div class="small muted">"…"</div> }.into_any(),
                Some(s) if !s.on => view! {
                    <p class="small muted">"Para usar o Michi em mais de um PC: escolha uma pasta que os PCs compartilham — dentro do OneDrive, Google Drive ou Dropbox, ou num pendrive. Cada PC grava ali o próprio arquivo e lê o dos outros; o progresso de cada um se junta sozinho."</p>
                    <div class="row"><button class="btn sm primary" on:click=pick>"Escolher pasta…"</button></div>
                }.into_any(),
                Some(s) => {
                    let others = s.devices.iter().map(|d| view! {
                        <div class="row">
                            <span class="grow small"><b>{d.name.clone()}</b>" · "{device_line(d)}</span>
                            <span class="tiny faint">{format!("atualizado {}{}", when(d.at), if d.version.is_empty() { String::new() } else { format!(" · v{}", d.version) })}</span>
                        </div>
                    }).collect_view();
                    let pending = s.pending.clone();
                    view! {
                        <div class="small">"Pasta: "<b class="brk">{s.dir.clone()}</b></div>
                        <div class="small">"Este PC: "<b>{s.device_name.clone()}</b>" · última sincronização: "{when(s.last_at)}</div>
                        {(!s.error.is_empty()).then(|| view! { <div class="banner warn"><span class="bico">"⚠️"</span><div class="grow small">{format!("A última sincronização falhou: {}", s.error)}</div></div> })}
                        {(!pending.is_empty()).then(|| view! { <div class="banner line"><span class="bico">"🔄"</span><div class="grow small">{format!("Chegou progresso de {}. Ele entra quando você fechar a janela, ou agora:", pending.join(", "))}</div></div> })}
                        {if s.devices.is_empty() {
                            view! { <div class="small muted">"Nenhum outro PC nesta pasta ainda. No outro PC, instale o app e escolha a mesma pasta em Ajustes → Sincronizar entre PCs."</div> }.into_any()
                        } else {
                            view! { <div class="stack">{others}</div> }.into_any()
                        }}
                        <div class="row">
                            <button class="btn sm primary" disabled=move || busy.get() on:click=now>"Sincronizar agora"</button>
                            <button class="btn sm" on:click=pick>"Trocar pasta…"</button>
                            <button class="btn sm ghost" on:click=|_| tauri::call("open_folder", Which { which: "sync" })>"Abrir pasta"</button>
                            <button class="btn sm ghost" on:click=off>"Desligar"</button>
                        </div>
                        <div class="tiny faint">"Sincroniza ao abrir, a cada minuto enquanto o app roda e ao fechar a janela. Os avisos e o histórico da central ficam em cada PC."</div>
                    }.into_any()
                }
            }}
            {choice}
        </div>
    }
}
