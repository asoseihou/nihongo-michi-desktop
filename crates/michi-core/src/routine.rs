//! Daily routine blocks (port of `ROUTINE_DEFAULT`, `daysOf`, `daysLabel`
//! from `10-today.js`).

use crate::de;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Block {
    #[serde(deserialize_with = "de::string_any")]
    pub id: String,
    /// "HH:MM"
    #[serde(deserialize_with = "de::string_any")]
    pub time: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub min: f64,
    #[serde(deserialize_with = "de::string_any")]
    pub jp: String,
    #[serde(deserialize_with = "de::string_any")]
    pub name: String,
    #[serde(deserialize_with = "de::string_any")]
    pub what: String,
    /// Route opened by the block (hoje, revisar, jogos…).
    #[serde(deserialize_with = "de::string_any")]
    pub go: String,
    /// "Essencial": kept on the minimal day.
    #[serde(deserialize_with = "de::bool_any", skip_serializing_if = "de::is_false")]
    pub core: bool,
    /// 0 = Sunday … 6 = Saturday; missing = every day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days: Option<Vec<u8>>,
    #[serde(deserialize_with = "de::bool_any", skip_serializing_if = "de::is_false")]
    pub off: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Block {
    fn default() -> Self {
        Block {
            id: String::new(),
            time: "19:00".into(),
            min: 15.0,
            jp: "＋".into(),
            name: "Novo bloco".into(),
            what: String::new(),
            go: "hoje".into(),
            core: false,
            days: None,
            off: false,
            extra: Map::new(),
        }
    }
}

pub const ALLDAYS: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];
pub const WEEK: [&str; 7] = ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"];
pub const WEEK1: [&str; 7] = ["D", "S", "T", "Q", "Q", "S", "S"];
/// Monday-first display order.
pub const WEEK_ORDER: [u8; 7] = [1, 2, 3, 4, 5, 6, 0];

fn b(id: &str, time: &str, min: f64, jp: &str, name: &str, what: &str, go: &str, core: bool) -> Block {
    Block {
        id: id.into(),
        time: time.into(),
        min,
        jp: jp.into(),
        name: name.into(),
        what: what.into(),
        go: go.into(),
        core,
        ..Block::default()
    }
}

pub fn routine_default() -> Vec<Block> {
    vec![
        b(
            "srs",
            "07:30",
            15.0,
            "復習",
            "Revisões (SRS)",
            "Kanji 2k e gramática aqui; depois seu Anki (Kaishi)",
            "revisar",
            true,
        ),
        b("jogo", "12:30", 10.0, "遊び", "Minigame rápido", "Kana, kanji ou partículas por 5–10 min", "jogos", false),
        b("estacao", "20:00", 25.0, "学習", "Estação do dia", "Gramática da estação + vídeo + Genki", "jornada", true),
        b("wagotabi", "20:30", 15.0, "旅", "Wagotabi", "Jogue e registre onde parou", "imersao", false),
        b("frases", "20:50", 10.0, "文", "Frases", "Monte e leia frases da estação", "frases", false),
        b(
            "vlog",
            "21:30",
            15.0,
            "浸",
            "Imersão: vlog",
            "Um vlog em japonês, sem legenda em português",
            "imersao",
            true,
        ),
    ]
}

impl Block {
    /// `daysOf(b)`.
    pub fn days_of(&self) -> Vec<u8> {
        match &self.days {
            Some(d) if !d.is_empty() => d.clone(),
            _ => ALLDAYS.to_vec(),
        }
    }
    pub fn on_day(&self, dow: u8) -> bool {
        self.days_of().contains(&dow)
    }
}

/// `daysLabel(days)`.
pub fn days_label(days: &[u8]) -> String {
    let has = |d: u8| days.contains(&d);
    if days.len() == 7 {
        return "todo dia".into();
    }
    if days.len() == 5 && [1, 2, 3, 4, 5].iter().all(|d| has(*d)) {
        return "seg a sex".into();
    }
    if days.len() == 2 && has(0) && has(6) {
        return "fim de semana".into();
    }
    WEEK_ORDER.iter().filter(|d| has(**d)).map(|d| WEEK[*d as usize]).collect::<Vec<_>>().join(", ")
}

/// The routine stored in the profile, or the default one (`routine()`).
pub fn routine_of(stored: &Option<Vec<Block>>) -> Vec<Block> {
    match stored {
        Some(r) if !r.is_empty() => r
            .iter()
            .take(40)
            .cloned()
            .map(|mut b| {
                // Blocks also arrive from backups and other PCs.
                b.min = if b.min.is_finite() { b.min.clamp(0.0, 600.0) } else { 0.0 };
                b
            })
            .collect(),
        _ => routine_default(),
    }
}

/// `activeBlocks()` for a given weekday.
pub fn active_blocks(stored: &Option<Vec<Block>>, dow: u8, minimal: bool) -> Vec<Block> {
    let r: Vec<Block> = routine_of(stored).into_iter().filter(|b| !b.off && b.on_day(dow)).collect();
    if minimal {
        r.into_iter()
            .filter(|b| b.core)
            .map(|mut b| {
                b.min = b.min.min(10.0);
                b
            })
            .collect()
    } else {
        r
    }
}

/// Category of active time for a route (`CAT` in `04-app.js`).
pub fn cat_of(route: &str) -> &'static str {
    match route {
        "jornada" | "sensei" => "estudo",
        "revisar" => "srs",
        "jogos" => "jogos",
        "frases" => "frases",
        "imersao" => "imersao",
        _ => "geral",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(days_label(&ALLDAYS), "todo dia");
        assert_eq!(days_label(&[1, 2, 3, 4, 5]), "seg a sex");
        assert_eq!(days_label(&[0, 6]), "fim de semana");
        assert_eq!(days_label(&[0, 2, 4]), "Ter, Qui, Dom");
    }

    #[test]
    fn minimal_day_keeps_core_blocks() {
        let a = active_blocks(&None, 3, true);
        assert_eq!(a.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["srs", "estacao", "vlog"]);
        assert!(a.iter().all(|b| b.min <= 10.0));
    }

    #[test]
    fn parses_artifact_block() {
        let j = r#"{"id":"c_x","time":"19:00","min":"20","jp":"＋","name":"Leitura","what":null,"go":"hoje","core":0,"days":[1,3]}"#;
        let b: Block = serde_json::from_str(j).unwrap();
        assert_eq!(b.min, 20.0);
        assert_eq!(b.what, "");
        assert_eq!(b.days_of(), vec![1, 3]);
    }
}
