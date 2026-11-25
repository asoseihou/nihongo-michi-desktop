//! Japanese text-to-speech with the system voices exposed to WebView2
//! (`speechSynthesis`), port of `TTS` in `02-jp.js`.

use crate::jp::plain_j;
use crate::store;
use std::cell::RefCell;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{SpeechSynthesis, SpeechSynthesisUtterance, SpeechSynthesisVoice};

thread_local! {
    static PICKED: RefCell<Option<SpeechSynthesisVoice>> = const { RefCell::new(None) };
}

fn synth() -> Option<SpeechSynthesis> {
    crate::util::window().speech_synthesis().ok()
}

/// All ja-JP voices.
pub fn ja_voices() -> Vec<SpeechSynthesisVoice> {
    let Some(s) = synth() else { return vec![] };
    s.get_voices()
        .iter()
        .filter_map(|v| v.dyn_into::<SpeechSynthesisVoice>().ok())
        .filter(|v| {
            let l = v.lang().to_lowercase();
            l == "ja" || l.starts_with("ja-") || l.starts_with("ja_")
        })
        .collect()
}

/// Chooses the preferred voice (`TTS.pick`).
pub fn pick() {
    let ja = ja_voices();
    let pref = store::read(|s| s.p.voice.clone());
    let find = |f: &dyn Fn(&SpeechSynthesisVoice) -> bool| ja.iter().find(|v| f(v)).cloned();
    let v = find(&|v| !pref.is_empty() && v.voice_uri() == pref)
        .or_else(|| find(&|v| v.name().contains("Nanami") || v.name().contains("Natural")))
        .or_else(|| find(&|v| v.name().contains("Google")))
        .or_else(|| find(&|v| v.name().contains("Online")))
        .or_else(|| ja.first().cloned());
    PICKED.with(|p| *p.borrow_mut() = v);
}

pub fn current_uri() -> String {
    PICKED.with(|p| p.borrow().as_ref().map(|v| v.voice_uri()).unwrap_or_default())
}

/// Loads voices now and again when the list changes.
pub fn init() {
    let Some(s) = synth() else { return };
    pick();
    let cb = Closure::<dyn FnMut()>::new(pick);
    s.set_onvoiceschanged(Some(cb.as_ref().unchecked_ref()));
    cb.forget();
}

pub fn stop() {
    if let Some(s) = synth() {
        s.cancel();
    }
}

/// `say(text, rate)`.
pub fn say(text: &str, rate: Option<f64>) {
    let text = plain_j(text);
    if text.is_empty() {
        return;
    }
    let Some(s) = synth() else { return };
    let voice = PICKED.with(|p| p.borrow().clone());
    let Some(v) = voice else {
        crate::app::toast(
            "Nenhuma voz japonesa instalada. No Windows: Configurações → Hora e idioma → Fala → Adicionar vozes → Japonês.",
            "",
        );
        return;
    };
    s.cancel();
    if let Ok(u) = SpeechSynthesisUtterance::new_with_text(&text) {
        u.set_voice(Some(&v));
        u.set_lang(&v.lang());
        let r = rate.unwrap_or_else(|| store::read(|s| s.p.tts_rate)).clamp(0.5, 1.5);
        u.set_rate(r as f32);
        s.speak(&u);
    }
}

/// `await say(text)`: resolves when the utterance ends (or after 15 s).
pub async fn say_wait(text: &str, rate: Option<f64>) {
    let text = plain_j(text);
    let (Some(s), Some(v)) = (synth(), PICKED.with(|p| p.borrow().clone())) else { return };
    if text.is_empty() {
        return;
    }
    s.cancel();
    let Ok(u) = SpeechSynthesisUtterance::new_with_text(&text) else { return };
    u.set_voice(Some(&v));
    u.set_lang(&v.lang());
    u.set_rate(rate.unwrap_or_else(|| store::read(|s| s.p.tts_rate)).clamp(0.5, 1.5) as f32);
    let p = js_sys::Promise::new(&mut |res, _| {
        u.set_onend(Some(&res));
        u.set_onerror(Some(&res));
        let _ = crate::util::window().set_timeout_with_callback_and_timeout_and_arguments_0(&res, 15_000);
    });
    s.speak(&u);
    let _ = wasm_bindgen_futures::JsFuture::from(p).await;
}
