//! Jogos (port of `14-games.js`): the ten minigames, kana statistics and
//! the verb conjugation engine.

use crate::app::{go, take_intent, toast};
use crate::data::{self, Kanji};
use crate::jp::{is_kanji, kana, romaji, spk, to_kata, KanaItem};
use crate::store;
use crate::tts::say;
use crate::util::{esc, ev_checked, pick, rand, shuffle};
use crate::views::kanji::{strokes_of, StrokeQuiz};
use leptos::html::{Div, Input};
use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

/* ---------------- kana stats ---------------- */

/// `kanaStat(k, ok, ms)`: [attempts, correct, avg ms], kept to the last ~30.
pub fn kana_stat(k: &str, ok: bool, ms: f64) {
    store::write(&["kana"], |s| {
        let x = s.ks.s.entry(k.to_string()).or_insert_with(|| vec![0.0, 0.0, 0.0]);
        while x.len() < 3 {
            x.push(0.0);
        }
        x[0] += 1.0;
        if ok {
            x[1] += 1.0;
        }
        x[2] = if x[2] > 0.0 { (x[2] * 0.7 + ms * 0.3).round() } else { ms };
        if x[0] > 30.0 {
            x[1] = (x[1] * 30.0 / x[0]).round();
            x[0] = 30.0;
        }
    });
}

/// `weightedKana(pool, avoid)`: the ones you miss come back more often.
pub fn weighted_kana(pool: &[KanaItem], avoid: Option<&str>) -> KanaItem {
    let w: Vec<f64> = store::read(|s| {
        pool.iter()
            .map(|x| {
                let st = s.ks.s.get(&x.k);
                let ratio = st.filter(|v| v.len() >= 2 && v[0] > 0.0).map(|v| v[1] / v[0]).unwrap_or(0.0);
                let base = if st.is_some() { (1.0 - ratio).powi(2) } else { 0.6 };
                (if Some(x.k.as_str()) == avoid { 0.0 } else { 1.0 }) * (base + 0.08)
            })
            .collect()
    });
    let mut r = rand() * w.iter().sum::<f64>();
    for (i, x) in pool.iter().enumerate() {
        r -= w[i];
        if r <= 0.0 {
            return x.clone();
        }
    }
    pool.last().cloned().unwrap_or_else(|| kana()[0].clone())
}

/// `gameDone`: counts a game for today (Today board: "Minigame").
fn game_done(xp: f64) {
    store::with_today(|d| d.games += 1.0);
    store::add_xp(xp);
}

/// `bestOf(key, v)`: true when it is a new record.
fn best_of(key: &str, v: f64) -> bool {
    let old = store::read(|s| s.ks.best.get(key).copied().unwrap_or(0.0));
    if v > old {
        store::write(&["kana"], |s| {
            s.ks.best.insert(key.to_string(), v);
        });
        true
    } else {
        false
    }
}

fn best(key: &str) -> f64 {
    store::read(|s| s.ks.best.get(key).copied().unwrap_or(0.0))
}

fn pool_from(spec: &str) -> Vec<KanaItem> {
    let (sc, sets) = spec.split_once(':').unwrap_or(("h", "basic"));
    let ss: Vec<&str> = sets.split(',').collect();
    kana().iter().filter(|x| sc.contains(x.script) && ss.contains(&x.set)).cloned().collect()
}

/* ---------------- conjugation engine ---------------- */

type Verb = (String, String, String, String, u8);

fn row5(end: char) -> Option<[char; 5]> {
    let s = match end {
        'う' => "いわえおっ",
        'つ' => "ちたてとっ",
        'る' => "りられろっ",
        'む' => "みまめもん",
        'ぶ' => "びばべぼん",
        'ぬ' => "になねのん",
        'く' => "きかけこい",
        'ぐ' => "ぎがげごい",
        'す' => "しさせそし",
        _ => return None,
    };
    let c: Vec<char> = s.chars().collect();
    Some([c[0], c[1], c[2], c[3], c[4]])
}

/// `conj(v, form)`.
pub fn conj(v: &Verb, form: &str) -> String {
    let r = &v.1;
    let cls = v.3.as_str();
    let chars: Vec<char> = r.chars().collect();
    let Some(&end) = chars.last() else { return String::new() };
    let base: String = chars[..chars.len() - 1].iter().collect();
    if cls == "irr" {
        let suru = r.ends_with("する");
        let pre: String = chars[..chars.len().saturating_sub(2)].iter().collect();
        let s = match form {
            "masu" => ["します", "きます"],
            "masen" => ["しません", "きません"],
            "mashita" => ["しました", "きました"],
            "masendeshita" => ["しませんでした", "きませんでした"],
            "nai" => ["しない", "こない"],
            "te" => ["して", "きて"],
            "ta" => ["した", "きた"],
            "nakatta" => ["しなかった", "こなかった"],
            "tai" => ["したい", "きたい"],
            "pot" => ["できる", "こられる"],
            "vol" => ["しよう", "こよう"],
            "ba" => ["すれば", "くれば"],
            "pass" => ["される", "こられる"],
            _ => ["させる", "こさせる"],
        };
        return pre + s[if suru { 0 } else { 1 }];
    }
    let ru = cls == "ru";
    let (stem_i, stem_a, stem_e, stem_o, te) = if ru {
        (base.clone(), base.clone(), base.clone(), base.clone(), format!("{base}て"))
    } else {
        let Some(row) = row5(end) else { return String::new() };
        let te = if r == "いく" || (r.ends_with("いく") && v.0.ends_with("行く")) {
            format!("{base}って")
        } else {
            match end {
                'く' => format!("{base}いて"),
                'ぐ' => format!("{base}いで"),
                'す' => format!("{base}して"),
                'む' | 'ぶ' | 'ぬ' => format!("{base}んで"),
                _ => format!("{base}って"),
            }
        };
        (
            format!("{base}{}", row[0]),
            format!("{base}{}", row[1]),
            format!("{base}{}", row[2]),
            format!("{base}{}", row[3]),
            te,
        )
    };
    let ta = if let Some(x) = te.strip_suffix('て') {
        format!("{x}た")
    } else if let Some(x) = te.strip_suffix('で') {
        format!("{x}だ")
    } else {
        te.clone()
    };
    let nai = if r == "ある" {
        "ない".to_string()
    } else if ru {
        format!("{base}ない")
    } else {
        format!("{stem_a}ない")
    };
    match form {
        "masu" => format!("{stem_i}ます"),
        "masen" => format!("{stem_i}ません"),
        "mashita" => format!("{stem_i}ました"),
        "masendeshita" => format!("{stem_i}ませんでした"),
        "nai" => nai,
        "te" => te,
        "ta" => ta,
        "nakatta" => nai.strip_suffix('い').map(|x| format!("{x}かった")).unwrap_or(nai),
        "tai" => format!("{stem_i}たい"),
        "pot" => {
            if ru {
                format!("{base}られる")
            } else {
                format!("{stem_e}る")
            }
        }
        "vol" => {
            if ru {
                format!("{base}よう")
            } else {
                format!("{stem_o}う")
            }
        }
        "ba" => {
            if ru {
                format!("{base}れば")
            } else {
                format!("{stem_e}ば")
            }
        }
        "pass" => {
            if ru {
                format!("{base}られる")
            } else {
                format!("{stem_a}れる")
            }
        }
        _ => {
            if ru {
                format!("{base}させる")
            } else {
                format!("{stem_a}せる")
            }
        }
    }
}

