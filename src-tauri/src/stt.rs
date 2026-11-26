//! Local speech-to-text for the 🎤 Falar mode: the microphone is recorded
//! with cpal (WASAPI), resampled to 16 kHz and transcribed in Japanese by
//! Whisper (whisper.cpp through whisper-rs). Nothing leaves the PC. The
//! model (~190 MB) is downloaded on demand into the app data folder.

use crate::db::{err, R};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use sha2::Digest;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const MODEL_FILE: &str = "ggml-small-q5_1.bin";
/// Pinned to one revision of the repository, and checked byte by byte
/// (size and SHA-256) before whisper.cpp ever reads it.
const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small-q5_1.bin";
const MODEL_SIZE: u64 = 190_085_487;
const MODEL_MB: u64 = MODEL_SIZE / 1_000_000;
const MODEL_SHA256: &str = "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb";
/// Longest recording kept (seconds).
const MAX_SECS: u64 = 30;

type Rec = JoinHandle<Result<(Vec<f32>, u32), String>>;

#[derive(Default)]
pub struct Stt {
    rec: Mutex<Option<(Sender<()>, Rec)>>,
    ctx: Mutex<Option<Arc<WhisperContext>>>,
    downloading: AtomicBool,
}

fn model_path(app: &AppHandle) -> R<PathBuf> {
    Ok(app.path().app_data_dir().map_err(err)?.join("models").join(MODEL_FILE))
}

#[derive(Serialize)]
pub struct SttStatus {
    pub available: bool,
    pub model: bool,
    pub downloading: bool,
    pub size_mb: u64,
    pub message: String,
}

#[tauri::command]
pub fn stt_status(app: AppHandle, stt: State<'_, Stt>) -> SttStatus {
    let model = model_path(&app).map(|p| p.exists()).unwrap_or(false);
    let downloading = stt.downloading.load(Ordering::SeqCst);
    let message = if model {
        String::new()
    } else if downloading {
        "Baixando o modelo de voz…".into()
    } else {
        format!("Para o app ouvir sua voz, baixe uma vez o modelo de voz em japonês (~{MODEL_MB} MB). Até lá, o treino fica no modo sombra.")
    };
    SttStatus { available: model, model, downloading, size_mb: MODEL_MB, message }
}

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
    finished: bool,
    error: Option<String>,
}

/// Downloads the Whisper model, reporting `michi://stt-progress`.
#[tauri::command]
pub async fn stt_download(app: AppHandle) -> R<()> {
    let stt = app.state::<Stt>();
    if stt.downloading.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let app2 = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || download(&app2)).await.map_err(err)?;
    app.state::<Stt>().downloading.store(false, Ordering::SeqCst);
    let _ = app.emit(
        "michi://stt-progress",
        Progress { done: 0, total: 0, finished: res.is_ok(), error: res.as_ref().err().cloned() },
    );
    res
}

fn download(app: &AppHandle) -> R<()> {
    use std::io::{Read, Write};
    let path = model_path(app)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(err)?;
    let tmp = path.with_extension("part");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(30 * 60)))
        .build()
        .into();
    let resp = agent.get(MODEL_URL).call().map_err(|e| format!("não consegui baixar o modelo: {e}"))?;
    let total = MODEL_SIZE;
    let mut body = resp.into_body();
    let mut reader = body.with_config().limit(MODEL_SIZE + 1).reader();
    let mut hasher = sha2::Sha256::new();
    let mut file = std::fs::File::create(&tmp).map_err(err)?;
    let mut buf = vec![0u8; 256 * 1024];
    let (mut done, mut last) = (0u64, 0u64);
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrompido: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(err)?;
        sha2::Digest::update(&mut hasher, &buf[..n]);
        done += n as u64;
        if done - last > total / 100 {
            last = done;
            let _ = app.emit("michi://stt-progress", Progress { done, total, finished: false, error: None });
        }
    }
    file.flush().map_err(err)?;
    drop(file);
    let digest: String = sha2::Digest::finalize(hasher).iter().map(|b| format!("{b:02x}")).collect();
    if done != MODEL_SIZE || digest != MODEL_SHA256 {
        let _ = std::fs::remove_file(&tmp);
        return Err("o arquivo baixado não confere com o modelo esperado; tente de novo".into());
    }
    std::fs::rename(&tmp, &path).map_err(err)?;
    Ok(())
}

