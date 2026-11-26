//! Ajustes (port of `Views.ajustes` in `19-ajustes.js`). The Google
//! Calendar card became the app's own reminder settings: the routine is
//! now a set of recurring events in the internal agenda.

use crate::app::{take_intent, toast};
use crate::logic::trip_dates;
use crate::store;
use crate::tauri::{self, NoArgs};
use crate::tts;
use crate::util::{clamp, ev_checked, ev_value, scroll_into_view};
use crate::views::dados::{reload_all, restore_banner, BackupInfo};
use leptos::prelude::*;
use michi_core::routine::{days_label, routine_default, routine_of, Block, ALLDAYS, WEEK, WEEK1, WEEK_ORDER};
use michi_core::time::minutes_of;
use serde::{Deserialize, Serialize};
use wasm_bindgen_futures::spawn_local;

#[component]
pub fn Ajustes() -> impl IntoView {
    let intent = take_intent();
    if let Some(f) = intent.as_ref().and_then(|i| i.get("focus")).and_then(|v| v.as_str()) {
        let id = match f {
            "rotina" => "rotina",
            "sincronizar" => "sincronizar",
            "backups" => "backups",
            _ => "",
        };
        if !id.is_empty() {
            set_timeout(move || scroll_into_view(id), std::time::Duration::from_millis(60));
        }
    }
    view! {
        <h1>"Ajustes"</h1>
        <div class="grid2"><You/><Voice/></div>
        <Routine/>
        <Reminders/>
        <Data/>
        <div class="grid2"><crate::views::dados::AutoBackup/><crate::views::dados::PcSync/></div>
        <crate::views::atualizar::Updates/>
        <Credits/>
    }
}

fn saved() {
    toast("Salvo.", "");
}

#[component]
fn You() -> impl IntoView {
    let p = store::p();
    let (ts, te) = trip_dates();
    let num = |key: &'static str, lo: f64, hi: f64| {
        move |ev: web_sys::Event| {
            let v = clamp(ev_value(&ev).parse::<f64>().unwrap_or(0.0).floor(), lo, hi);
            store::write(&["profile"], |s| match key {
                "goalMin" => s.p.goal_min = v,
                "newKanji" => s.p.new_kanji = v,
                "newVocab" => s.p.new_vocab = v,
                "newGrammar" => s.p.new_grammar = v,
                _ => s.p.new_mined = v,
            });
            saved();
        }
    };
    let trip = |is_start: bool| {
        move |ev: web_sys::Event| {
            let v = ev_value(&ev);
            if michi_core::time::parse_key(&v).is_none() {
                return;
            }
            let (s, e) = trip_dates();
            let (ns, ne) = if is_start { (v.clone(), e) } else { (s, v.clone()) };
            if ne < ns {
                toast("A volta precisa ser depois da ida.", "bad");
                store::refresh();
                return;
            }
            store::write(&["profile"], |st| if is_start { st.p.trip_start = v } else { st.p.trip_end = v });
            toast("Datas da viagem salvas. A agenda já mostra os dias da viagem nas novas datas.", "");
        }
    };
    let news = [("newKanji", "Kanji novos/dia", p.new_kanji), ("newVocab", "Vocab novos/dia", p.new_vocab), ("newGrammar", "Gramática nova/dia", p.new_grammar), ("newMined", "Mineração nova/dia", p.new_mined)]
        .into_iter()
        .map(|(k, l, v)| {
            let id = format!("s_{k}");
            view! { <div class="field"><label for=id.clone()>{l}</label><input type="number" id=id min="0" max="50" value=v.to_string() on:change=num(k, 0.0, 50.0)/></div> }
        })
        .collect_view();
    let furi = p.furi.clone();
    view! {
        <div class="card stack">
            <h2>"Você"</h2>
            <div class="field"><label for="sn">"Nome"</label>
                <input type="text" id="sn" value=p.name.clone() on:change=|ev| {
                    let v: String = ev_value(&ev).trim().chars().take(30).collect();
                    store::write(&["profile"], |s| s.p.name = v);
                    store::refresh_chips();
                    saved();
                }/>
            </div>
            <div class="grid2">
                <div class="field"><label for="ts">"✈️ Viagem: ida"</label><input type="date" id="ts" value=ts on:change=trip(true)/></div>
                <div class="field"><label for="te">"Volta"</label><input type="date" id="te" value=te on:change=trip(false)/></div>
            </div>
            <div class="field"><label for="sg">"Meta diária (minutos)"</label><input type="number" id="sg" min="10" max="240" value=p.goal_min.to_string() on:change=num("goalMin", 10.0, 240.0)/></div>
            <div class="field"><label for="sf">"Furigana"</label>
                <select id="sf" on:change=|ev| { let v = ev_value(&ev); store::write(&["profile"], |s| s.p.furi = v); saved(); }>
                    <option value="smart" selected=furi == "smart">"Inteligente — some nos kanji que você já domina"</option>
                    <option value="always" selected=furi == "always">"Sempre"</option>
                    <option value="never" selected=furi == "never">"Nunca"</option>
                </select>
            </div>
            <label class="switch"><input type="checkbox" prop:checked=p.show_pt on:change=|ev| { let v = ev_checked(&ev); store::write(&["profile"], |s| s.p.show_pt = v); }/>" Mostrar também o significado em português nos kanji"</label>
            <div class="grid2">{news}</div>
        </div>
    }
}

