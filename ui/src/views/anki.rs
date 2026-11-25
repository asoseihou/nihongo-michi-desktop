//! Anki (port of `18-anki.js`): export .txt with fixed GUIDs, import your
//! progress, and live sync with AnkiConnect (127.0.0.1:8765, via the backend).

use crate::app::toast;
use crate::data;
use crate::docs::MinedItem;
use crate::jp::{chunk_text, role, ruby};
use crate::store;
use crate::tauri;
use crate::util::{esc, ev_checked, ev_value, uid};
use leptos::prelude::*;
use michi_core::srs::mark_known;
use michi_core::time::now_ms;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use wasm_bindgen_futures::spawn_local;

fn tsv(s: &str) -> String {
    s.replace(['\t', '\r'], " ").replace('\n', "<br>")
}

fn header(deck: &str) -> String {
    format!("#separator:tab\n#html:true\n#notetype:Basic\n#deck:{deck}\n#guid column:1\n#tags column:4\n")
}

fn row(cols: [String; 4]) -> String {
    cols.iter().map(|c| tsv(c)).collect::<Vec<_>>().join("\t") + "\n"
}

/// Static stroke-order SVG for the card back (numbers at each stroke start).
fn stroke_svg_static(paths: &[String]) -> String {
    let nums: String = paths
        .iter()
        .enumerate()
        .filter_map(|(i, d)| {
            let t = d.trim_start_matches(['M', ' ']);
            let mut it = t.split(|c: char| c == ',' || c == ' ' || c.is_ascii_alphabetic()).filter(|x| !x.is_empty());
            let x: f64 = it.next()?.parse().ok()?;
            let y: f64 = it.next()?.parse().ok()?;
            Some(format!(
                r##"<text x="{:.1}" y="{:.1}" font-size="7" fill="#c0392b" font-family="sans-serif">{}</text>"##,
                x - 4.0,
                y - 1.0,
                i + 1
            ))
        })
        .collect();
    let ps: String = paths
        .iter()
        .map(|d| format!(r##"<path d="{d}" fill="none" stroke="#222" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>"##))
        .collect();
    format!(
        r#"<svg viewBox="0 0 109 109" width="150" height="150" style="border:1px solid #ccc;border-radius:6px;background:#fff">{ps}{nums}</svg>"#
    )
}

async fn export_kanji(scope: &str, with_strokes: bool) -> (String, usize) {
    let strokes = if with_strokes { data::need_strokes().await.ok() } else { None };
    let all = data::kanji().unwrap_or_default();
    let list: Vec<&data::Kanji> = all
        .iter()
        .filter(|k| match scope {
            "all" => true,
            "started" => store::card(&format!("k:{}", k.k)).is_some(),
            n => k.n.map(|x| x.to_string()) == Some(n.to_string()),
        })
        .collect();
    let mut out = header("Nihongo Michi::Kanji 2k");
    for k in &list {
        let ex = if k.ex.is_empty() {
            String::new()
        } else {
            "<hr>".to_string()
                + &k.ex
                    .iter()
                    .map(|e| format!("{}【{}】 {}", esc(&e.0), esc(&e.1), esc(&e.2)))
                    .collect::<Vec<_>>()
                    .join("<br>")
        };
        let svg = strokes
            .as_ref()
            .and_then(|s| s.get(&k.k))
            .map(|p| format!("<hr>{}", stroke_svg_static(p)))
            .unwrap_or_default();
        let back = format!(
            r#"<div style="font-size:20px"><b>{}</b></div><div>音: {}<br>訓: {}</div>{}{}"#,
            esc(&k.en.join(", ")),
            esc(&k.on.join("、")),
            esc(&k.kun.join("、")),
            ex,
            svg
        );
        out += &row([
            format!("michi-k-{}", k.k),
            format!(r#"<div style="font-size:80px">{}</div>"#, esc(&k.k)),
            back,
            format!("nihongo-michi kanji N{}", k.n.unwrap_or(0)),
        ]);
    }
    (out, list.len())
}

async fn export_vocab() -> (String, usize) {
    let v = data::need_vocab().await.unwrap_or_default();
    let list: Vec<&data::Vocab> = v.iter().filter(|x| x.3 == 5 || x.3 == 4).collect();
    let mut out = header("Nihongo Michi::Vocabulário");
    for x in &list {
        out += &row([
            format!("michi-v-{}-{}", x.0, x.1),
            format!(r#"<div style="font-size:42px">{}</div>"#, esc(&x.0)),
            format!(r#"<div style="font-size:26px">{}</div><div>{}</div>"#, esc(&x.1), esc(&x.2)),
            format!("nihongo-michi vocab N{}", x.3),
        ]);
    }
    (out, list.len())
}

fn export_mined() -> (String, usize) {
    let l = store::read(|s| s.mined.list.clone());
    let mut out = header("Nihongo Michi::Mineração");
    for m in &l {
        out += &row([
            format!("michi-m-{}", m.id),
            format!(
                r#"<div style="font-size:40px">{}</div>{}"#,
                esc(&m.front),
                if m.sentence.is_empty() { String::new() } else { format!("<div>{}</div>", esc(&m.sentence)) }
            ),
            format!(
                r#"<div style="font-size:24px">{}</div><div>{}</div>{}"#,
                esc(&m.reading),
                esc(&m.meaning),
                if m.note.is_empty() {
                    String::new()
                } else {
                    format!(r#"<div style="color:#666">{}</div>"#, esc(&m.note))
                }
            ),
            format!("nihongo-michi mined {}", m.source.split_whitespace().collect::<Vec<_>>().join("_")),
        ]);
    }
    (out, l.len())
}

fn export_sentences(travel: bool) -> (String, usize) {
    let c = data::content().unwrap_or_default();
    let mut out = header(if travel { "Nihongo Michi::Viagem" } else { "Nihongo Michi::Frases" });
    let mut n = 0;
    for l in c.lessons.iter().filter(|l| l.id.starts_with("V-") == travel) {
        for p in &l.points {
            for (i, s) in p.s.iter().enumerate() {
                let front = format!(
                    r#"<div style="font-size:28px">{}{}</div>"#,
                    s.c.iter().map(|c| ruby(&c.w, "always") + &esc(c.p.as_deref().unwrap_or(""))).collect::<String>(),
                    esc(&crate::jp::end_of(s))
                );
                let chunks: String = s
                    .c
                    .iter()
                    .map(|c| format!(r#"<span style="border-bottom:3px solid #999;margin-right:4px">{}<sub style="color:#888"> {}</sub></span>"#, esc(&chunk_text(c)), role(&c.r).1))
                    .collect();
                let back = format!(
                    r#"<div style="font-size:18px">{}</div><div style="color:#666">{} · {}</div><div>{}</div>"#,
                    esc(&s.en),
                    esc(&p.title),
                    esc(&p.pattern),
                    chunks
                );
                out += &row([
                    format!("michi-s-{}-{}", p.id, i),
                    front,
                    back,
                    format!("nihongo-michi {} L{}", if travel { "travel" } else { "grammar" }, l.id),
                ]);
                n += 1;
            }
        }
    }
    (out, n)
}

#[derive(Serialize)]
struct SaveArgs {
    name: String,
    content: String,
}

async fn save(name: &str, content: String) {
    match tauri::invoke::<Option<String>, _>("save_text", &SaveArgs { name: name.into(), content }).await {
        Ok(Some(p)) => toast(&format!("Arquivo pronto: {p}"), "ok"),
        Ok(None) => {}
        Err(e) => toast(&format!("Não deu para salvar ({e})."), "bad"),
    }
}

#[derive(Serialize)]
struct Ac {
    action: String,
    params: Value,
}

async fn anki_connect(action: &str, params: Value) -> Result<Value, String> {
    tauri::invoke("anki_connect", &Ac { action: action.into(), params }).await
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    let mut br = 0;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            '[' if !tag => br += 1,
            ']' if !tag && br > 0 => br -= 1,
            _ if !tag && br == 0 => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ").trim().to_string()
}

/// `markWordsKnown`: vocabulary cards for words you already know in Anki.
fn mark_words_known(words: &BTreeSet<String>) -> usize {
    let Some(v) = data::vocab() else { return 0 };
    let now = now_ms();
    let mut n = 0;
    for x in v.iter() {
        let id = format!("v:{}|{}", x.0, x.1);
        if words.contains(&x.0) && store::card(&id).is_none() {
            store::set_card(&id, Some(mark_known(21.0, now, crate::util::rand(), crate::util::rand())));
            n += 1;
        }
    }
    n
}

#[component]
pub fn Anki() -> impl IntoView {
    spawn_local(async {
        let _ = data::need_content().await;
        let _ = data::need_kanji().await;
    });
    let scope = RwSignal::new("started".to_string());
    let strokes = RwSignal::new(true);
    let busy = RwSignal::new(false);
    let import_text = RwSignal::new(String::new());
    let analysis = RwSignal::new(None::<(usize, BTreeSet<String>, BTreeSet<String>, Vec<Vec<String>>)>);
    let ac_out = RwSignal::new(String::new());
    let n_mined = store::read(|s| s.mined.list.len());

    let run_export = move |what: &'static str| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let (r, name) = match what {
                "kanji" => {
                    let sc = scope.get_untracked();
                    (export_kanji(&sc, strokes.get_untracked()).await, format!("michi-kanji-{sc}.txt"))
                }
                "vocab" => (export_vocab().await, "michi-vocab-n5-n4.txt".into()),
                "mined" => (export_mined(), "michi-mineracao.txt".into()),
                "sent" => (export_sentences(false), "michi-frases.txt".into()),
                _ => (export_sentences(true), "michi-viagem.txt".into()),
            };
            if r.1 == 0 {
                toast("Nada para exportar ainda.", "");
            } else {
                save(&name, r.0).await;
            }
            busy.set(false);
        });
    };
    let analyze = move || {
        let raw = import_text.get_untracked();
        spawn_local(async move {
            let _ = data::need_vocab().await;
            let rows: Vec<Vec<String>> = raw
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                .map(|l| l.split('\t').map(strip_html).collect())
                .collect();
            let kmap = data::kanji_index().unwrap_or_default();
            let vocab = data::vocab().unwrap_or_default();
            let mut kan = BTreeSet::new();
            let mut words = BTreeSet::new();
            let mut others = Vec::new();
            for r in &rows {
                let f = r.first().cloned().unwrap_or_default();
                if f.chars().count() == 1 && kmap.contains_key(&f) {
                    kan.insert(f);
                } else if vocab.iter().any(|v| v.0 == f) {
                    words.insert(f);
                } else {
                    others.push(r.clone());
                }
            }
            analysis.set(Some((rows.len(), kan, words, others)));
        });
    };
    let ac = move |what: &'static str| {
        ac_out.set("Conversando com o Anki…".into());
        spawn_local(async move {
            let r: Result<String, String> = async {
                match what {
                    "test" => Ok(format!("Conectado! AnkiConnect versão {}", anki_connect("version", json!({})).await?)),
                    "decks" => {
                        let d = anki_connect("deckNames", json!({})).await?;
                        let names: Vec<String> = d.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
                        Ok(format!("Baralhos: {}", names.join(" · ")))
                    }
                    "push" => {
                        let l: Vec<MinedItem> = store::read(|s| s.mined.list.iter().filter(|m| !m.extra.contains_key("anki")).cloned().collect());
                        if l.is_empty() {
                            return Ok("Nada novo para enviar.".into());
                        }
                        anki_connect("createDeck", json!({"deck": "Nihongo Michi::Mineração"})).await?;
                        let notes: Vec<Value> = l
                            .iter()
                            .map(|m| {
                                json!({"deckName": "Nihongo Michi::Mineração", "modelName": "Basic",
                                    "fields": {"Front": format!(r#"<div style="font-size:40px">{}</div>{}"#, esc(&m.front), esc(&m.sentence)), "Back": format!("{}<br>{}", esc(&m.reading), esc(&m.meaning))},
                                    "tags": ["nihongo-michi"], "options": {"allowDuplicate": false}})
                            })
                            .collect();
                        let res = anki_connect("addNotes", json!({"notes": notes})).await?;
                        let ids: Vec<Value> = res.as_array().cloned().unwrap_or_default();
                        let mut ok = 0;
                        store::write(&["mined"], |s| {
                            for (i, id) in ids.iter().enumerate() {
                                if !id.is_null() {
                                    if let Some(m) = l.get(i) {
                                        if let Some(x) = s.mined.list.iter_mut().find(|x| x.id == m.id) {
                                            x.extra.insert("anki".into(), id.clone());
                                            ok += 1;
                                        }
                                    }
                                }
                            }
                        });
                        Ok(format!("{ok} cartas enviadas ao Anki."))
                    }
                    _ => {
                        let ids = anki_connect("findCards", json!({"query": "deck:Kaishi* prop:ivl>=21"})).await?;
                        let ids: Vec<Value> = ids.as_array().cloned().unwrap_or_default();
                        if ids.is_empty() {
                            return Ok("Nenhum cartão maduro encontrado em decks \"Kaishi*\".".into());
                        }
                        let _ = data::need_vocab().await;
                        let info = anki_connect("cardsInfo", json!({"cards": ids.iter().take(3000).collect::<Vec<_>>()})).await?;
                        let words: Vec<String> = info
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|c| {
                                        let f = c.get("fields")?.as_object()?;
                                        let first = f.values().min_by_key(|x| x.get("order").and_then(Value::as_i64).unwrap_or(0))?;
                                        Some(strip_html(first.get("value")?.as_str()?))
                                    })
                                    .filter(|w| !w.is_empty())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let set: BTreeSet<String> = words.iter().cloned().collect();
                        let n = mark_words_known(&set);
                        store::write(&["profile"], |s| {
                            s.p.extra.insert("ankiKnown".into(), json!(words.iter().take(3000).collect::<Vec<_>>()));
                        });
                        Ok(format!("{} palavras maduras lidas; {n} batem com o vocabulário do app e foram marcadas como conhecidas.", words.len()))
                    }
                }
            }
            .await;
            ac_out.set(match r {
                Ok(s) => s,
                Err(e) => format!("⚠️ {e}"),
            });
        });
    };

    view! {
        <div class="row-between"><h1>"Anki "<span class="jp muted">"暗記"</span></h1><span class="small muted">"Seu Anki e o Michi trabalhando juntos"</span></div>
        <div class="grid2">
            <div class="card stack"><h2>"1 · Exportar para o Anki"</h2>
                <p class="small muted">"Gera um arquivo "<b>".txt"</b>" no formato de importação do Anki (Arquivo → Importar). Os cartões têm GUID fixo: importar de novo atualiza em vez de duplicar."</p>
                <div class="field"><label>"Kanji 2k"</label>
                    <div class="row">
                        <select on:change=move |ev| scope.set(ev_value(&ev))>
                            <option value="started">"Só os que já iniciei"</option><option value="5">"N5"</option><option value="4">"N4"</option>
                            <option value="3">"N3"</option><option value="2">"N2"</option><option value="1">"N1"</option><option value="all">"Todos (2.383)"</option>
                        </select>
                        <label class="switch small"><input type="checkbox" prop:checked=true on:change=move |ev| strokes.set(ev_checked(&ev))/>" com ordem dos traços"</label>
                        <button class="btn sm primary" disabled=move || busy.get() on:click=move |_| run_export("kanji")>"Exportar"</button>
                    </div>
                </div>
                <div class="row">
                    <button class="btn sm" disabled=move || busy.get() on:click=move |_| run_export("mined")>{format!("⛏️ Mineração ({n_mined})")}</button>
                    <button class="btn sm" disabled=move || busy.get() on:click=move |_| run_export("sent")>"文 Frases N5–N3"</button>
                    <button class="btn sm" disabled=move || busy.get() on:click=move |_| run_export("travel")>"✈️ Frases de viagem"</button>
                    <button class="btn sm" disabled=move || busy.get() on:click=move |_| run_export("vocab")>"語彙 Vocabulário N5–N4"</button>
                </div>
            </div>
            <div class="card stack"><h2>"2 · Trazer o seu progresso"</h2>
                <p class="small muted">"No Anki: Navegar → selecione os cartões (ex.: "<span class="jp">"deck:Kaishi* prop:ivl>=21"</span>") → Notas → Exportar notas → "<b>"Notas em texto simples"</b>". Depois escolha o arquivo aqui ou cole o conteúdo."</p>
                <div class="row"><button class="btn sm" on:click=move |_| spawn_local(async move {
                    #[derive(Serialize)] struct E { exts: Vec<&'static str> }
                    if let Ok(Some(t)) = tauri::invoke::<Option<String>, _>("open_text", &E { exts: vec!["txt", "csv", "tsv"] }).await {
                        import_text.set(t);
                        analyze();
                    }
                })>"📂 Escolher arquivo…"</button></div>
                <textarea placeholder="…ou cole aqui as linhas exportadas" style="min-height:70px" prop:value=move || import_text.get() on:input=move |ev| import_text.set(ev_value(&ev))></textarea>
                <button class="btn sm" on:click=move |_| analyze()>"Analisar"</button>
                {move || analysis.get().map(|(n, kan, words, others)| {
                    let (nk, nw, no) = (kan.len(), words.len(), others.len());
                    let kan2 = kan.clone();
                    let words2 = words.clone();
                    let others2 = others.clone();
                    view! {
                        <div class="small">{format!("{n} linhas: ")}<b>{nk}</b>" kanji isolados, "<b>{nw}</b>{format!(" palavras do vocabulário JLPT, {no} outras.")}</div>
                        <div class="row">
                            {(nk > 0).then(|| view! { <button class="btn sm" on:click=move |_| {
                                let now = now_ms();
                                for k in &kan2 { let id = format!("k:{k}"); if store::card(&id).is_none() { store::set_card(&id, Some(mark_known(21.0, now, crate::util::rand(), crate::util::rand()))); } }
                                toast(&format!("{} kanji marcados.", kan2.len()), "ok");
                            }>{format!("Marcar {nk} kanji como conhecidos")}</button> })}
                            {(nw > 0).then(|| view! { <button class="btn sm" on:click=move |_| { toast(&format!("{} palavras marcadas.", mark_words_known(&words2)), "ok"); }>{format!("Marcar {nw} palavras como conhecidas")}</button> })}
                            {(no > 0).then(|| view! { <button class="btn sm" on:click=move |_| {
                                let now = now_ms();
                                store::write(&["mined"], |s| for r in others2.iter().take(500) {
                                    s.mined.list.push(MinedItem { id: uid(), front: r.first().cloned().unwrap_or_default(), meaning: r.get(1).cloned().unwrap_or_default(), source: "Anki".into(), added: now, ..MinedItem::default() });
                                });
                                toast("Importado para Mineração.", "ok");
                            }>{format!("Importar {} como Mineração", no.min(500))}</button> })}
                        </div>
                    }
                })}
            </div>
        </div>
        <div class="card stack"><h2>"3 · Sincronização ao vivo (AnkiConnect)"</h2>
            <p class="small">"Com o Anki aberto e o add-on AnkiConnect, o Michi conversa direto com o seu Anki."</p>
            <div class="row">
                <button class="btn sm" on:click=move |_| ac("test")>"Testar conexão"</button>
                <button class="btn sm" on:click=move |_| ac("decks")>"Ver baralhos"</button>
                <button class="btn sm primary" on:click=move |_| ac("push")>"Enviar Mineração → Anki"</button>
                <button class="btn sm" on:click=move |_| ac("pull")>"Ler cartões maduros do Kaishi"</button>
            </div>
            <div class="small">{move || ac_out.get()}</div>
            <div class="small muted">"No Anki: Ferramentas → Complementos → Obter complementos → código "<b>"2055492159"</b>" (AnkiConnect), e reinicie o Anki."</div>
        </div>
    }
}
