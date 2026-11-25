//! UI-side documents of the artifact that the backend never reads:
//! `journey`, `kana`, `mined`. Unknown fields are preserved.

use michi_core::de;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PointState {
    /// When it was marked as studied (ms), 0 = not studied.
    #[serde(deserialize_with = "de::f64_any", skip_serializing_if = "is_zero")]
    pub studied: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Journey {
    /// station → 1 (passed when positioning) or the time it was cleared.
    #[serde(deserialize_with = "de::f64_map")]
    pub done: BTreeMap<String, f64>,
    /// station → best boss score (%).
    #[serde(deserialize_with = "de::f64_map")]
    pub best: BTreeMap<String, f64>,
    pub points: BTreeMap<String, PointState>,
    /// station → when the stamp was earned.
    #[serde(deserialize_with = "de::f64_map")]
    pub stamps: BTreeMap<String, f64>,
    /// sentence id → spoken with 🎤 (count or time).
    #[serde(deserialize_with = "de::f64_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub spoke: BTreeMap<String, f64>,
    /// sign word → times read correctly.
    #[serde(deserialize_with = "de::f64_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub signs: BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Journey {
    pub fn studied(&self, pid: &str) -> bool {
        self.points.get(pid).map(|x| x.studied > 0.0).unwrap_or(false)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct KanaDoc {
    /// kana → [attempts, correct, average ms]
    pub s: BTreeMap<String, Vec<f64>>,
    /// game → best score
    #[serde(deserialize_with = "de::f64_map", skip_serializing_if = "BTreeMap::is_empty")]
    pub best: BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl KanaDoc {
    /// `kanaStrength(k)`.
    pub fn strength(&self, k: &str) -> f64 {
        match self.s.get(k) {
            Some(s) if s.len() >= 2 && s[0] >= 2.0 => s[1] / s[0],
            _ => 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct MinedItem {
    #[serde(deserialize_with = "de::string_any")]
    pub id: String,
    #[serde(deserialize_with = "de::string_any")]
    pub front: String,
    #[serde(deserialize_with = "de::string_any")]
    pub reading: String,
    #[serde(deserialize_with = "de::string_any")]
    pub meaning: String,
    #[serde(deserialize_with = "de::string_any")]
    pub sentence: String,
    #[serde(deserialize_with = "de::string_any")]
    pub source: String,
    #[serde(deserialize_with = "de::string_any", skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(deserialize_with = "de::f64_any")]
    pub added: f64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Mined {
    #[serde(deserialize_with = "de::vec_lossy")]
    pub list: Vec<MinedItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