#[component]
fn Voice() -> impl IntoView {
    let rate = RwSignal::new(store::read(|s| s.p.tts_rate));
    let voices = tts::ja_voices();
    let cur = tts::current_uri();
    let picker = if voices.is_empty() {
        view! { <div class="small muted">"Nenhuma voz ja-JP encontrada. No Windows: Configurações → Hora e idioma → Fala → Adicionar vozes → Japonês (Haruka, Ayumi, Ichiro). Depois reabra o app."</div> }.into_any()
    } else {
        let opts = voices
            .iter()
            .map(|v| {
                let uri = v.voice_uri();
                let sel = uri == cur;
                view! { <option value=uri selected=sel>{v.name()}</option> }
            })
            .collect_view();
        view! {
            <div class="field"><label for="sv">"Voz"</label>
                <select id="sv" on:change=|ev| {
                    let v = ev_value(&ev);
                    store::write(&["profile"], |s| s.p.voice = v);
                    tts::pick();
                    tts::say("こんにちは", None);
                }>{opts}</select>
            </div>
        }
        .into_any()
    };
    view! {
        <div class="card stack">
            <h2>"Voz japonesa"</h2>
            {picker}
            <div class="field"><label for="sr">"Velocidade: "{move || format!("{:.2}", rate.get())}</label>
                <input type="range" id="sr" min="0.6" max="1.2" step="0.02" value=rate.get_untracked().to_string() on:input=move |ev| {
                    let v = ev_value(&ev).parse::<f64>().unwrap_or(0.92);
                    rate.set(v);
                    store::write(&["profile"], |s| s.p.tts_rate = v);
                }/>
            </div>
            <button class="btn sm" data-say="今日もいっしょに日本語を勉強しましょう。">"🔊 Testar"</button>
        </div>
    }
}

/* ---------------- routine editor ---------------- */

fn same_block(a: &Block, b: &Block) -> bool {
    a.time == b.time
        && a.min == b.min
        && a.name == b.name
        && a.what == b.what
        && a.core == b.core
        && a.off == b.off
        && a.days_of() == b.days_of()
}