/// (id, label, JLPT)
const FORMS: [(&str, &str, u8); 14] = [
    ("masu", "ます (polite)", 5),
    ("masen", "ません (polite neg.)", 5),
    ("mashita", "ました (polite past)", 5),
    ("masendeshita", "ませんでした", 5),
    ("te", "て-form", 5),
    ("nai", "ない (plain neg.)", 5),
    ("ta", "た (plain past)", 5),
    ("nakatta", "なかった", 5),
    ("tai", "たい (want to)", 5),
    ("pot", "potential", 4),
    ("vol", "volitional", 4),
    ("ba", "ば conditional", 4),
    ("pass", "passive", 4),
    ("caus", "causative", 4),
];

/// `conjWrong(v, form)`: three plausible wrong answers.
fn conj_wrong(v: &Verb, form: &str) -> Vec<String> {
    let right = conj(v, form);
    let mut c: Vec<String> = Vec::new();
    let alt = if v.3 == "ru" {
        Some((v.0.clone(), v.1.clone(), v.2.clone(), "u".to_string(), v.4))
    } else if v.3 == "u" && v.1.ends_with('る') {
        Some((v.0.clone(), v.1.clone(), v.2.clone(), "ru".to_string(), v.4))
    } else {
        None
    };
    if let Some(a) = alt {
        let x = conj(&a, form);
        if !x.is_empty() {
            c.push(x);
        }
    }
    for (f, _, _) in FORMS {
        if f != form {
            let x = conj(v, f);
            if !x.is_empty() && !c.contains(&x) {
                c.push(x);
            }
        }
    }
    c.retain(|x| *x != right);
    shuffle(&c).into_iter().take(3).collect()
}

/* ---------------- hub ---------------- */

/// (id, glyph, name, description, group, colour)
const GAMES: [(&str, &str, &str, &str, &str, &str); 10] = [
    (
        "flash",
        "⚡",
        "Relâmpago",
        "Kana em 60 s: veja e escolha o som. Foca nos que você mais erra.",
        "kana",
        "var(--kana)",
    ),
    ("rain", "雨", "Chuva de Kana", "Digite o romaji antes de o kana tocar o chão.", "kana", "var(--kana)"),
    ("memory", "対", "Pares", "Ache os pares: hiragana × katakana ou kana × som.", "kana", "var(--kana)"),
    ("listen", "耳", "Ouvido", "Ouça e escolha o kana ou a palavra certa.", "kana", "var(--kana)"),
    ("katakana", "外", "Detetive Katakana", "Decifre palavras estrangeiras em katakana.", "kana", "var(--kana)"),
    ("write", "筆", "Escrever kana", "Desenhe na ordem certa dos traços.", "kana", "var(--kana)"),
    ("kmatch", "漢", "Kanji × significado", "Pares de kanji com os seus significados.", "kanji", "var(--n3)"),
    ("kread", "読", "Leitura certa", "Escolha a leitura da palavra em kanji.", "kanji", "var(--n3)"),
    ("kwrite", "書", "Escrever kanji", "Veja o significado e escreva o kanji de memória.", "kanji", "var(--n3)"),
    ("conj", "活", "Conjugador", "Verbos: ます, て, ない, た… e formas do N4.", "gram", "var(--n5)"),
];

#[component]
pub fn Jogos() -> impl IntoView {
    let intent = take_intent();
    let start = intent.as_ref().and_then(|i| i.get("game").and_then(|g| g.as_str())).map(|g| {
        let spec = intent.as_ref().and_then(|i| i.get("kana").and_then(|k| k.as_str())).map(str::to_string);
        (g.to_string(), spec)
    });
    let game = RwSignal::new(start);
    let ready = RwSignal::new(false);
    spawn_local(async move {
        let _ = data::need_content().await;
        let _ = data::need_kanji().await;
        ready.set(true);
    });
    move || {
        if !ready.get() {
            return view! { <div class="loading"><div class="spin"></div>"Preparando os jogos…"</div> }.into_any();
        }
        match game.get() {
            None => hub(game),
            Some((g, spec)) => {
                crate::util::scroll_top();
                let spec = spec.unwrap_or_else(|| {
                    let last = store::read(|s| s.p.last_kana.clone());
                    if last.is_empty() {
                        "h:basic".into()
                    } else {
                        last
                    }
                });
                view! { <GameScreen id=g spec=spec back=Callback::new(move |_| game.set(None))/> }.into_any()
            }
        }
    }
}

fn hub(game: RwSignal<Option<(String, Option<String>)>>) -> AnyView {
    let groups = [("kana", "Kana"), ("kanji", "Kanji"), ("gram", "Gramática")]
        .into_iter()
        .map(|(grp, label)| {
            let cards = GAMES
                .iter()
                .filter(|g| g.4 == grp)
                .map(|g| {
                    let id = g.0;
                    let rec = best(id);
                    view! {
                        <button class="gamecard" style=format!("--lc:{}", g.5) on:click=move |_| game.set(Some((id.to_string(), None)))>
                            <span class="gj">{g.1}</span><span class="gt">{g.2}</span><span class="small muted">{g.3}</span>
                            {(rec > 0.0).then(|| view! { <span class="tiny faint">{format!("Recorde: {rec}")}</span> })}
                        </button>
                    }
                })
                .collect_view();
            view! { <div class="stack"><div class="eyebrow">{label}</div><div class="grid3">{cards}</div></div> }
        })
        .collect_view();
    let weak: Vec<(KanaItem, f64)> = store::read(|s| {
        let mut v: Vec<(KanaItem, f64)> = kana()
            .iter()
            .filter(|x| s.ks.s.get(&x.k).map(|v| v[0] >= 3.0).unwrap_or(false))
            .map(|x| (x.clone(), s.ks.strength(&x.k)))
            .collect();
        v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        v.truncate(16);
        v
    });
    let weak_html = if weak.is_empty() {
        r#"<div class="small muted">Jogue algumas rodadas e os kana que você mais erra aparecem aqui.</div>"#
            .to_string()
    } else {
        let cells: String = weak
            .iter()
            .map(|(x, s)| {
                let c = if *s >= 0.8 { "w3" } else if *s >= 0.5 { "w2" } else { "w1" };
                format!(
                    r#"<div class="kcell {c}" data-say="{k}"><div class="kc">{k}</div><div class="kr">{r} · {p}%</div></div>"#,
                    k = esc(&x.k),
                    r = esc(&x.r),
                    p = (s * 100.0).round()
                )
            })
            .collect();
        format!(
            r#"<div class="kanatable" style="grid-template-columns:repeat(auto-fill,minmax(58px,1fr))">{cells}</div>"#
        )
    };
    view! {
        <div class="row-between"><h1>"Jogos"</h1><button class="btn sm" on:click=|_| go("frases", None)>"Jogos de frases →"</button></div>
        {groups}
        <div class="card stack"><div class="eyebrow">"Seus kana mais fracos"</div><div inner_html=weak_html></div></div>
    }
    .into_any()
}

