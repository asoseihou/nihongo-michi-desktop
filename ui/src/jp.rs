//! Japanese helpers (port of `02-jp.js`): kana tables, romaji, Anki-style
//! furigana `漢字[かんじ]`, sentence chunks and grammatical roles.

use crate::data::{Chunk, Sentence};
use crate::util::esc;
use std::collections::HashMap;
use std::sync::OnceLock;

/* ---------------- kana tables ---------------- */

const KANA_ROWS: [(&str, &str, &[&str]); 10] = [
    ("a", "あいうえお", &["a", "i", "u", "e", "o"]),
    ("k", "かきくけこ", &["ka", "ki", "ku", "ke", "ko"]),
    ("s", "さしすせそ", &["sa", "shi", "su", "se", "so"]),
    ("t", "たちつてと", &["ta", "chi", "tsu", "te", "to"]),
    ("n", "なにぬねの", &["na", "ni", "nu", "ne", "no"]),
    ("h", "はひふへほ", &["ha", "hi", "fu", "he", "ho"]),
    ("m", "まみむめも", &["ma", "mi", "mu", "me", "mo"]),
    ("y", "やゆよ", &["ya", "yu", "yo"]),
    ("r", "らりるれろ", &["ra", "ri", "ru", "re", "ro"]),
    ("w", "わをん", &["wa", "wo", "n"]),
];
const DAKU_ROWS: [(&str, &str, &[&str]); 5] = [
    ("g", "がぎぐげご", &["ga", "gi", "gu", "ge", "go"]),
    ("z", "ざじずぜぞ", &["za", "ji", "zu", "ze", "zo"]),
    ("d", "だぢづでど", &["da", "ji", "zu", "de", "do"]),
    ("b", "ばびぶべぼ", &["ba", "bi", "bu", "be", "bo"]),
    ("p", "ぱぴぷぺぽ", &["pa", "pi", "pu", "pe", "po"]),
];
const YOON: [(&str, &str); 11] = [
    ("き", "k"),
    ("し", "sh"),
    ("ち", "ch"),
    ("に", "n"),
    ("ひ", "h"),
    ("み", "m"),
    ("り", "r"),
    ("ぎ", "g"),
    ("じ", "j"),
    ("び", "b"),
    ("ぴ", "p"),
];
const KATA_EXT: [(&str, &str); 18] = [
    ("ティ", "ti"),
    ("ディ", "di"),
    ("ファ", "fa"),
    ("フィ", "fi"),
    ("フェ", "fe"),
    ("フォ", "fo"),
    ("ウィ", "wi"),
    ("ウェ", "we"),
    ("ウォ", "wo"),
    ("ヴァ", "va"),
    ("ヴィ", "vi"),
    ("ヴ", "vu"),
    ("ジェ", "je"),
    ("シェ", "she"),
    ("チェ", "che"),
    ("トゥ", "tu"),
    ("ドゥ", "du"),
    ("デュ", "dyu"),
];