#[component]
fn Routine() -> impl IntoView {
    let start: Vec<Block> = routine_of(&store::read(|s| s.p.routine.clone()))
        .into_iter()
        .map(|mut b| {
            b.days = Some(b.days_of());
            b
        })
        .collect();
    let draft = StoredValue::new(start);
    let rows_rev = RwSignal::new(0u64);
    let dirty = RwSignal::new(false);
    let tick = RwSignal::new(0u64);
    let changed = move || {
        dirty.set(true);
        tick.update(|v| *v += 1);
    };
    let total = move || {
        tick.get();
        let d = draft.get_value();
        let per: Vec<f64> =
            WEEK_ORDER.iter().map(|w| d.iter().filter(|b| !b.off && b.on_day(*w)).map(|b| b.min).sum()).collect();
        let lo = per.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = per.iter().cloned().fold(0.0, f64::max);
        format!(
            "{} blocos ligados · {} min por dia{}",
            d.iter().filter(|b| !b.off).count(),
            if lo == hi { format!("{hi}") } else { format!("{lo}–{hi}") },
            if dirty.get() { " · alterações não salvas" } else { "" }
        )
    };
    let save = move |_| {
        let mut d = draft.get_value();
        d.sort_by_key(|b| minutes_of(&b.time));
        let vlog = d.iter().find(|b| b.id == "vlog" && !b.off).map(|b| b.time.clone());
        store::write(&["profile"], |s| {
            s.p.routine = Some(d.clone());
            if let Some(t) = vlog {
                s.p.vlog_time = t;
            }
        });
        store::flush();
        draft.set_value(d);
        dirty.set(false);
        rows_rev.update(|v| *v += 1);
        tick.update(|v| *v += 1);
        toast("Rotina salva. A agenda e os avisos já seguem os novos horários.", "ok");
    };
    let rows = move || {
        rows_rev.get();
        let saved: Vec<Block> = routine_of(&store::read(|s| s.p.routine.clone()));
        draft
            .get_value()
            .into_iter()
            .enumerate()
            .map(|(i, b)| row(i, b, &saved, draft, rows_rev, tick, changed))
            .collect_view()
    };
    view! {
        <div class="card stack" id="rotina">
            <div class="row-between"><h2>"Rotina"</h2>
                <div class="row">
                    <button class="btn sm" on:click=move |_| {
                        draft.update_value(|d| d.push(Block {
                            id: format!("c_{}", crate::util::uid()),
                            days: Some(ALLDAYS.to_vec()),
                            ..Block::default()
                        }));
                        rows_rev.update(|v| *v += 1);
                        changed();
                    }>"+ Bloco"</button>
                    <button class="btn sm ghost" on:click=move |_| {
                        draft.set_value(routine_default().into_iter().map(|mut b| { b.days = Some(ALLDAYS.to_vec()); b }).collect());
                        rows_rev.update(|v| *v += 1);
                        changed();
                        toast("Rotina padrão carregada. Clique em Salvar rotina para confirmar.", "");
                    }>"Restaurar padrão"</button>
                </div>
            </div>
            <p class="small muted">"Ajuste à mão o horário, a duração e os dias de cada bloco. Para tirar um bloco, desmarque a caixa da esquerda. O quadro de Hoje, a agenda e os avisos seguem esta tabela. "<b>"Essencial"</b>" = continua no dia mínimo."</p>
            <div>{rows}</div>
            <div class="row">
                <button class="btn sm primary" disabled=move || !dirty.get() on:click=save>"Salvar rotina"</button>
                <span class="small muted">{total}</span>
            </div>
        </div>
    }
}

