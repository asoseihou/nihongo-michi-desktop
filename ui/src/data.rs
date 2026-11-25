//! Teaching data bundled with the app (`ui/data/*.json`, embedded in the
//! executable by Tauri): KANJIDIC2 kanji (CC BY-SA 4.0), KanjiVG strokes
//! (CC BY-SA 3.0), JLPT vocabulary and the Michi content. Loaded lazily,
//! like `need()` in `03-srs.js`.

use serde::Deserialize;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Chunk {
    pub w: String,
    pub p: Option<String>,
    pub r: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Sentence {
    pub en: String,
    #[serde(deserialize_with = "null_vec")]
    pub c: Vec<Chunk>,
    #[serde(deserialize_with = "null_vec")]
    pub alt: Vec<Vec<usize>>,
    pub end: Option<String>,
    /// Filled after loading: point id + "#" + index.
    #[serde(skip)]
    pub id: String,
    #[serde(skip)]
    pub pid: String,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Point {
    pub id: String,
    pub title: String,
    pub pattern: String,
    pub gist: String,
    pub explain: String,
    pub tip: String,
    pub yt: String,
    pub s: Vec<Sentence>,
    #[serde(skip)]
    pub lesson: String,
}

fn null_vec<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Lesson {
    pub id: String,
    pub jlpt: u8,
    pub genki: String,
    pub title: String,
    pub title_jp: String,
    #[serde(deserialize_with = "null_vec")]
    pub vocab: Vec<(String, String)>,
    #[serde(deserialize_with = "null_vec")]
    pub points: Vec<Point>,
    #[serde(deserialize_with = "null_vec")]
    pub signs: Vec<(String, String)>,
    pub roleplay: Option<Value>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Station {
    pub id: String,
    pub line: String,
    pub name: String,
    pub jp: String,
    pub kana: Option<String>,
    #[serde(deserialize_with = "null_vec")]
    pub lessons: Vec<String>,
    /// Inclusive index range into the kanji list.
    pub kanji: Option<(usize, usize)>,
    #[serde(deserialize_with = "null_vec")]
    pub yt: Vec<String>,
    pub genki: Option<String>,
    pub city: Option<String>,
    pub days: Option<String>,
    pub talk: bool,
    #[serde(deserialize_with = "null_vec")]
    pub base: Vec<String>,
    #[serde(deserialize_with = "null_vec")]
    pub signs: Vec<(String, String)>,
    pub roleplay: Option<Value>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Area {
    pub id: String,
    pub pref: String,
    pub name: String,
    pub jp: String,
    pub verified: bool,
    pub topics: Vec<String>,
    pub lessons: Vec<String>,
    pub drills: Vec<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Wagotabi {
    pub note: Value,
    pub sources: Value,
    pub areas: Vec<Area>,
    pub anywhere: Value,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Resource {
    pub name: String,
    pub url: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub lang: String,
    pub level: String,
    pub best_for: String,
    pub searches: Vec<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Resources {
    pub checked: Value,
    pub items: Vec<Resource>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Content {
    pub lessons: Vec<Lesson>,
    pub readings: Vec<Lesson>,
    pub stations: Vec<Station>,
    pub wagotabi: Wagotabi,
    pub resources: Resources,
    pub verbs: Vec<Value>,
    /// lesson id → index in `lessons`
    #[serde(skip)]
    pub lesson_idx: HashMap<String, usize>,
    /// point id → (lesson index, point index)
    #[serde(skip)]
    pub point_idx: HashMap<String, (usize, usize)>,
}

impl Content {
    fn index(&mut self) {
        for (li, l) in self.lessons.iter_mut().enumerate() {
            self.lesson_idx.insert(l.id.clone(), li);
            for (pi, p) in l.points.iter_mut().enumerate() {
                p.lesson = l.id.clone();
                self.point_idx.insert(p.id.clone(), (li, pi));
                for (si, s) in p.s.iter_mut().enumerate() {
                    s.id = format!("{}#{}", p.id, si);
                    s.pid = p.id.clone();
                }
            }
        }
    }
    pub fn lesson(&self, id: &str) -> Option<&Lesson> {
        self.lesson_idx.get(id).map(|i| &self.lessons[*i])
    }
    pub fn point(&self, id: &str) -> Option<&Point> {
        self.point_idx.get(id).map(|(l, p)| &self.lessons[*l].points[*p])
    }
    pub fn points(&self) -> impl Iterator<Item = &Point> {
        self.lessons.iter().flat_map(|l| l.points.iter())
    }
    pub fn station(&self, id: &str) -> Option<&Station> {
        self.stations.iter().find(|s| s.id == id)
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Kanji {
    pub k: String,
    /// strokes
    pub s: Option<u32>,
    /// school grade
    pub g: Option<u32>,
    /// frequency rank
    pub f: Option<u32>,
    /// JLPT level
    pub n: Option<u32>,
    pub on: Vec<String>,
    pub kun: Vec<String>,
    pub en: Vec<String>,
    pub pt: Vec<String>,
    pub rad: String,
    /// examples: word, reading, meaning, JLPT
    pub ex: Vec<(String, String, String, Option<u32>)>,
    pub parts: Vec<String>,
}

/// word, reading, meaning, JLPT level
pub type Vocab = (String, String, String, u32);

thread_local! {
    static CONTENT: RefCell<Option<Rc<Content>>> = const { RefCell::new(None) };
    static KANJI: RefCell<Option<Rc<Vec<Kanji>>>> = const { RefCell::new(None) };
    static KIDX: RefCell<Option<Rc<HashMap<String, usize>>>> = const { RefCell::new(None) };
    static VOCAB: RefCell<Option<Rc<Vec<Vocab>>>> = const { RefCell::new(None) };
    static STROKES: RefCell<Option<Rc<HashMap<String, Vec<String>>>>> = const { RefCell::new(None) };
}

async fn fetch_text(path: &str) -> Result<String, String> {
    let w = crate::util::window();
    let resp = JsFuture::from(w.fetch_with_str(path)).await.map_err(|_| format!("{path}: falha ao ler"))?;
    let resp: web_sys::Response = resp.dyn_into().map_err(|_| "resposta inválida".to_string())?;
    if !resp.ok() {
        return Err(format!("{path} → {}", resp.status()));
    }
    let t = JsFuture::from(resp.text().map_err(|_| "texto".to_string())?).await.map_err(|_| "texto".to_string())?;
    t.as_string().ok_or_else(|| "texto".into())
}

pub fn content() -> Option<Rc<Content>> {
    CONTENT.with(|c| c.borrow().clone())
}
pub fn kanji() -> Option<Rc<Vec<Kanji>>> {
    KANJI.with(|c| c.borrow().clone())
}
pub fn kanji_index() -> Option<Rc<HashMap<String, usize>>> {
    KIDX.with(|c| c.borrow().clone())
}
pub fn vocab() -> Option<Rc<Vec<Vocab>>> {
    VOCAB.with(|c| c.borrow().clone())
}
pub fn strokes() -> Option<Rc<HashMap<String, Vec<String>>>> {
    STROKES.with(|c| c.borrow().clone())
}

pub async fn need_content() -> Result<Rc<Content>, String> {
    if let Some(c) = content() {
        return Ok(c);
    }
    let t = fetch_text("data/content.json").await?;
    let mut c: Content = serde_json::from_str(&t).map_err(|e| format!("content.json: {e}"))?;
    c.index();
    let rc = Rc::new(c);
    CONTENT.with(|x| *x.borrow_mut() = Some(rc.clone()));
    Ok(rc)
}

pub async fn need_kanji() -> Result<Rc<Vec<Kanji>>, String> {
    if let Some(k) = kanji() {
        return Ok(k);
    }
    let t = fetch_text("data/kanji.json").await?;
    let v: Vec<Kanji> = serde_json::from_str(&t).map_err(|e| format!("kanji.json: {e}"))?;
    let idx: HashMap<String, usize> = v.iter().enumerate().map(|(i, k)| (k.k.clone(), i)).collect();
    let rc = Rc::new(v);
    KANJI.with(|x| *x.borrow_mut() = Some(rc.clone()));
    KIDX.with(|x| *x.borrow_mut() = Some(Rc::new(idx)));
    Ok(rc)
}

pub async fn need_vocab() -> Result<Rc<Vec<Vocab>>, String> {
    if let Some(v) = vocab() {
        return Ok(v);
    }
    let t = fetch_text("data/vocab.json").await?;
    let v: Vec<Vocab> = serde_json::from_str(&t).map_err(|e| format!("vocab.json: {e}"))?;
    let rc = Rc::new(v);
    VOCAB.with(|x| *x.borrow_mut() = Some(rc.clone()));
    Ok(rc)
}

pub async fn need_strokes() -> Result<Rc<HashMap<String, Vec<String>>>, String> {
    if let Some(s) = strokes() {
        return Ok(s);
    }
    let t = fetch_text("data/strokes.json").await?;
    let v: HashMap<String, Vec<String>> = serde_json::from_str(&t).map_err(|e| format!("strokes.json: {e}"))?;
    let rc = Rc::new(v);
    STROKES.with(|x| *x.borrow_mut() = Some(rc.clone()));
    Ok(rc)
}
