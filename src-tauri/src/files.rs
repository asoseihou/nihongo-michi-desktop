//! Files and the Anki bridge: save/open text through the system dialogs
//! (exports, imports) and AnkiConnect on 127.0.0.1:8765.

use crate::db::{err, R};
use serde_json::{json, Value};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

/// Saves text where the learner chooses; returns the path (None = cancelled).
#[tauri::command]
pub async fn save_text(app: AppHandle, name: String, content: String, ext: Option<String>) -> R<Option<String>> {
    let ext = ext.unwrap_or_else(|| name.rsplit('.').next().unwrap_or("txt").to_string());
    let Some(path) =
        app.dialog().file().add_filter(ext.to_uppercase(), &[ext.as_str()]).set_file_name(&name).blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(err)?;
    std::fs::write(&path, content.as_bytes()).map_err(err)?;
    Ok(Some(path.display().to_string()))
}

/// Opens a text file chosen by the learner (UTF-8, BOM removed).
#[tauri::command]
pub async fn open_text(app: AppHandle, exts: Vec<String>) -> R<Option<String>> {
    let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
    let Some(path) = app.dialog().file().add_filter("Texto", &refs).blocking_pick_file() else { return Ok(None) };
    let path = path.into_path().map_err(err)?;
    let bytes = std::fs::read(&path).map_err(err)?;
    Ok(Some(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string()))
}

/// AnkiConnect actions the Anki screen uses. Calls from here skip
/// AnkiConnect's browser-origin check, so nothing else goes through (no file
/// media, no exports, no URLs).
const ANKI_ACTIONS: [&str; 6] = ["version", "deckNames", "createDeck", "addNotes", "findCards", "cardsInfo"];

/// One AnkiConnect call (`{action, version: 6, params}`).
#[tauri::command]
pub async fn anki_connect(action: String, params: Option<Value>) -> R<Value> {
    if !ANKI_ACTIONS.contains(&action.as_str()) {
        return Err(format!("ação do AnkiConnect não permitida: {action}"));
    }
    let body = json!({"action": action, "version": 6, "params": params.unwrap_or_else(|| json!({}))});
    let agent: ureq::Agent =
        ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(12))).build().into();
    let resp = agent
        .post("http://127.0.0.1:8765")
        .send_json(&body)
        .map_err(|_| "O Anki não respondeu. Ele está aberto com o AnkiConnect instalado?".to_string())?;
    let j: Value = resp.into_body().read_json().map_err(|_| "Resposta inválida do AnkiConnect.".to_string())?;
    if let Some(e) = j.get("error").filter(|e| !e.is_null()) {
        return Err(e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string()));
    }
    Ok(j.get("result").cloned().unwrap_or(Value::Null))
}