fn row(
    i: usize,
    b: Block,
    saved: &[Block],
    draft: StoredValue<Vec<Block>>,
    rows_rev: RwSignal<u64>,
    tick: RwSignal<u64>,
    changed: impl Fn() + Copy + 'static,
) -> AnyView {
    let custom = b.id.starts_with("c_");
    let status = move || {
        tick.get();
        let Some(cur) = draft.get_value().get(i).cloned() else { return ("".to_string(), String::new()) };
        let was = routine_of(&store::read(|s| s.p.routine.clone())).into_iter().find(|x| x.id == cur.id);
        if cur.off {
            ("".into(), "desligado".into())
        } else {
            match was {
                Some(w) if same_block(&w, &cur) => ("ok".into(), "📅 na agenda".into()),
                Some(_) => ("warn".into(), "📅 mudou".into()),
                None => ("warn".into(), "📅 novo".into()),
            }
        }
    };
    let _ = saved;
    let set = move |f: Box<dyn Fn(&mut Block)>| {
        draft.update_value(|d| {
            if let Some(b) = d.get_mut(i) {
                f(b);
            }
        });
        changed();
    };
    let days_lbl = move || {
        tick.get();
        draft.get_value().get(i).map(|b| days_label(&b.days_of())).unwrap_or_default()
    };
    let day_btns = WEEK_ORDER
        .iter()
        .map(|w| {
            let w = *w;
            let pressed = move || {
                tick.get();
                draft.get_value().get(i).map(|b| b.on_day(w)).unwrap_or(false).to_string()
            };
            view! {
                <button type="button" aria-pressed=pressed title=WEEK[w as usize] on:click=move |_| {
                    let cur = draft.get_value().get(i).map(|b| b.days_of()).unwrap_or_default();
                    if cur.contains(&w) && cur.len() == 1 {
                        toast("Cada bloco precisa de pelo menos um dia. Para tirar o bloco, desmarque a caixa da esquerda.", "");
                        return;
                    }
                    set(Box::new(move |b| {
                        let mut d = b.days_of();
                        if d.contains(&w) { d.retain(|x| *x != w); } else { d.push(w); d.sort_unstable(); }
                        b.days = Some(d);
                    }));
                }>{WEEK1[w as usize]}</button>
            }
        })
        .collect_view();
    let off = RwSignal::new(b.off);
    let name = b.name.clone();
    view! {
        <div class="rt" class:off=move || off.get()>
            <input type="checkbox" class="rt-on" prop:checked=!b.off aria-label=format!("Bloco {name} ligado") on:change=move |ev| {
                let on = ev_checked(&ev);
                off.set(!on);
                set(Box::new(move |b| b.off = !on));
            }/>
            <input type="time" value=b.time.clone() aria-label="Horário" on:change=move |ev| {
                let v = ev_value(&ev);
                if !v.is_empty() { set(Box::new(move |b| b.time = v.clone())); }
            }/>
            <label class="rt-min">
                <input type="number" min="5" max="180" step="5" value=b.min.to_string() aria-label="Minutos" on:change=move |ev| {
                    let v = clamp(ev_value(&ev).parse::<f64>().unwrap_or(15.0).floor(), 5.0, 180.0);
                    set(Box::new(move |b| b.min = v));
                }/>
                <span class="tiny faint">"min"</span>
            </label>
            <div class="rt-name">
                <span class="jp rt-jp">{b.jp.clone()}</span>
                <input type="text" value=b.name.clone() maxlength="40" aria-label="Nome do bloco" on:change=move |ev| {
                    let v: String = ev_value(&ev).trim().chars().take(40).collect();
                    if !v.is_empty() { set(Box::new(move |b| b.name = v.clone())); }
                }/>
                {custom.then(|| view! {
                    <button class="btn sm ghost" aria-label="Remover bloco" on:click=move |_| {
                        draft.update_value(|d| { if i < d.len() { d.remove(i); } });
                        rows_rev.update(|v| *v += 1);
                        changed();
                    }>"✕"</button>
                })}
            </div>
            <div class="rt-sub">
                <div class="rt-days" role="group" aria-label="Dias">{day_btns}</div>
                <span class="tiny muted rt-dl">{days_lbl}</span>
                <label class="switch small"><input type="checkbox" prop:checked=b.core on:change=move |ev| {
                    let v = ev_checked(&ev);
                    set(Box::new(move |b| b.core = v));
                }/>" Essencial"</label>
                <span class=move || format!("pill {}", status().0)>{move || status().1}</span>
            </div>
            {if custom {
                view! {
                    <input type="text" class="rt-what" value=b.what.clone() maxlength="80" placeholder="O que fazer neste bloco (opcional)" aria-label="O que fazer" on:change=move |ev| {
                        let v: String = ev_value(&ev).trim().chars().take(80).collect();
                        set(Box::new(move |b| b.what = v.clone()));
                    }/>
                }.into_any()
            } else {
                view! { <div class="rt-what tiny muted">{b.what.clone()}</div> }.into_any()
            }}
        </div>
    }
    .into_any()
}

/* ---------------- reminders (desktop) ---------------- */

#[derive(Serialize)]
struct On {
    on: bool,
}

#[derive(Serialize)]
struct Vol {
    volume: f64,
}

fn desk_write(f: impl FnOnce(&mut michi_core::agenda::DeskSettings)) {
    store::write(&["desk"], |s| f(&mut s.desk));
    store::flush();
}

