//! Lenient deserializers. The artifact stored plain JavaScript objects, so a
//! backup can hold `null`, strings or floats where Rust expects something
//! stricter. Importing must never fail because of that.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

fn num_of(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Any JSON value → f64 (null / garbage → 0).
pub fn f64_any<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(num_of(&v).filter(|x| x.is_finite()).unwrap_or(0.0))
}

/// Any JSON value → truthiness, like `!!x` in JavaScript.
pub fn bool_any<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(truthy(&v))
}

pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0 && !x.is_nan()).unwrap_or(false),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// Any JSON value → String (null → "", numbers → their text).
pub fn string_any<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::String(s) => s,
        Value::Null => String::new(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        other => other.to_string(),
    })
}

/// `{key: number-ish}` → map of f64, skipping entries that are not numbers.
pub fn f64_map<'de, D: Deserializer<'de>>(d: D) -> Result<std::collections::BTreeMap<String, f64>, D::Error> {
    let v = Value::deserialize(d)?;
    let mut out = std::collections::BTreeMap::new();
    if let Value::Object(m) = v {
        for (k, x) in m {
            if let Some(n) = num_of(&x) {
                out.insert(k, n);
            }
        }
    }
    Ok(out)
}

/// `{key: truthy}` → map of bool.
pub fn bool_map<'de, D: Deserializer<'de>>(d: D) -> Result<std::collections::BTreeMap<String, bool>, D::Error> {
    let v = Value::deserialize(d)?;
    let mut out = std::collections::BTreeMap::new();
    if let Value::Object(m) = v {
        for (k, x) in m {
            out.insert(k, truthy(&x));
        }
    }
    Ok(out)
}

/// Array of objects; entries that fail to parse are dropped instead of
/// failing the whole document.
pub fn vec_lossy<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::Array(a) => a.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect(),
        _ => Vec::new(),
    })
}

/// `null` or a lossy array.
pub fn opt_vec_lossy<'de, D, T>(d: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::Array(a) => Some(a.into_iter().filter_map(|x| serde_json::from_value(x).ok()).collect()),
        _ => None,
    })
}

pub fn is_false(b: &bool) -> bool {
    !*b
}