fn alt_of(r: &str) -> &'static [&'static str] {
    match r {
        "shi" => &["si"],
        "chi" => &["ti"],
        "tsu" => &["tu"],
        "fu" => &["hu"],
        "ji" => &["zi", "di"],
        "zu" => &["du"],
        "wo" => &["o"],
        "n" => &["nn"],
        "sha" => &["sya"],
        "shu" => &["syu"],
        "sho" => &["syo"],
        "cha" => &["tya", "cya"],
        "chu" => &["tyu", "cyu"],
        "cho" => &["tyo", "cyo"],
        "ja" => &["zya", "jya"],
        "ju" => &["zyu", "jyu"],
        "jo" => &["zyo", "jyo"],
        _ => &[],
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KanaItem {
    pub k: String,
    pub r: String,
    /// basic | daku | yoon | ext
    pub set: &'static str,
    pub row: String,
    pub alts: Vec<String>,
    /// 'h' hiragana, 'k' katakana
    pub script: char,
}

pub fn to_kata(s: &str) -> String {
    s.chars()
        .map(|c| if ('ぁ'..='ゖ').contains(&c) { char::from_u32(c as u32 + 0x60).unwrap_or(c) } else { c })
        .collect()
}

pub fn to_hira(s: &str) -> String {
    s.chars()
        .map(|c| if ('ァ'..='ヶ').contains(&c) { char::from_u32(c as u32 - 0x60).unwrap_or(c) } else { c })
        .collect()
}

fn build_kana() -> Vec<KanaItem> {
    let mut l: Vec<(String, String, &'static str, String)> = Vec::new();
    for (row, ks, rs) in KANA_ROWS {
        for (i, k) in ks.chars().enumerate() {
            l.push((k.to_string(), rs[i].to_string(), "basic", row.to_string()));
        }
    }
    for (row, ks, rs) in DAKU_ROWS {
        for (i, k) in ks.chars().enumerate() {
            l.push((k.to_string(), rs[i].to_string(), "daku", row.to_string()));
        }
    }
    for (base, c) in YOON {
        for (i, sm) in ["ゃ", "ゅ", "ょ"].iter().enumerate() {
            let v = ["a", "u", "o"][i];
            let r = if c == "sh" || c == "ch" || c == "j" { format!("{c}{v}") } else { format!("{c}y{v}") };
            l.push((format!("{base}{sm}"), r, "yoon", format!("y-{c}")));
        }
    }
    let mk = |k: String, r: String, set: &'static str, row: String, script: char| {
        let mut alts = vec![r.clone()];
        alts.extend(alt_of(&r).iter().map(|s| s.to_string()));
        KanaItem { k, r, set, row, alts, script }
    };
    let mut out: Vec<KanaItem> = l.iter().map(|(k, r, s, row)| mk(k.clone(), r.clone(), s, row.clone(), 'h')).collect();
    let kata: Vec<KanaItem> = l.iter().map(|(k, r, s, row)| mk(to_kata(k), r.clone(), s, row.clone(), 'k')).collect();
    out.extend(kata);
    for (k, r) in KATA_EXT {
        out.push(KanaItem {
            k: k.into(),
            r: r.into(),
            set: "ext",
            row: "ext".into(),
            alts: vec![r.into()],
            script: 'k',
        });
    }
    out
}

pub fn kana() -> &'static [KanaItem] {
    static K: OnceLock<Vec<KanaItem>> = OnceLock::new();
    K.get_or_init(build_kana)
}

pub fn kana_by(k: &str) -> Option<&'static KanaItem> {
    kana().iter().find(|x| x.k == k)
}

/// `kanaSet("h:basic,daku")`.
pub fn kana_set(spec: &str) -> Vec<&'static KanaItem> {
    let Some((sc, sets)) = spec.split_once(':') else { return vec![] };
    let script = if sc == "k" { 'k' } else { 'h' };
    let ss: Vec<&str> = sets.split(',').collect();
    kana().iter().filter(|x| x.script == script && ss.contains(&x.set)).collect()
}

fn roma() -> &'static HashMap<String, String> {
    static R: OnceLock<HashMap<String, String>> = OnceLock::new();
    R.get_or_init(|| {
        let mut m = HashMap::new();
        for x in kana() {
            m.entry(x.k.clone()).or_insert_with(|| x.r.clone());
        }
        for (k, r) in [
            ("ぁ", "a"),
            ("ぃ", "i"),
            ("ぅ", "u"),
            ("ぇ", "e"),
            ("ぉ", "o"),
            ("ァ", "a"),
            ("ィ", "i"),
            ("ゥ", "u"),
            ("ェ", "e"),
            ("ォ", "o"),
            ("ゎ", "wa"),
            ("ゐ", "i"),
            ("ゑ", "e"),
        ] {
            m.insert(k.into(), r.into());
        }
        m
    })
}

/// Kana → Hepburn romaji for hints (`romaji`).
pub fn romaji(s: &str) -> String {
    let m = roma();
    let c: Vec<char> = s.chars().collect();
    let get = |a: usize, n: usize| -> Option<&String> {
        if a + n > c.len() {
            return None;
        }
        m.get(&c[a..a + n].iter().collect::<String>())
    };
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        let one = c[i];
        if one == 'っ' || one == 'ッ' {
            let nx = get(i + 1, 2).or_else(|| get(i + 1, 1)).cloned().unwrap_or_default();
            if nx.starts_with("ch") {
                out.push('t');
            } else if let Some(ch) = nx.chars().next() {
                out.push(ch);
            }
            i += 1;
            continue;
        }
        if one == 'ー' {
            if let Some(l) = out.chars().last() {
                out.push(l);
            }
            i += 1;
            continue;
        }
        if let Some(r) = get(i, 2) {
            out.push_str(r);
            i += 2;
            continue;
        }
        if let Some(r) = get(i, 1) {
            let mut r = r.clone();
            if r == "n" {
                let next = get(i + 1, 1).cloned().unwrap_or_default();
                if next.starts_with(['a', 'i', 'u', 'e', 'o', 'y']) {
                    r = "n'".into();
                }
            }
            out.push_str(&r);
            i += 1;
            continue;
        }
        out.push(one);
        i += 1;
    }
    out
}