#[component]
fn Reminders() -> impl IntoView {
    let d = store::read(|s| s.desk.clone());
    let remind = store::read(|s| s.p.remind_before());
    let auto = RwSignal::new(false);
    let vol = RwSignal::new(d.volume);
    spawn_local(async move {
        if let Ok(v) = tauri::invoke::<bool, _>("autostart_get", &NoArgs {}).await {
            auto.set(v);
        }
    });
    let snooze = d.snooze_min as i64;
    let remind_opts = [0, 5, 10, 15, 30]
        .into_iter()
        .map(|m| view! { <option value=m.to_string() selected=m == remind>{if m == 0 { "na hora".to_string() } else { format!("{m} min antes") }}</option> })
        .collect_view();
    let snooze_opts = [5, 10, 15, 30, 60]
        .into_iter()
        .map(|m| view! { <option value=m.to_string() selected=m == snooze>{format!("{m} min")}</option> })
        .collect_view();
    view! {
        <div class="card stack">
            <h2>"Lembretes"</h2>
            <p class="small muted">"O Michi avisa sozinho, sem notificações do Windows: na hora de cada bloco abre uma janelinha no canto da tela, com som, e os botões Feito / Adiar / Abrir. Fechar a janela principal deixa o app na bandeja (ícone 道 perto do relógio); para sair de vez, use "<b>"Sair"</b>" no menu da bandeja."</p>
            <div class="grid2">
                <div class="card tint stack">
                    <label class="switch"><input type="checkbox" prop:checked=d.reminders_on on:change=|ev| { let v = ev_checked(&ev); desk_write(|d| d.reminders_on = v); }/>" Avisos ligados"</label>
                    <div class="row"><label class="small" for="crm">"Avisar os blocos da rotina"</label>
                        <select id="crm" on:change=|ev| {
                            let v = ev_value(&ev).parse::<f64>().unwrap_or(0.0);
                            store::write(&["profile"], |s| s.p.cal_remind = v);
                            store::flush();
                            saved();
                        }>{remind_opts}</select>
                    </div>
                    <div class="row"><label class="small" for="snz">"Adiar por padrão"</label>
                        <select id="snz" on:change=|ev| { let v = ev_value(&ev).parse::<f64>().unwrap_or(10.0); desk_write(|d| d.snooze_min = v); }>{snooze_opts}</select>
                    </div>
                    <label class="switch small"><input type="checkbox" prop:checked=move || auto.get() on:change=move |ev| {
                        let on = ev_checked(&ev);
                        spawn_local(async move {
                            match tauri::invoke::<bool, _>("autostart_set", &On { on }).await {
                                Ok(v) => { auto.set(v); toast(if v { "O Michi vai abrir com o Windows, direto na bandeja." } else { "O Michi não abre mais com o Windows." }, "ok"); }
                                Err(e) => toast(&format!("Não consegui mudar: {e}"), "bad"),
                            }
                        });
                    }/>" Abrir com o Windows (na bandeja) — para os avisos chegarem mesmo sem abrir o app"</label>
                </div>
                <div class="card tint stack">
                    <label class="switch"><input type="checkbox" prop:checked=d.quiet_on on:change=|ev| { let v = ev_checked(&ev); desk_write(|d| d.quiet_on = v); }/>" Horário de silêncio"</label>
                    <div class="row">
                        <label class="small">"das"</label>
                        <input type="time" value=d.quiet_start.clone() on:change=|ev| { let v = ev_value(&ev); if !v.is_empty() { desk_write(|d| d.quiet_start = v); } }/>
                        <label class="small">"às"</label>
                        <input type="time" value=d.quiet_end.clone() on:change=|ev| { let v = ev_value(&ev); if !v.is_empty() { desk_write(|d| d.quiet_end = v); } }/>
                    </div>
                    <span class="tiny faint">"Nesse horário nada abre nem toca: o aviso fica guardado na central (🔔)."</span>
                    <label class="switch"><input type="checkbox" prop:checked=d.sound on:change=|ev| { let v = ev_checked(&ev); desk_write(|d| d.sound = v); }/>" Som do aviso"</label>
                    <div class="row">
                        <input type="range" min="0.1" max="1" step="0.05" value=d.volume.to_string() aria-label="Volume" on:change=move |ev| {
                            let v = ev_value(&ev).parse::<f64>().unwrap_or(0.7);
                            vol.set(v);
                            desk_write(|d| d.volume = v);
                        }/>
                        <button class="btn sm" on:click=move |_| tauri::call("sound_test", Vol { volume: vol.get_untracked() })>"🔊 Testar som"</button>
                    </div>
                </div>
            </div>
            <div class="row">
                <button class="btn sm primary" on:click=|_| tauri::call("aviso_test", NoArgs {})>"🔔 Testar aviso agora"</button>
                <a class="btn sm" href="#avisos">"Abrir a central de avisos"</a>
                <a class="btn sm" href="#agenda">"Ver a agenda"</a>
            </div>
        </div>
    }
}

/* ---------------- data ---------------- */