/* ---------------- game screen ---------------- */

fn head(title: &'static str, back: Callback<()>, extra: AnyView) -> AnyView {
    view! {
        <div class="row-between">
            <button class="btn ghost sm" on:click=move |_| back.run(())>"← Jogos"</button>
            <h2>{title}</h2>
            <div>{extra}</div>
        </div>
    }
    .into_any()
}

fn title_of(id: &str) -> &'static str {
    match id {
        "flash" => "⚡ Relâmpago",
        "rain" => "雨 Chuva de Kana",
        "memory" => "対 Pares",
        "listen" => "耳 Ouvido",
        "katakana" => "外 Detetive Katakana",
        "write" => "筆 Escrever kana",
        "kmatch" => "漢 Kanji × significado",
        "kread" => "読 Leitura certa",
        "kwrite" => "書 Escrever kanji",
        _ => "活 Conjugador",
    }
}

fn desc_of(id: &str) -> &'static str {
    match id {
        "flash" => "60 segundos. Cada acerto seguido aumenta o combo. Os kana que você erra voltam mais vezes.",
        "rain" => "Digite o romaji (shi/si, tsu/tu… valem) e aperte nada: some sozinho. 3 vidas. Fica mais rápido a cada 10 acertos.",
        "memory" => "Vire duas cartas por vez. Ache todos os 8 pares no menor número de jogadas.",
        "listen" => "Ouça e toque no kana certo. Precisa de uma voz japonesa do Windows (Haruka, Ayumi, Ichiro…).",
        "write" => "Escreva cada kana seguindo a ordem dos traços. Com \"sem guia\", a sombra some.",
        _ => "",
    }
}

#[component]
fn GameScreen(id: String, spec: String, back: Callback<()>) -> impl IntoView {
    let kana_game = matches!(id.as_str(), "flash" | "rain" | "memory" | "listen" | "write");
    let spec = RwSignal::new(spec);
    let running = RwSignal::new(!kana_game && id != "conj");
    let gen = RwSignal::new(0u32);
    let title = title_of(&id);
    let id2 = id.clone();
    let again = Callback::new(move |_| gen.update(|g| *g += 1));
    move || {
        if id2 == "conj" {
            return view! { <ConjGame back=back/> }.into_any();
        }
        if !running.get() {
            return setup(title, desc_of(&id2), spec, running, back);
        }
        gen.get();
        let pool = pool_from(&spec.get_untracked());
        match id2.as_str() {
            "flash" => view! { <Flash pool=pool back=back again=again/> }.into_any(),
            "rain" => view! { <Rain pool=pool back=back again=again/> }.into_any(),
            "memory" => view! { <Memory pool=pool spec=spec.get_untracked() back=back again=again/> }.into_any(),
            "listen" => view! { <Listen pool=pool back=back again=again/> }.into_any(),
            "write" => view! { <WriteKana pool=pool back=back/> }.into_any(),
            "katakana" => view! { <Katakana back=back again=again/> }.into_any(),
            "kmatch" => view! { <KMatch back=back again=again/> }.into_any(),
            "kread" => view! { <KRead back=back again=again/> }.into_any(),
            _ => view! { <KWrite back=back/> }.into_any(),
        }
    }
}

/// `setup` + `kanaPicker`: script and kana sets.
fn setup(
    title: &'static str,
    desc: &'static str,
    spec: RwSignal<String>,
    running: RwSignal<bool>,
    back: Callback<()>,
) -> AnyView {
    let set_spec = move |v: String| {
        spec.set(v.clone());
        store::write(&["profile"], |s| s.p.last_kana = v);
    };
    let scripts = [("h", "ひらがな"), ("k", "カタカナ"), ("hk", "ambos")]
        .into_iter()
        .map(|(v, l)| {
            view! { <button aria-pressed=move || (spec.get().split(':').next() == Some(v)).to_string() on:click=move |_| {
                let sets = spec.get_untracked().split_once(':').map(|x| x.1.to_string()).unwrap_or_else(|| "basic".into());
                set_spec(format!("{v}:{sets}"));
            }>{l}</button> }
        })
        .collect_view();
    let sets = [("basic", "básico"), ("daku", "dakuten"), ("yoon", "combinações"), ("ext", "estendido")]
        .into_iter()
        .map(|(v, l)| {
            view! { <button aria-pressed=move || spec.get().split_once(':').map(|x| x.1.split(',').any(|s| s == v)).unwrap_or(false).to_string() on:click=move |_| {
                let cur = spec.get_untracked();
                let (sc, ss) = cur.split_once(':').unwrap_or(("h", "basic"));
                let mut s: Vec<String> = ss.split(',').map(str::to_string).collect();
                if s.iter().any(|x| x == v) { s.retain(|x| x != v); } else { s.push(v.to_string()); }
                if s.is_empty() { s.push("basic".into()); }
                set_spec(format!("{sc}:{}", s.join(",")));
            }>{l}</button> }
        })
        .collect_view();
    view! {
        {head(title, back, view! { <span></span> }.into_any())}
        <div class="card stack">
            <p class="muted">{desc}</p>
            <div class="row">
                <div class="seg" role="group" aria-label="Escrita">{scripts}</div>
                <div class="seg" role="group" aria-label="Conjunto">{sets}</div>
            </div>
            <button class="btn primary" on:click=move |_| {
                if pool_from(&spec.get_untracked()).len() < 4 { toast("Escolha um conjunto maior.", ""); return; }
                running.set(true);
            }>"Começar"</button>
        </div>
    }
    .into_any()
}

fn result_card(big: String, sub: String, again: Callback<()>) -> AnyView {
    view! {
        <div class="card stack" style="text-align:center">
            <h1>{big}</h1>
            {(!sub.is_empty()).then(|| view! { <p class="muted">{sub}</p> })}
            <div class="row" style="justify-content:center"><button class="btn primary" on:click=move |_| again.run(())>"Jogar de novo"</button></div>
        </div>
    }
    .into_any()
}

/// Options that turn green/red after a pick.
fn options(
    opts: Vec<(String, String)>,
    right: String,
    picked: RwSignal<Option<String>>,
    jp: bool,
    one_col: bool,
    on_pick: impl Fn(String) + Clone + 'static,
) -> AnyView {
    let btns = opts
        .into_iter()
        .map(|(key, label)| {
            let k2 = key.clone();
            let r2 = right.clone();
            let on_pick = on_pick.clone();
            view! {
                <button class="opt" class:jpo=jp
                    class:right=move || picked.get().is_some() && k2 == r2
                    class:wrong={let k3 = key.clone(); let r3 = right.clone(); move || picked.get().as_deref() == Some(k3.as_str()) && k3 != r3}
                    disabled=move || picked.get().is_some()
                    on:click={let key = key.clone(); move |_| { if picked.get_untracked().is_none() { picked.set(Some(key.clone())); on_pick(key.clone()); } }}>
                    {label}
                </button>
            }
        })
        .collect_view();
    view! { <div class="opts" style=if one_col { "grid-template-columns:1fr" } else { "" }>{btns}</div> }.into_any()
}