/// `romaMatch`.
pub fn roma_match(input: &str, item: &KanaItem) -> bool {
    let v = input.trim().to_lowercase();
    item.alts.iter().any(|a| *a == v)
}

/* ---------------- furigana ---------------- */

pub fn is_kanji(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '々' | '〆' | 'ヵ' | 'ヶ')
}

pub fn has_kanji(s: &str) -> bool {
    s.chars().any(is_kanji)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Text(String),
    Ruby(String, String),
}

/// Splits `食[た]べます` into text and ruby runs.
pub fn segments(w: &str) -> Vec<Seg> {
    let c: Vec<char> = w.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < c.len() {
        if is_kanji(c[i]) {
            let start = i;
            while i < c.len() && is_kanji(c[i]) {
                i += 1;
            }
            if i < c.len() && c[i] == '[' {
                if let Some(close) = c[i + 1..].iter().position(|&x| x == ']') {
                    if close > 0 {
                        if !text.is_empty() {
                            out.push(Seg::Text(std::mem::take(&mut text)));
                        }
                        out.push(Seg::Ruby(c[start..i].iter().collect(), c[i + 1..i + 1 + close].iter().collect()));
                        i += close + 2;
                        continue;
                    }
                }
            }
            text.extend(&c[start..i]);
            continue;
        }
        text.push(c[i]);
        i += 1;
    }
    if !text.is_empty() {
        out.push(Seg::Text(text));
    }
    out
}

/// Text without readings (`plainJ`).
pub fn plain_j(w: &str) -> String {
    segments(w)
        .into_iter()
        .map(|s| match s {
            Seg::Text(t) => t,
            Seg::Ruby(k, _) => k,
        })
        .collect()
}

/// Reading in kana (`kanaJ`).
pub fn kana_j(w: &str) -> String {
    segments(w)
        .into_iter()
        .map(|s| match s {
            Seg::Text(t) => t,
            Seg::Ruby(_, r) => r,
        })
        .collect()
}

/// "smart" furigana hides readings when every kanji is a mature card (≥ 7 days).
fn known_kanji(run: &str) -> bool {
    run.chars()
        .filter(|c| *c != '々')
        .all(|ch| crate::store::card(&format!("k:{ch}")).map(|c| c.state() == 2 && c.ivl() >= 7.0).unwrap_or(false))
}

/// HTML with `<ruby>` (`ruby(w, mode)`); mode: smart | always | never | "" (profile).
pub fn ruby(w: &str, mode: &str) -> String {
    let mode = if mode.is_empty() { crate::store::read(|s| s.p.furi.clone()) } else { mode.to_string() };
    if mode == "never" {
        return esc(&plain_j(w));
    }
    segments(w)
        .into_iter()
        .map(|s| match s {
            Seg::Text(t) => esc(&t),
            Seg::Ruby(k, r) => {
                if mode == "smart" && known_kanji(&k) {
                    esc(&k)
                } else {
                    format!("<ruby>{}<rt>{}</rt></ruby>", esc(&k), esc(&r))
                }
            }
        })
        .collect()
}

/* ---------------- sentences (chunk model) ---------------- */