#[derive(Deserialize, Clone, Debug, Default)]
struct AppInfo {
    version: String,
    data_dir: String,
}

#[component]
fn Data() -> impl IntoView {
    let pending = RwSignal::new(None::<BackupInfo>);
    let confirm_reset = RwSignal::new(false);
    let info = RwSignal::new(AppInfo::default());
    spawn_local(async move {
        if let Ok(i) = tauri::invoke::<AppInfo, _>("app_info", &NoArgs {}).await {
            info.set(i);
        }
    });
    let export = |_| {
        store::flush();
        spawn_local(async {
            match tauri::invoke::<Option<String>, _>("backup_export", &NoArgs {}).await {
                Ok(Some(p)) => toast(&format!("Backup salvo em {p}"), "ok"),
                Ok(None) => {}
                Err(e) => toast(&format!("Não consegui salvar: {e}"), "bad"),
            }
        });
    };
    let pick = move |_| {
        spawn_local(async move {
            match tauri::invoke::<Option<BackupInfo>, _>("backup_pick", &NoArgs {}).await {
                Ok(Some(i)) => pending.set(Some(i)),
                Ok(None) => {}
                Err(e) => toast(&format!("Não consegui ler: {e}"), "bad"),
            }
        });
    };
    let reset = move |_| {
        spawn_local(async move {
            store::flush_now().await;
            match tauri::invoke::<(), _>("reset_progress", &NoArgs {}).await {
                Ok(()) => {
                    confirm_reset.set(false);
                    reload_all().await;
                    toast("Progresso zerado.", "");
                    crate::app::go("hoje", None);
                }
                Err(e) => toast(&format!("Não consegui zerar: {e}"), "bad"),
            }
        });
    };
    view! {
        <div class="card stack">
            <h2>"Seus dados"</h2>
            <p class="small muted">"Status: "<b>"salvo neste PC (SQLite)"</b>", com backup automático todo dia (abaixo). O backup usa o mesmo formato da versão web do Nihongo Michi: dá para restaurar aqui o arquivo de "<b>"Ajustes → 💾 Exportar backup"</b>" da versão web, e o backup daqui também abre lá."</p>
            <div class="row">
                <button class="btn sm" on:click=export>"💾 Exportar backup"</button>
                <button class="btn sm" on:click=pick>"📂 Restaurar backup"</button>
                <button class="btn sm danger" on:click=move |_| confirm_reset.set(true)>"Zerar progresso…"</button>
                <button class="btn sm ghost" on:click=|_| tauri::call("reveal_data_dir", NoArgs {})>"Abrir pasta dos dados"</button>
            </div>
            {restore_banner(pending)}
            {move || confirm_reset.get().then(|| view! {
                <div class="banner warn"><span class="bico">"⚠️"</span>
                    <div class="grow">"Zerar todo o progresso (SRS, jornada, registros)? As configurações ficam. Antes, o Michi guarda uma cópia na pasta dos backups."</div>
                    <button class="btn sm danger" on:click=reset>"Zerar tudo"</button>
                    <button class="btn sm ghost" on:click=move |_| confirm_reset.set(false)>"Cancelar"</button>
                </div>
            })}
            <div class="tiny faint">{move || { let i = info.get(); format!("Versão {} · dados em {}", i.version, i.data_dir) }}</div>
        </div>
    }
}

#[component]
fn Credits() -> impl IntoView {
    view! {
        <div class="card stack">
            <h2>"Créditos e licenças"</h2>
            <div class="small muted">
                "Kanji: "<b>"KANJIDIC2"</b>" © EDRDG, CC BY-SA 4.0 · Ordem dos traços: "<b>"KanjiVG"</b>" © Ulrich Apel, CC BY-SA 3.0 · Níveis JLPT dos kanji: kanji-data (D. Gouveia, MIT) · Vocabulário JLPT: open-anki-jlpt-decks (MIT), baseado nas listas de Jonathan Waller (tanos.co.uk, CC BY) · Frases, gramática, leituras e frases de viagem: conteúdo original do Nihongo Michi, escrito para você. Wagotabi é um jogo da Wagotabi Limited; o mapa da Linha Wagotabi usa só informações públicas (notas de atualização e lojas) e não reproduz conteúdo do jogo. Genki é citado apenas por número de lição. Som do aviso: composição original gerada pelo próprio app."
            </div>
        </div>
    }
}