/* ---------------- ⚡ Relâmpago ---------------- */

#[derive(Clone)]
struct FlashQ {
    cur: KanaItem,
    opts: Vec<KanaItem>,
    reverse: bool,
    shown_at: f64,
}

fn flash_q(pool: &[KanaItem], avoid: Option<&str>) -> FlashQ {
    let cur = weighted_kana(pool, avoid);
    let others: Vec<KanaItem> =
        shuffle(&pool.iter().filter(|x| x.r != cur.r).cloned().collect::<Vec<_>>()).into_iter().take(3).collect();
    let mut opts = vec![cur.clone()];
    opts.extend(others);
    FlashQ { cur, opts: shuffle(&opts), reverse: rand() < 0.25, shown_at: js_sys::Date::now() }
}

#[component]
fn Flash(pool: Vec<KanaItem>, back: Callback<()>, again: Callback<()>) -> impl IntoView {
    let pool = StoredValue::new(pool);
    let q = RwSignal::new(pool.with_value(|p| flash_q(p, None)));
    let score = RwSignal::new(0u32);
    let combo = RwSignal::new(0u32);
    let left = RwSignal::new(60i32);
    let ended = RwSignal::new(false);
    let picked = RwSignal::new(None::<String>);
    let t0 = js_sys::Date::now();
    let end = move || {
        if ended.get_untracked() {
            return;
        }
        ended.set(true);
        best_of("flash", score.get_untracked() as f64);
        game_done((5 + score.get_untracked()).min(40) as f64);
    };
    let h = set_interval_with_handle(
        move || {
            if ended.get_untracked() {
                return;
            }
            let l = 60 - ((js_sys::Date::now() - t0) / 1000.0).floor() as i32;
            left.set(l.max(0));
            if l <= 0 {
                end();
            }
        },
        std::time::Duration::from_millis(250),
    )
    .ok();
    on_cleanup(move || {
        if let Some(h) = h {
            h.clear();
        }
    });
    let next = move || {
        picked.set(None);
        let avoid = q.get_untracked().cur.k;
        q.set(pool.with_value(|p| flash_q(p, Some(&avoid))));
    };
    move || {
        if ended.get() {
            let sc = score.get_untracked();
            let rec = best("flash");
            let sub =
                if rec <= sc as f64 && sc > 0 { "🏆 Novo recorde!".to_string() } else { format!("Recorde: {rec}") };
            return view! { {head("⚡ Relâmpago", back, view! { <span></span> }.into_any())}{result_card(format!("{sc} acertos"), sub, again)} }.into_any();
        }
        let fq = q.get();
        let hud = view! {
            <span class="hud"><span>"⏱ "<span class="hv num">{move || left.get()}</span></span><span>"✔ "<span class="hv num">{move || score.get()}</span></span><span>{move || format!("×{}", 1 + combo.get() / 5)}</span></span>
        }
        .into_any();
        let opts: Vec<(String, String)> =
            fq.opts.iter().map(|o| (o.k.clone(), if fq.reverse { o.k.clone() } else { o.r.clone() })).collect();
        let cur = fq.cur.clone();
        let shown_at = fq.shown_at;
        view! {
            {head("⚡ Relâmpago", back, hud)}
            <div class="card stack">
                <div class="bigq" style=if fq.reverse { "font-family:var(--f-sign);font-size:64px" } else { "" }>{if fq.reverse { fq.cur.r.clone() } else { fq.cur.k.clone() }}</div>
                {options(opts, fq.cur.k.clone(), picked, fq.reverse, false, move |k| {
                    if ended.get_untracked() { return; }
                    let ok = k == cur.k;
                    kana_stat(&cur.k, ok, js_sys::Date::now() - shown_at);
                    if ok {
                        score.update(|s| *s += 1 + combo.get_untracked() / 5);
                        combo.update(|c| *c += 1);
                        next();
                    } else {
                        combo.set(0);
                        say(&cur.k, None);
                        set_timeout(move || if score.try_get_untracked().is_some() { next() }, std::time::Duration::from_millis(900));
                    }
                })}
            </div>
        }
        .into_any()
    }
}

/* ---------------- 雨 Chuva ---------------- */

struct Drop {
    x: KanaItem,
    el: web_sys::Element,
    y: f64,
    born: f64,
}

struct RainState {
    drops: Vec<Drop>,
    speed: f64,
    last: f64,
    spawn_t: f64,
    alive: bool,
}