/// role → (English name, Portuguese, question, marker)
pub const ROLES: [(&str, &str, &str, &str, &str); 15] = [
    ("TOPIC", "Topic", "tópico", "As for…?", "は"),
    ("SUBJ", "Subject", "sujeito", "Who / what?", "が"),
    ("OBJ", "Object", "objeto", "What (is acted on)?", "を"),
    ("TIME", "Time", "tempo", "When?", "に"),
    ("PLACE", "Place", "lugar", "Where?", "で・に"),
    ("GOAL", "Goal", "destino", "To where / whom?", "に・へ"),
    ("WITH", "With", "com", "With whom?", "と"),
    ("MEANS", "Means", "meio", "How / with what?", "で"),
    ("FROM", "From", "origem", "From where / whom?", "から"),
    ("UNTIL", "Until", "até", "Until when / where?", "まで"),
    ("MOD", "Modifier", "modificador", "Which / what kind?", "の・clause"),
    ("ADV", "Adverb", "advérbio", "How? How much? How often?", ""),
    ("QUOTE", "Quote", "citação", "What is said / thought?", "と"),
    ("LINK", "Link", "ligação", "Because / and then / when / if…", "て・から・ので"),
    ("PRED", "Predicate", "predicado", "What happens / what is it?", ""),
];

pub fn role(r: &str) -> (&'static str, &'static str, &'static str, &'static str, &'static str) {
    ROLES.iter().find(|x| x.0 == r).copied().unwrap_or(ROLES[14])
}

pub fn end_of(s: &Sentence) -> String {
    s.end.clone().filter(|e| !e.is_empty()).unwrap_or_else(|| "。".into())
}

pub fn sent_plain(s: &Sentence) -> String {
    s.c.iter().map(chunk_text).collect::<String>() + &end_of(s)
}

pub fn sent_kana(s: &Sentence) -> String {
    s.c.iter().map(|c| kana_j(&c.w) + c.p.as_deref().unwrap_or("")).collect::<String>() + &end_of(s)
}

pub fn chunk_text(c: &Chunk) -> String {
    plain_j(&c.w) + c.p.as_deref().unwrap_or("")
}

/// `sentHTML(S, {anat, tags, mode})`.
pub fn sent_html(s: &Sentence, anat: bool, tags: bool, mode: &str) -> String {
    let parts: String =
        s.c.iter()
            .map(|c| {
                let r = role(&c.r).0;
                format!(
                    r#"<span class="chunk role-{r}" {}>{}{}{}</span>"#,
                    if anat { "" } else { r#"style="--rc:transparent""# },
                    ruby(&c.w, mode),
                    c.p.as_deref().map(|p| format!(r#"<span class="pt">{}</span>"#, esc(p))).unwrap_or_default(),
                    if tags {
                        format!(r#"<span class="rtag">{}</span>"#, role(&c.r).1.to_uppercase())
                    } else {
                        String::new()
                    }
                )
            })
            .collect();
    format!(r#"<span class="sent-line{}">{}{}</span>"#, if anat { " anat" } else { "" }, parts, esc(&end_of(s)))
}

/// `roleLegend(roles)`.
pub fn role_legend(roles: &[&str]) -> String {
    let set: Vec<&str> = if roles.is_empty() { ROLES.iter().map(|r| r.0).collect() } else { roles.to_vec() };
    let items: String = set
        .iter()
        .map(|r| {
            let x = role(r);
            format!(
                r#"<span class="role-{}" title="{}">{} {}</span>"#,
                x.0,
                esc(x.3),
                x.1,
                if x.4.is_empty() { String::new() } else { format!("· {}", esc(x.4)) }
            )
        })
        .collect();
    format!(r#"<div class="legend">{items}</div>"#)
}

/// Speaker button HTML (`spk(text)`); clicks are handled globally by `data-say`.
pub fn spk(text: &str, cls: &str) -> String {
    format!(r#"<button class="{}" data-say="{}" title="Ouvir" aria-label="Ouvir">🔊</button>"#, cls, esc(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furigana_parsing() {
        assert_eq!(plain_j("食[た]べます"), "食べます");
        assert_eq!(kana_j("日本語[にほんご]を勉強[べんきょう]する"), "にほんごをべんきょうする");
        assert_eq!(segments("お茶[ちゃ]").len(), 2);
        assert_eq!(plain_j("[x]漢"), "[x]漢");
    }

    #[test]
    fn kana_and_romaji() {
        assert_eq!(kana().len(), (46 + 25 + 33) * 2 + 18);
        assert_eq!(romaji("きっぷ"), "kippu");
        assert_eq!(romaji("まっちゃ"), "matcha");
        assert_eq!(romaji("こんや"), "kon'ya");
        assert_eq!(romaji("ラーメン"), "raamen");
        assert_eq!(kana_set("h:basic").len(), 46);
        assert!(roma_match(" SI ", kana_by("し").unwrap()));
    }
}