/// Starts recording the default microphone.
#[tauri::command]
pub fn stt_start(stt: State<'_, Stt>) -> R<()> {
    let mut rec = stt.rec.lock().map_err(err)?;
    if let Some((tx, h)) = rec.take() {
        let _ = tx.send(());
        let _ = h.join();
    }
    let (tx, rx) = channel::<()>();
    let (ready_tx, ready_rx) = channel::<Result<(), String>>();
    let handle: Rec = std::thread::spawn(move || {
        let host = cpal::default_host();
        let Some(dev) = host.default_input_device() else {
            let _ = ready_tx.send(Err("nenhum microfone encontrado".into()));
            return Err("sem microfone".into());
        };
        let cfg = match dev.default_input_config() {
            Ok(c) => c,
            Err(e) => {
                let _ = ready_tx.send(Err(format!("o microfone não abriu ({e}). Confira em Configurações → Privacidade → Microfone se apps da área de trabalho podem usar o microfone.")));
                return Err(e.to_string());
            }
        };
        let rate = cfg.sample_rate();
        let channels = cfg.channels() as usize;
        let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let max = rate as usize * MAX_SECS as usize;
        let push = {
            let buf = buf.clone();
            move |mono: &mut dyn Iterator<Item = f32>| {
                if let Ok(mut b) = buf.lock() {
                    for x in mono {
                        if b.len() < max {
                            b.push(x);
                        }
                    }
                }
            }
        };
        let errf = |e| log::warn!("microfone: {e}");
        let sc = cfg.config();
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => {
                let push = push.clone();
                dev.build_input_stream(
                    sc.clone(),
                    move |d: &[f32], _| push(&mut d.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32)),
                    errf,
                    None,
                )
            }
            cpal::SampleFormat::I16 => {
                let push = push.clone();
                dev.build_input_stream(
                    sc.clone(),
                    move |d: &[i16], _| {
                        push(
                            &mut d
                                .chunks(channels)
                                .map(|f| f.iter().map(|x| *x as f32 / 32768.0).sum::<f32>() / channels as f32),
                        )
                    },
                    errf,
                    None,
                )
            }
            cpal::SampleFormat::U16 => {
                let push = push.clone();
                dev.build_input_stream(
                    sc.clone(),
                    move |d: &[u16], _| {
                        push(
                            &mut d.chunks(channels).map(|f| {
                                f.iter().map(|x| (*x as f32 - 32768.0) / 32768.0).sum::<f32>() / channels as f32
                            }),
                        )
                    },
                    errf,
                    None,
                )
            }
            other => {
                let _ = ready_tx.send(Err(format!("formato de áudio não suportado ({other:?})")));
                return Err("formato".into());
            }
        };
        let stream =
            match stream.map_err(|e| e.to_string()).and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("o microfone não abriu ({e})")));
                    return Err(e);
                }
            };
        let _ = ready_tx.send(Ok(()));
        let _ = rx.recv_timeout(std::time::Duration::from_secs(MAX_SECS + 5));
        drop(stream);
        let samples = buf.lock().map(|b| b.clone()).unwrap_or_default();
        Ok((samples, rate))
    });
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => {
            *rec = Some((tx, handle));
            Ok(())
        }
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => Err("o microfone demorou para abrir".into()),
    }
}

/// Linear resampling to 16 kHz mono.
fn to_16k(x: &[f32], rate: u32) -> Vec<f32> {
    if rate == 16_000 || x.is_empty() {
        return x.to_vec();
    }
    let ratio = rate as f64 / 16_000.0;
    let n = (x.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let a = p.floor() as usize;
            let f = (p - a as f64) as f32;
            let b = (a + 1).min(x.len() - 1);
            x[a] * (1.0 - f) + x[b] * f
        })
        .collect()
}

/// Stops recording and returns the transcript.
#[tauri::command]
pub async fn stt_stop(app: AppHandle) -> R<String> {
    let rec = app.state::<Stt>().rec.lock().map_err(err)?.take();
    let Some((tx, handle)) = rec else { return Err("não estava gravando".into()) };
    let _ = tx.send(());
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (samples, rate) = handle.join().map_err(|_| "a gravação falhou".to_string())??;
        let audio = to_16k(&samples, rate);
        if audio.len() < 16_000 / 4 {
            return Ok(String::new());
        }
        let ctx = context(&app2)?;
        let mut state = ctx.create_state().map_err(err)?;
        let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        p.set_language(Some("ja"));
        p.set_translate(false);
        p.set_no_context(true);
        p.set_print_progress(false);
        p.set_print_realtime(false);
        p.set_print_special(false);
        p.set_print_timestamps(false);
        p.set_n_threads(std::thread::available_parallelism().map(|n| n.get().min(8) as i32).unwrap_or(4));
        state.full(p, &audio).map_err(err)?;
        let text: String = state.as_iter().filter_map(|s| s.to_str_lossy().ok().map(|t| t.to_string())).collect();
        Ok(text.trim().to_string())
    })
    .await
    .map_err(err)?
}

fn context(app: &AppHandle) -> R<Arc<WhisperContext>> {
    let stt = app.state::<Stt>();
    let mut g = stt.ctx.lock().map_err(err)?;
    if let Some(c) = g.as_ref() {
        return Ok(c.clone());
    }
    let path = model_path(app)?;
    if !path.exists() {
        return Err("o modelo de voz ainda não foi baixado".into());
    }
    let ctx = WhisperContext::new_with_params(&path, WhisperContextParameters::default())
        .map_err(|e| format!("não consegui abrir o modelo: {e}"))?;
    let ctx = Arc::new(ctx);
    *g = Some(ctx.clone());
    Ok(ctx)
}

/// Drops an unfinished recording (leaving the screen).
#[tauri::command]
pub fn stt_cancel(stt: State<'_, Stt>) {
    if let Ok(mut rec) = stt.rec.lock() {
        if let Some((tx, h)) = rec.take() {
            let _ = tx.send(());
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resamples_48k_to_16k() {
        let x: Vec<f32> = (0..48_000).map(|i| (i as f32 / 48.0).sin()).collect();
        let y = super::to_16k(&x, 48_000);
        assert_eq!(y.len(), 16_000);
        assert!((y[100] - x[300]).abs() < 1e-3);
    }
}