#[component]
fn Rain(pool: Vec<KanaItem>, back: Callback<()>, again: Callback<()>) -> impl IntoView {
    let arena = NodeRef::<Div>::new();
    let input = NodeRef::<Input>::new();
    let score = RwSignal::new(0u32);
    let lives = RwSignal::new(3i32);
    let ended = RwSignal::new(false);
    let st = Rc::new(RefCell::new(RainState { drops: vec![], speed: 32.0, last: 0.0, spawn_t: 0.0, alive: true }));
    let pool = Rc::new(pool);

    let end = move || {
        if ended.get_untracked() {
            return;
        }
        ended.set(true);
        best_of("rain", score.get_untracked() as f64);
        game_done((5 + score.get_untracked()).min(40) as f64);
    };
    let hit = {
        let st = st.clone();
        move |idx: usize| {
            let mut s = st.borrow_mut();
            let d = s.drops.remove(idx);
            let _ = d.el.class_list().add_1("hit");
            let el = d.el.clone();
            set_timeout(move || el.remove(), std::time::Duration::from_millis(300));
            kana_stat(&d.x.k, true, js_sys::Date::now() - d.born);
            score.update(|v| *v += 1);
            if score.get_untracked() % 10 == 0 {
                s.speed += 9.0;
            }
        }
    };

    // requestAnimationFrame loop
    {
        let st_keep = st.clone();
        let st = st.clone();
        let pool = pool.clone();
        let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
        let g = f.clone();
        *g.borrow_mut() = Some(Closure::new(move |t: f64| {
            // Left the game: stop before touching any (disposed) signal.
            if !st.borrow().alive {
                return;
            }
            let Some(ar) = arena.get_untracked() else { return };
            let ar: web_sys::HtmlElement = ar.into();
            {
                let mut s = st.borrow_mut();
                if !s.alive {
                    return;
                }
                if s.last == 0.0 {
                    s.last = t;
                }
                let dt = ((t - s.last) / 1000.0).min(0.05);
                s.last = t;
                s.spawn_t -= dt;
                if s.spawn_t <= 0.0 {
                    let x = weighted_kana(&pool, None);
                    if let Ok(d) = crate::util::document().create_element("div") {
                        d.set_class_name("drop");
                        d.set_text_content(Some(&x.k));
                        let w = ar.client_width() as f64;
                        let _ = d.set_attribute("style", &format!("left:{}px", 10.0 + rand() * (w - 90.0).max(10.0)));
                        let _ = ar.append_child(&d);
                        s.drops.push(Drop { x, el: d, y: -50.0, born: js_sys::Date::now() });
                    }
                    s.spawn_t = (2.2 - score.get_untracked() as f64 / 25.0).max(0.7);
                }
                let h = ar.client_height() as f64;
                let speed = s.speed;
                let mut fallen = Vec::new();
                for (i, d) in s.drops.iter_mut().enumerate() {
                    d.y += speed * dt;
                    let style = d.el.get_attribute("style").unwrap_or_default();
                    let left = style.split(';').find(|p| p.starts_with("left")).unwrap_or("").to_string();
                    let _ = d.el.set_attribute("style", &format!("{left};transform:translateY({}px)", d.y));
                    if d.y > h - 46.0 {
                        fallen.push(i);
                    }
                }
                for i in fallen.into_iter().rev() {
                    let d = s.drops.remove(i);
                    d.el.remove();
                    kana_stat(&d.x.k, false, 0.0);
                    lives.update(|l| *l -= 1);
                    toast(&format!("{} = {}", d.x.k, d.x.r), "");
                }
                if lives.get_untracked() <= 0 {
                    s.alive = false;
                }
            }
            if lives.get_untracked() <= 0 {
                end();
                return;
            }
            if let Some(cb) = f.borrow().as_ref() {
                let _ = crate::util::window().request_animation_frame(cb.as_ref().unchecked_ref());
            }
        }));
        if let Some(cb) = g.borrow().as_ref() {
            let _ = crate::util::window().request_animation_frame(cb.as_ref().unchecked_ref());
        }
        // The frame closure must outlive this scope; it stops itself via `alive`.
        let keep = send_wrapper::SendWrapper::new((g, st_keep));
        on_cleanup(move || {
            keep.1.borrow_mut().alive = false;
        });
    }
    set_timeout(
        move || {
            if let Some(i) = input.get_untracked() {
                let _ = i.focus();
            }
        },
        std::time::Duration::from_millis(50),
    );

    let st_in = st.clone();
    let hit_in = hit.clone();
    let on_input = move |_| {
        let Some(inp) = input.get_untracked() else { return };
        let v = inp.value().trim().to_lowercase();
        if v.is_empty() {
            return;
        }
        let (idx, longer, any_prefix) = {
            let s = st_in.borrow();
            let idx = s.drops.iter().position(|d| d.x.alts.contains(&v));
            let longer = s
                .drops
                .iter()
                .enumerate()
                .any(|(i, d)| Some(i) != idx && d.x.alts.iter().any(|a| a.starts_with(&v) && *a != v));
            let any_prefix = s.drops.iter().any(|d| d.x.alts.iter().any(|a| a.starts_with(&v)));
            (idx, longer, any_prefix)
        };
        if let (Some(i), false) = (idx, longer) {
            hit_in(i);
            inp.set_value("");
        } else if !any_prefix {
            inp.set_value("");
        }
    };
    let st_k = st.clone();
    let on_key = move |e: web_sys::KeyboardEvent| {
        if e.key() != "Enter" {
            return;
        }
        let Some(inp) = input.get_untracked() else { return };
        let v = inp.value().trim().to_lowercase();
        inp.set_value("");
        let idx = st_k.borrow().drops.iter().position(|d| d.x.alts.contains(&v));
        if let Some(i) = idx {
            hit(i);
        }
    };
    let hud = view! {
        <span class="hud"><span>"✔ "<span class="hv num">{move || score.get()}</span></span>
            <span>{move || { let l = lives.get().max(0) as usize; format!("{}{}", "❤".repeat(l), "♡".repeat(3 - l.min(3))) }}</span></span>
    }
    .into_any();
    view! {
        {head("雨 Chuva de Kana", back, hud)}
        {move || ended.get().then(|| {
            let sc = score.get_untracked();
            let rec = best("rain");
            result_card(format!("{sc} kana"), if rec <= sc as f64 && sc > 0 { "🏆 Novo recorde!".into() } else { format!("Recorde: {rec}") }, again)
        })}
        <div class="arena" node_ref=arena style:display=move || if ended.get() { "none" } else { "" }></div>
        <input type="text" node_ref=input autocomplete="off" autocapitalize="off" spellcheck="false" placeholder="digite o romaji aqui"
            style="font-size:20px;text-align:center" aria-label="Romaji" style:display=move || if ended.get() { "none" } else { "" }
            on:input=on_input on:keydown=on_key/>
    }
}

/* ---------------- 対 Pares / 漢 Kanji × significado ---------------- */

#[derive(Clone)]
struct MemCard {
    i: usize,
    t: String,
    sm: bool,
}

#[component]
fn MemoryBoard(
    cards: Vec<MemCard>,
    pairs: usize,
    say_of: Vec<String>,
    kanji: bool,
    on_won: Callback<u32>,
    again: Callback<()>,
    moves: RwSignal<u32>,
) -> impl IntoView {
    let n = cards.len();
    let up = RwSignal::new(vec![false; n]);
    let gone = RwSignal::new(vec![false; n]);
    let open = RwSignal::new(Vec::<usize>::new());
    let found = RwSignal::new(0usize);
    let won = RwSignal::new(false);
    let cards = StoredValue::new(cards);
    let say_of = StoredValue::new(say_of);
    let t0 = js_sys::Date::now();
    let secs = RwSignal::new(0i64);
    let click = move |k: usize| {
        if up.get_untracked()[k] || gone.get_untracked()[k] || open.get_untracked().len() >= 2 {
            return;
        }
        up.update(|u| u[k] = true);
        open.update(|o| o.push(k));
        let o = open.get_untracked();
        if o.len() == 2 {
            moves.update(|m| *m += 1);
            let (a, c) = (o[0], o[1]);
            let same = cards.with_value(|cs| cs[a].i == cs[c].i);
            if same {
                set_timeout(
                    move || {
                        if gone.try_get_untracked().is_none() {
                            return;
                        }
                        gone.update(|g| {
                            g[a] = true;
                            g[c] = true;
                        });
                        open.set(vec![]);
                        found.update(|f| *f += 1);
                        let idx = cards.with_value(|cs| cs[a].i);
                        if !kanji {
                            if let Some(s) = say_of.with_value(|v| v.get(idx).cloned()) {
                                say(&s, None);
                            }
                        }
                        if found.get_untracked() == pairs {
                            secs.set(((js_sys::Date::now() - t0) / 1000.0).round() as i64);
                            won.set(true);
                            on_won.run(moves.get_untracked());
                        }
                    },
                    std::time::Duration::from_millis(350),
                );
            } else {
                set_timeout(
                    move || {
                        if up.try_get_untracked().is_none() {
                            return;
                        }
                        up.update(|u| {
                            u[a] = false;
                            u[c] = false;
                        });
                        open.set(vec![]);
                    },
                    std::time::Duration::from_millis(if kanji { 1000 } else { 900 }),
                );
            }
        }
    };
    let grid = cards.with_value(|cs| {
        cs.iter()
            .enumerate()
            .map(|(k, c)| {
                let style = if kanji && !c.sm { "font-family:var(--f-hand);font-size:34px" } else { "" };
                view! {
                    <button class="mem" class:sm=c.sm class:up=move || up.get()[k] class:gone=move || gone.get()[k] style=style on:click=move |_| click(k)>{c.t.clone()}</button>
                }
            })
            .collect_view()
    });
    view! {
        <div class="memgrid" style=if kanji { "grid-template-columns:repeat(auto-fill,minmax(96px,1fr))" } else { "" }>{grid}</div>
        {move || won.get().then(|| view! {
            <div class="banner ok"><span class="bico">"🎉"</span>
                <div class="grow">{if kanji { format!("{} jogadas.", moves.get_untracked()) } else { format!("{} jogadas em {}s.", moves.get_untracked(), secs.get_untracked()) }}</div>
                <button class="btn sm primary" on:click=move |_| again.run(())>"De novo"</button>
            </div>
        })}
    }
}

