//! Speech-to-text for the 🎤 Falar mode. The UI asks for the status, starts
//! recording, and gets the transcript when it stops. Until the local engine
//! is installed the status says so and the UI stays in "sombra" mode.

use serde::Serialize;

#[derive(Serialize)]
pub struct SttStatus {
    pub available: bool,
    pub message: String,
}

#[tauri::command]
pub fn stt_status() -> SttStatus {
    SttStatus {
        available: false,
        message: "O reconhecimento de voz local (Whisper) ainda não está instalado neste app, então o treino fica no modo sombra.".into(),
    }
}

#[tauri::command]
pub fn stt_start() -> Result<(), String> {
    Err("reconhecimento de voz indisponível".into())
}

#[tauri::command]
pub fn stt_stop() -> Result<String, String> {
    Err("reconhecimento de voz indisponível".into())
}

#[tauri::command]
pub fn stt_cancel() {}
