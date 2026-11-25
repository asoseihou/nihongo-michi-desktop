//! Grammar point cards and the lesson modal (port of `pointCard`,
//! `openLesson`, `markStudied` in `11-journey.js`).

use crate::app::{close_all_modals, go, open_modal, toast};
use crate::data::{self, Point, Sentence};
use crate::jp::{role_legend, ruby, sent_html, sent_plain, spk, ROLES};
use crate::store;
use crate::util::{esc, md, yt_search};
use leptos::prelude::*;
use michi_core::time::now_ms;
use serde_json::json;

/// `markStudied(pid)`: the point joins the Grammar deck.
pub fn mark_studied(pid: &str, ev: Option<&web_sys::MouseEvent>) {
    let now = now_ms();
    store::write(&["journey"], |s| s.j.points.entry(pid.to_string()).or_default().studied = now);
    store::with_today(|d| d.gp += 1.0);
    store::add_xp_at(15.0, ev);
    toast("Ponto marcado como estudado. Ele entra no baralho de Gramática.", "ok");
}

pub fn roles_in(sents: &[Sentence]) -> Vec<&'static str> {
    ROLES.iter().map(|r| r.0).filter(|r| sents.iter().any(|s| s.c.iter().any(|c| c.r == *r))).collect()
}

/// `exampleHTML(S)`.
pub fn example_html(s: &Sentence, anat: bool) -> String {
    format!(
        r#"<div class="ex"><div class="row" style="gap:6px;align-items:flex-start"><div class="sent grow">{}</div>{}</div><div class="en">{}</div></div>"#,
        sent_html(s, anat, false, ""),
        spk(&sent_plain(s), "icon-btn"),
        esc(&s.en)
    )
}

/// One `<details class="gpoint">` card; `on_change` redraws after "Estudei".
pub fn point_card(p: &Point, open: bool, on_change: impl Fn() + Clone + 'static) -> AnyView {
    let studied = store::read(|s| s.j.studied(&p.id));
    let ex: String = p.s.iter().take(2).map(|s| example_html(s, true)).collect();
    let more: Vec<String> = p.s.iter().skip(2).map(|s| example_html(s, true)).collect();
    let legend = role_legend(&roles_in(&p.s));
    let pid = p.id.clone();
    let (pid_b, pid_p, pid_x, pid_a, pid_s) = (pid.clone(), pid.clone(), pid.clone(), pid.clone(), pid.clone());
    let yt = (!p.yt.is_empty()).then(|| yt_search(&p.yt));
    let explain = md(&p.explain);
    let tip = (!p.tip.is_empty()).then(|| format!("<b>Cuidado:</b> {}", md(&p.tip)));
    let n_more = more.len();
    let practice = move |mode: &'static str, pid: String| {
        move |_| {
            close_all_modals();
            go("frases", Some(json!({"mode": mode, "pids": [pid.clone()]})));
        }
    };
    view! {
        <details class="gpoint" open=open>
            <summary>
                <div style="min-width:0">
                    <div class="pat">{p.pattern.clone()}</div>
                    <div style="font-weight:700">{p.title.clone()}</div>
                    <div class="small muted">{p.gist.clone()}</div>
                </div>
                <div>{if studied { view! { <span class="pill ok">"✓ estudado"</span> }.into_any() } else { view! { <span class="pill">"novo"</span> }.into_any() }}</div>
            </summary>
            <div class="body">
                <div class="explain" inner_html=explain></div>
                {tip.map(|t| view! { <div class="small" inner_html=t></div> })}
                <div class="stack" inner_html=ex></div>
                {(n_more > 0).then(|| view! {
                    <details><summary class="small" style="cursor:pointer;font-weight:650">{format!("Mais {n_more} frases")}</summary>
                        <div class="stack" style="margin-top:8px" inner_html=more.concat()></div>
                    </details>
                })}
                <div inner_html=legend></div>
                <div class="row">
                    <button class="btn sm" on:click=practice("builder", pid_b)>"🧩 Montar"</button>
                    <button class="btn sm" on:click=practice("particles", pid_p)>"を Partículas"</button>
                    <button class="btn sm" on:click=practice("xray", pid_x)>"🔬 Raio-X"</button>
                    {yt.map(|u| view! { <a class="btn sm" href=u target="_blank" rel="noopener">"▶ Vídeo"</a> })}
                    <button class="btn sm" on:click=move |_| {
                        close_all_modals();
                        if let Some(c) = data::content() {
                            if let Some(p) = c.point(&pid_a) {
                                go("sensei", Some(json!({"ask": format!("Explain the grammar point \"{}\" ({}) again with a different angle and 3 new example sentences.", p.title, p.pattern)})));
                            }
                        }
                    }>"先生 Perguntar"</button>
                    {(!studied).then(|| {
                        let on_change = on_change.clone();
                        view! { <button class="btn sm primary" on:click=move |ev| { mark_studied(&pid_s, Some(&ev)); on_change(); }>"✓ Estudei"</button> }
                    })}
                </div>
            </div>
        </details>
    }
    .into_any()
}

/// `openLesson(lid)`.
pub fn open_lesson(lid: &str) {
    let Some(c) = data::content() else { return };
    let Some(l) = c.lesson(lid).or_else(|| c.readings.iter().find(|r| r.id == lid)) else { return };
    let title = format!("{} <span class=\"jp muted small\">{}</span>", esc(&l.title), esc(&l.title_jp));
    let lid = lid.to_string();
    let rev = RwSignal::new(0u64);
    open_modal(&title, true, move || {
        let lid = lid.clone();
        (move || {
            rev.get();
            let Some(c) = data::content() else { return view! { <div></div> }.into_any() };
            let Some(l) = c.lesson(&lid).or_else(|| c.readings.iter().find(|r| r.id == lid)) else {
                return view! { <div></div> }.into_any();
            };
            let cards = l
                .points
                .iter()
                .enumerate()
                .map(|(i, p)| point_card(p, i == 0, move || rev.update(|v| *v += 1)))
                .collect_view();
            let vocab = (!l.vocab.is_empty()).then(|| {
                let items = l
                    .vocab
                    .iter()
                    .map(|v| {
                        format!(
                            r#"<button class="btn sm jp" data-say="{}" title="{}">{} <span class="tiny muted">{}</span></button>"#,
                            esc(&crate::jp::plain_j(&v.0)),
                            esc(&v.1),
                            ruby(&v.0, "always"),
                            esc(&v.1)
                        )
                    })
                    .collect::<String>();
                view! { <div class="card tint stack"><div class="eyebrow">"Vocabulário da lição"</div><div class="row" style="gap:6px" inner_html=items></div></div> }
            });
            view! {
                {(!l.genki.is_empty()).then(|| view! { <div class="pill">{l.genki.clone()}</div> })}
                <div class="stack">{cards}</div>
                {vocab}
            }
            .into_any()
        })
        .into_any()
    });
}