#[component]
fn Memory(pool: Vec<KanaItem>, spec: String, back: Callback<()>, again: Callback<()>) -> impl IntoView {
    let hk = spec.starts_with("hk");
    let base: Vec<KanaItem> = shuffle(&pool.iter().filter(|x| !hk || x.script == 'h').cloned().collect::<Vec<_>>())
        .into_iter()
        .take(8)
        .collect();
    let cards: Vec<MemCard> = shuffle(
        &base
            .iter()
            .enumerate()
            .flat_map(|(i, x)| {
                if hk {
                    vec![MemCard { i, t: x.k.clone(), sm: false }, MemCard { i, t: to_kata(&x.k), sm: false }]
                } else {
                    vec![MemCard { i, t: x.k.clone(), sm: false }, MemCard { i, t: x.r.clone(), sm: true }]
                }
            })
            .collect::<Vec<_>>(),
    );
    let pairs = base.len();
    let say_of: Vec<String> = base.iter().map(|x| x.k.clone()).collect();
    let moves = RwSignal::new(0u32);
    let hud = view! { <span class="hud">"Jogadas "<span class="hv num">{move || moves.get()}</span></span> }.into_any();
    view! {
        {head("対 Pares", back, hud)}
        <MemoryBoard cards=cards pairs=pairs say_of=say_of kanji=false moves=moves again=again on_won=Callback::new(move |_| game_done(15.0))/>
    }
}

fn learned_kanji() -> Vec<Kanji> {
    let all = data::kanji().unwrap_or_default();
    all.iter().filter(|k| store::card(&format!("k:{}", k.k)).is_some()).cloned().collect()
}

#[component]
fn KMatch(back: Callback<()>, again: Callback<()>) -> impl IntoView {
    let mut learned = learned_kanji();
    if learned.len() < 8 {
        learned = data::kanji().unwrap_or_default().iter().take(40).cloned().collect();
    }
    let base: Vec<Kanji> = shuffle(&learned).into_iter().take(6).collect();
    let cards: Vec<MemCard> = shuffle(
        &base
            .iter()
            .enumerate()
            .flat_map(|(i, x)| {
                vec![
                    MemCard { i, t: x.k.clone(), sm: false },
                    MemCard { i, t: x.en.first().cloned().unwrap_or_default(), sm: true },
                ]
            })
            .collect::<Vec<_>>(),
    );
    let moves = RwSignal::new(0u32);
    let hud = view! { <span class="hud">"Jogadas "<span class="hv num">{move || moves.get()}</span></span> }.into_any();
    view! {
        {head("漢 Kanji × significado", back, hud)}
        <MemoryBoard cards=cards pairs=base.len() say_of=vec![] kanji=true moves=moves again=again on_won=Callback::new(move |_| game_done(15.0))/>
    }
}

/* ---------------- round-based multiple choice (Ouvido, Katakana, Leitura) ---------------- */

#[derive(Clone)]
struct Round {
    /// HTML of the prompt.
    prompt: String,
    opts: Vec<(String, String)>,
    right: String,
    /// spoken after answering (and at the start for "listen")
    say: String,
    hint: Option<String>,
}

#[component]
fn Rounds(
    title: &'static str,
    total: usize,
    make: Callback<(), Round>,
    back: Callback<()>,
    again: Callback<()>,
    key: &'static str,
    #[prop(optional)] listen: bool,
    #[prop(optional)] one_col: bool,
    #[prop(optional)] jp: bool,
    #[prop(optional)] stat: bool,
) -> impl IntoView {
    let n = RwSignal::new(0usize);
    let ok = RwSignal::new(0usize);
    let round = RwSignal::new(make.run(()));
    let picked = RwSignal::new(None::<String>);
    let hint = RwSignal::new(None::<String>);
    let done = RwSignal::new(false);
    if listen {
        say(&round.get_untracked().say, Some(0.8));
    }
    let next = move || {
        n.update(|x| *x += 1);
        if n.get_untracked() >= total {
            done.set(true);
            game_done(5.0 + ok.get_untracked() as f64);
            if !key.is_empty() {
                best_of(key, ok.get_untracked() as f64);
            }
            return;
        }
        picked.set(None);
        hint.set(None);
        round.set(make.run(()));
        if listen {
            say(&round.get_untracked().say, Some(0.8));
        }
    };
    move || {
        if done.get() {
            return view! { {head(title, back, view! { <span></span> }.into_any())}{result_card(format!("{}/{}", ok.get_untracked(), total), String::new(), again)} }.into_any();
        }
        let r = round.get();
        let hud = view! { <span class="hud">{move || format!("{}/{} · ✔ {}", n.get() + 1, total, ok.get())}</span> }
            .into_any();
        let r2 = r.clone();
        let play = listen.then(|| {
            let s = r.say.clone();
            view! { <button class="btn primary" style="font-size:22px;min-height:70px;padding:0 30px" on:click=move |_| say(&s, Some(0.7))>"🔊 Ouvir de novo"</button> }
        });
        let hint_btn = r.hint.clone().map(|h| {
            let speak = spk(&r.say, "btn sm");
            view! {
                <div style="text-align:center"><span inner_html=speak></span>" "
                    <button class="btn sm ghost" on:click=move |_| hint.set(Some(h.clone()))>"Mostrar romaji"</button>" "
                    <span class="small muted">{move || hint.get().unwrap_or_default()}</span></div>
            }
        });
        view! {
            {head(title, back, hud)}
            <div class="card stack" style=if listen { "align-items:center" } else { "" }>
                {play}
                <div inner_html=r.prompt.clone() style="width:100%"></div>
                {hint_btn}
                <div style="width:100%">{options(r.opts.clone(), r.right.clone(), picked, jp, one_col, move |k| {
                    let good = k == r2.right;
                    if good { ok.update(|x| *x += 1); }
                    if stat { kana_stat(&r2.right, good, 0.0); }
                    if !listen { say(&r2.say, None); }
                    set_timeout(move || if n.try_get_untracked().is_some() { next() }, std::time::Duration::from_millis(match (listen, good) { (true, true) => 500, (true, false) => 1300, (false, true) => 700, _ => 1500 }));
                })}</div>
            </div>
        }
        .into_any()
    }
}

