//! Multiple-choice quiz in a modal, shared by the boss (`bossQuiz`), the
//! sign quiz (`signQuiz`) and a few games.

use crate::app::{close_modal, detached, open_modal};
use crate::jp::spk;
use leptos::prelude::*;
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct Mc {
    /// HTML shown above the options.
    pub prompt: String,
    pub opts: Vec<String>,
    pub a: String,
    /// Text for the 🔊 button.
    pub say: Option<String>,
    /// One option per line.
    pub wide: bool,
    /// Options in Japanese type.
    pub jp: bool,
    /// HTML shown after answering; when set, the quiz waits for "Próxima".
    pub reveal: Option<String>,
}

/// `finish(right, total, modal_id)` draws the result screen.
pub type Finish = Arc<dyn Fn(usize, usize, u32) -> AnyView + Send + Sync>;
pub type OnAnswer = Arc<dyn Fn(&Mc, bool) + Send + Sync>;
/// Runs once, right after the last answer: `on_done(right, total)` (save results here,
/// not in `finish`, which may redraw).
pub type OnDone = Arc<dyn Fn(usize, usize) + Send + Sync>;

/// Opens a quiz modal. `on_answer(q, ok)` runs on every answer.
pub fn open_quiz(title: &str, wide: bool, qs: Vec<Mc>, on_answer: OnAnswer, on_done: OnDone, finish: Finish) -> u32 {
    detached(move || {
        let qs = StoredValue::new(qs);
        let i = RwSignal::new(0usize);
        let right = RwSignal::new(0usize);
        let picked = RwSignal::new(None::<String>);
        let total = qs.with_value(|q| q.len());
        let mid = StoredValue::new(0u32);
        let id = open_modal(title, wide, move || {
            let finish = finish.clone();
            let on_answer = on_answer.clone();
            let on_done = on_done.clone();
            (move || {
                let n = i.get();
                if n >= total {
                    return finish(right.get_untracked(), total, mid.get_value());
                }
                let q = qs.with_value(|v| v[n].clone());
                let chosen = picked.get();
                let say = q.say.as_ref().map(|s| spk(s, "btn sm"));
                let opts = q
                    .opts
                    .iter()
                    .map(|o| {
                        let o2 = o.clone();
                        let is_right = chosen.is_some() && *o == q.a;
                        let is_wrong = chosen.as_deref() == Some(o.as_str()) && *o != q.a;
                        let q2 = q.clone();
                        let on_answer = on_answer.clone();
                        let on_done = on_done.clone();
                        view! {
                            <button class="opt" class:jpo=q.jp class:right=is_right class:wrong=is_wrong disabled=chosen.is_some()
                                on:click=move |_| {
                                    if picked.get_untracked().is_some() { return; }
                                    let ok = o2 == q2.a;
                                    if ok { right.update(|r| *r += 1); }
                                    picked.set(Some(o2.clone()));
                                    on_answer(&q2, ok);
                                    if n + 1 == total { on_done(right.get_untracked(), total); }
                                    if q2.reveal.is_none() {
                                        set_timeout(move || { picked.set(None); i.update(|x| *x += 1); },
                                            std::time::Duration::from_millis(if ok { 550 } else { 1300 }));
                                    }
                                }>{o.clone()}</button>
                        }
                    })
                    .collect_view();
                view! {
                    <div class="row-between"><span class="pill">{format!("{}/{}", n + 1, total)}</span><span class="pill ok">{format!("{} certas", right.get())}</span></div>
                    <div style="text-align:center" inner_html=q.prompt.clone()></div>
                    {say.map(|s| view! { <div style="text-align:center" inner_html=s></div> })}
                    <div class="opts" style=if q.wide { "grid-template-columns:1fr" } else { "" }>{opts}</div>
                    {(chosen.is_some() && q.reveal.is_some()).then(|| view! {
                        <div class="stack" style="align-items:center">
                            <div inner_html=q.reveal.clone().unwrap_or_default()></div>
                            <button class="btn primary sm" on:click=move |_| { picked.set(None); i.update(|x| *x += 1); }>
                                {if n + 1 < total { "Próxima →" } else { "Ver resultado" }}
                            </button>
                        </div>
                    })}
                }
                .into_any()
            })
            .into_any()
        });
        mid.set_value(id);
        id
    })
}

/// "Tentar de novo / Fechar" buttons for result screens.
pub fn result_buttons(modal: u32, again: impl Fn() + Send + Sync + 'static) -> AnyView {
    view! {
        <div class="row" style="justify-content:center">
            <button class="btn" on:click=move |_| { close_modal(modal); again(); }>"Tentar de novo"</button>
            <button class="btn primary" on:click=move |_| close_modal(modal)>"Fechar"</button>
        </div>
    }
    .into_any()
}