#[component]
fn Listen(pool: Vec<KanaItem>, back: Callback<()>, again: Callback<()>) -> impl IntoView {
    let pool = StoredValue::new(pool);
    let last = StoredValue::new(String::new());
    let make = Callback::new(move |_| {
        let cur = pool.with_value(|p| weighted_kana(p, Some(&last.get_value())));
        last.set_value(cur.k.clone());
        let others: Vec<KanaItem> = pool.with_value(|p| {
            shuffle(&p.iter().filter(|x| x.r != cur.r).cloned().collect::<Vec<_>>()).into_iter().take(3).collect()
        });
        let mut opts = vec![(cur.k.clone(), cur.k.clone())];
        opts.extend(others.into_iter().map(|o| (o.k.clone(), o.k)));
        Round { prompt: String::new(), opts: shuffle(&opts), right: cur.k.clone(), say: cur.k, hint: None }
    });
    view! { <Rounds title="耳 Ouvido" total=15 make=make back=back again=again key="" listen=true jp=true stat=true/> }
}

#[component]
fn Katakana(back: Callback<()>, again: Callback<()>) -> impl IntoView {
    let words = RwSignal::new(None::<Vec<data::Vocab>>);
    spawn_local(async move {
        let v = data::need_vocab().await.unwrap_or_default();
        let w: Vec<data::Vocab> = v
            .iter()
            .filter(|x| {
                x.0.chars().count() >= 2 && x.0.chars().all(|c| ('\u{30A0}'..='\u{30FF}').contains(&c) || c == 'ー')
            })
            .cloned()
            .collect();
        words.set(Some(w));
    });
    move || match words.get() {
        None => view! { <div class="loading"><div class="spin"></div>"Carregando palavras…"</div> }.into_any(),
        Some(w) if w.len() < 4 => view! { <div class="empty">"Sem palavras em katakana."</div> }.into_any(),
        Some(w) => {
            let w = StoredValue::new(w);
            let make = Callback::new(move |_| {
                w.with_value(|w| {
                    let cur = pick(w).unwrap_or_default();
                    let others: Vec<data::Vocab> =
                        shuffle(&w.iter().filter(|x| x.2 != cur.2).cloned().collect::<Vec<_>>())
                            .into_iter()
                            .take(3)
                            .collect();
                    let mut opts = vec![(cur.0.clone(), cur.2.split(',').next().unwrap_or("").to_string())];
                    opts.extend(
                        others.into_iter().map(|o| (o.0.clone(), o.2.split(',').next().unwrap_or("").to_string())),
                    );
                    Round {
                        prompt: format!(
                            r#"<div class="bigq jp" style="font-family:var(--f-jp);font-size:56px">{}</div>"#,
                            esc(&cur.0)
                        ),
                        opts: shuffle(&opts),
                        right: cur.0.clone(),
                        say: cur.0.clone(),
                        hint: Some(romaji(&cur.0)),
                    }
                })
            });
            view! { <Rounds title="外 Detetive Katakana" total=12 make=make back=back again=again key="katakana" one_col=true/> }.into_any()
        }
    }
}

#[component]
fn KRead(back: Callback<()>, again: Callback<()>) -> impl IntoView {
    let all = data::kanji().unwrap_or_default();
    let mut learned: Vec<Kanji> = learned_kanji().into_iter().filter(|k| !k.ex.is_empty()).collect();
    if learned.len() < 10 {
        learned = all.iter().take(80).filter(|k| !k.ex.is_empty()).cloned().collect();
    }
    let words: Vec<(String, String, String)> = learned
        .iter()
        .flat_map(|k| {
            k.ex.iter()
                .filter(|e| e.0 != e.1 && e.0.chars().any(is_kanji))
                .map(|e| (e.0.clone(), e.1.clone(), e.2.clone()))
        })
        .collect();
    let all_r: Vec<String> = all.iter().take(600).flat_map(|k| k.ex.iter().map(|e| e.1.clone())).collect();
    let words = StoredValue::new(words);
    let all_r = StoredValue::new(all_r);
    let make = Callback::new(move |_| {
        let cur = words.with_value(|w| pick(w)).unwrap_or_default();
        let n = cur.1.chars().count() as i64;
        let mut similar: Vec<String> = all_r.with_value(|a| {
            a.iter().filter(|r| **r != cur.1 && (r.chars().count() as i64 - n).abs() <= 1).cloned().collect()
        });
        similar.sort();
        similar.dedup();
        let mut opts = vec![(cur.1.clone(), cur.1.clone())];
        opts.extend(shuffle(&similar).into_iter().take(3).map(|r| (r.clone(), r)));
        Round {
            prompt: format!(
                r#"<div class="bigq" style="font-family:var(--f-jp);font-size:58px">{}</div><div class="small muted" style="text-align:center">{}</div>"#,
                esc(&cur.0),
                esc(&cur.2)
            ),
            opts: shuffle(&opts),
            right: cur.1.clone(),
            say: cur.1.clone(),
            hint: None,
        }
    });
    view! { <Rounds title="読 Leitura certa" total=12 make=make back=back again=again key="kread" jp=true/> }
}

/* ---------------- 筆 / 書 writing ---------------- */

#[component]
fn WriteKana(pool: Vec<KanaItem>, back: Callback<()>) -> impl IntoView {
    let pool = StoredValue::new(pool);
    let blind = RwSignal::new(false);
    let cur = RwSignal::new(pool.with_value(|p| weighted_kana(p, None)));
    let paths = RwSignal::new(None::<Option<Vec<String>>>);
    let load = move || {
        let k = cur.get_untracked().k;
        paths.set(None);
        spawn_local(async move { paths.set(Some(strokes_of(&k).await)) });
    };
    load();
    let next = move || {
        let avoid = cur.get_untracked().k;
        cur.set(pool.with_value(|p| weighted_kana(p, Some(&avoid))));
        load();
    };
    let extra = view! { <label class="switch small"><input type="checkbox" prop:checked=move || blind.get() on:change=move |ev| { blind.set(ev_checked(&ev)); next(); }/>" sem guia"</label> }.into_any();
    view! {
        {head("筆 Escrever kana", back, extra)}
        <div class="card stack">
            {move || {
                let c = cur.get();
                let sp = spk(&c.k, "btn xs");
                view! { <div style="text-align:center"><span style="font-family:var(--f-sign);font-size:34px;font-weight:700">{c.r.clone()}</span>" "<span inner_html=sp></span></div> }
            }}
            {move || match paths.get() {
                None => view! { <div class="loading"><div class="spin"></div></div> }.into_any(),
                Some(None) => view! { <div class="empty small">"Sem dados de traço."</div> }.into_any(),
                Some(Some(p)) => {
                    let k = cur.get_untracked().k;
                    let k2 = k.clone();
                    view! { <StrokeQuiz ch=k paths=p blind=blind.get_untracked()
                        on_score=Callback::new(move |pct: u32| kana_stat(&k2, pct >= 80, 0.0))
                        on_out=Callback::new(move |_| next())/> }.into_any()
                }
            }}
        </div>
    }
}

#[component]
fn KWrite(back: Callback<()>) -> impl IntoView {
    let mut learned = learned_kanji();
    if learned.len() < 3 {
        learned = data::kanji().unwrap_or_default().iter().take(20).cloned().collect();
    }
    let learned = StoredValue::new(learned);
    let blind = RwSignal::new(true);
    let cur = RwSignal::new(learned.with_value(|l| pick(l)).unwrap_or_default());
    let paths = RwSignal::new(None::<Option<Vec<String>>>);
    let gen = RwSignal::new(0u32);
    let load = move || {
        let k = cur.get_untracked().k;
        paths.set(None);
        spawn_local(async move { paths.set(Some(strokes_of(&k).await)) });
    };
    load();
    let next = move || {
        let prev = cur.get_untracked().k;
        let c = learned
            .with_value(|l| pick(&l.iter().filter(|k| k.k != prev).cloned().collect::<Vec<_>>()))
            .unwrap_or_default();
        cur.set(c);
        gen.update(|g| *g += 1);
        load();
    };
    let extra = view! { <label class="switch small"><input type="checkbox" prop:checked=move || blind.get() on:change=move |ev| { blind.set(ev_checked(&ev)); next(); }/>" sem guia"</label> }.into_any();
    view! {
        {head("書 Escrever kanji", back, extra)}
        <div class="card stack">
            {move || {
                let c = cur.get();
                let readings: Vec<String> = c.kun.iter().take(2).chain(c.on.iter().take(1)).cloned().collect();
                view! {
                    <div style="text-align:center">
                        <div style="font-size:20px;font-weight:700">{c.en.iter().take(2).cloned().collect::<Vec<_>>().join(", ")}</div>
                        <div class="jp muted">{format!("{} · {} traços", readings.join("・"), c.s.unwrap_or(0))}</div>
                    </div>
                }
            }}
            {move || {
                gen.get();
                match paths.get() {
                    None => view! { <div class="loading"><div class="spin"></div></div> }.into_any(),
                    Some(None) => view! { <div class="empty small">"Sem dados de traço."</div> }.into_any(),
                    Some(Some(p)) => view! { <StrokeQuiz ch=cur.get_untracked().k paths=p blind=blind.get_untracked() on_out=Callback::new(move |_| next())/> }.into_any(),
                }
            }}
            <div class="row" style="justify-content:center">
                <button class="btn sm ghost" on:click=move |_| { let c = cur.get_untracked(); toast(&format!("{} — {}", c.k, c.en.first().cloned().unwrap_or_default()), ""); }>"Mostrar o kanji"</button>
                <button class="btn sm ghost" on:click=move |_| next()>"Pular"</button>
            </div>
        </div>
    }
}

/* ---------------- 活 Conjugador ---------------- */

#[component]
fn ConjGame(back: Callback<()>) -> impl IntoView {
    let lvl_n4 = store::read(|s| s.j.done.keys().any(|k| k.starts_with("N4")) || s.j.done.contains_key("N513"));
    let forms = RwSignal::new(FORMS.iter().filter(|f| f.2 == 5 || lvl_n4).map(|f| f.0.to_string()).collect::<Vec<_>>());
    let running = RwSignal::new(false);
    let gen = RwSignal::new(0u32);
    move || {
        if !running.get() {
            let checks = FORMS
                .iter()
                .map(|(fid, l, lv)| {
                    let fid = *fid;
                    view! {
                        <label class="switch small">
                            <input type="checkbox" prop:checked=forms.get_untracked().iter().any(|x| x == fid) on:change=move |ev| {
                                let on = ev_checked(&ev);
                                forms.update(|f| { f.retain(|x| x != fid); if on { f.push(fid.to_string()); } });
                            }/>
                            {format!(" {l}")}{(*lv == 4).then(|| view! { " "<span class="pill">"N4"</span> })}
                        </label>
                    }
                })
                .collect_view();
            return view! {
                {head("活 Conjugador", back, view! { <span></span> }.into_any())}
                <div class="card stack">
                    <p class="muted">"Escolha as formas que quer treinar. As do N4 abrem quando você chega na Linha N4 (ou marque mesmo assim)."</p>
                    <div class="row">{checks}</div>
                    <button class="btn primary" on:click=move |_| {
                        if forms.get_untracked().is_empty() { toast("Escolha ao menos uma forma.", ""); return; }
                        running.set(true);
                    }>"Começar"</button>
                </div>
            }
            .into_any();
        }
        gen.get();
        let verbs = data::content().map(|c| c.verbs.clone()).unwrap_or_default();
        let verbs = StoredValue::new(verbs);
        let make = Callback::new(move |_| {
            let v = verbs.with_value(|v| pick(v)).unwrap_or_default();
            let f = pick(&forms.get_untracked()).unwrap_or_else(|| "masu".into());
            let right = conj(&v, &f);
            let mut opts = vec![(right.clone(), right.clone())];
            opts.extend(conj_wrong(&v, &f).into_iter().map(|x| (x.clone(), x)));
            let cls = match v.3.as_str() {
                "u" => "う-verb",
                "ru" => "る-verb",
                _ => "irregular",
            };
            let label = FORMS.iter().find(|x| x.0 == f).map(|x| x.1).unwrap_or("");
            Round {
                prompt: format!(
                    r#"<div style="text-align:center"><div class="jp" style="font-size:44px">{}</div><div class="jp muted">{} · {} · <span class="pill">{}</span></div><div style="margin-top:8px;font-family:var(--f-sign);font-size:22px;font-weight:700;color:var(--line)">→ {}</div></div>"#,
                    esc(&v.0),
                    esc(&v.1),
                    esc(&v.2),
                    cls,
                    esc(label)
                ),
                opts: shuffle(&opts),
                right: right.clone(),
                say: right,
                hint: None,
            }
        });
        view! { <Rounds title="活 Conjugador" total=15 make=make back=back again=Callback::new(move |_| gen.update(|g| *g += 1)) key="conj" jp=true/> }.into_any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(k: &str, r: &str, c: &str) -> Verb {
        (k.into(), r.into(), String::new(), c.into(), 5)
    }

    #[test]
    fn conjugates() {
        assert_eq!(conj(&v("行く", "いく", "u"), "te"), "いって");
        assert_eq!(conj(&v("飲む", "のむ", "u"), "ta"), "のんだ");
        assert_eq!(conj(&v("書く", "かく", "u"), "te"), "かいて");
        assert_eq!(conj(&v("泳ぐ", "およぐ", "u"), "ta"), "およいだ");
        assert_eq!(conj(&v("話す", "はなす", "u"), "te"), "はなして");
        assert_eq!(conj(&v("買う", "かう", "u"), "nai"), "かわない");
        assert_eq!(conj(&v("ある", "ある", "u"), "nakatta"), "なかった");
        assert_eq!(conj(&v("食べる", "たべる", "ru"), "pot"), "たべられる");
        assert_eq!(conj(&v("勉強する", "べんきょうする", "irr"), "masu"), "べんきょうします");
        assert_eq!(conj(&v("来る", "くる", "irr"), "nai"), "こない");
        assert_eq!(conj(&v("待つ", "まつ", "u"), "vol"), "まとう");
    }
}
