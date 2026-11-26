//! Commands called by the UI (`window.__TAURI__.core.invoke`).

use crate::agenda;
use crate::aviso;
use crate::backup::{self, BackupInfo};
use crate::db::{self, Db, R};
use crate::events;
use crate::scheduler::Scheduler;
use crate::sync;
use michi_core::agenda::{Aviso, Kind, Occ};
use michi_core::ops::Op;
use michi_core::time::{now_ms, parse_key};
use michi_core::Card;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

fn wake(app: &AppHandle) {
    if let Some(s) = app.try_state::<Scheduler>() {
        s.wake();
    }
}

/* ---------------- documents ---------------- */

#[derive(Serialize)]
pub struct LoadAll {
    docs: BTreeMap<String, Value>,
    days: BTreeMap<String, Value>,
    cards: Vec<(String, Card)>,
}

/// Everything the UI keeps in memory. Waits (a few seconds at most) for the
/// first sync between PCs, so the window opens with the latest progress.
#[tauri::command]
pub async fn load_all(app: AppHandle) -> R<LoadAll> {
    if let Some(k) = app.try_state::<crate::keeper::Keeper>() {
        let ready = k.ready.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || ready.wait(crate::keeper::FIRST_SYNC_WAIT)).await;
    }
    let db = app.state::<Db>();
    let c = db.lock();
    Ok(LoadAll { docs: db::docs_all(&c)?, days: db::days_all(&c)?, cards: db::cards_all(&c)? })
}

fn poke_keeper(app: &AppHandle) {
    if let Some(k) = app.try_state::<crate::keeper::Keeper>() {
        k.poke();
    }
}

#[tauri::command]
pub fn doc_get(db: State<'_, Db>, name: String) -> Option<Value> {
    db::doc_get(&db.lock(), &name)
}

#[derive(Deserialize)]
pub struct DocIn {
    name: String,
    data: Value,
}

#[tauri::command]
pub fn save_docs(app: AppHandle, db: State<'_, Db>, items: Vec<DocIn>) -> R<()> {
    let mut profile = false;
    let mut desk = false;
    let mut closed = false;
    {
        let c = db.lock();
        for it in &items {
            if let Some(day) = it.name.strip_prefix("day:") {
                db::day_put(&c, day, &it.data)?;
                closed |= agenda::close_done_routine(&c, day)?;
            } else {
                db::doc_put(&c, &it.name, &it.data)?;
                profile |= it.name == "profile";
                desk |= it.name == "desk";
            }
        }
        if profile {
            sync::sync_events(&c)?;
        }
    }
    if closed {
        events::avisos_changed(&app);
    }
    if profile || desk {
        wake(&app);
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct CardIn {
    id: String,
    card: Option<Card>,
}

#[tauri::command]
pub fn save_cards(db: State<'_, Db>, items: Vec<CardIn>) -> R<()> {
    let mut c = db.lock();
    let tx = c.transaction().map_err(db::err)?;
    for it in &items {
        db::card_put(&tx, &it.id, it.card.as_ref())?;
    }
    tx.commit().map_err(db::err)
}

/* ---------------- backup ---------------- */

#[tauri::command]
pub async fn backup_export(app: AppHandle, db: State<'_, Db>) -> R<Option<String>> {
    let json = {
        let c = db.lock();
        backup::export(&c)?
    };
    let name = format!("nihongo-michi-backup-{}.json", michi_core::time::today_key());
    let Some(path) =
        app.dialog().file().add_filter("Backup do Michi", &["json"]).set_file_name(&name).blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(db::err)?;
    std::fs::write(&path, serde_json::to_string(&json).map_err(db::err)?).map_err(db::err)?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub async fn backup_pick(app: AppHandle) -> R<Option<BackupInfo>> {
    let Some(path) = app.dialog().file().add_filter("Backup do Michi", &["json"]).blocking_pick_file() else {
        return Ok(None);
    };
    let path = path.into_path().map_err(db::err)?;
    let (_, info) = backup::inspect(&path.display().to_string())?;
    Ok(Some(info))
}

/// Restores a backup. A safety copy of the current progress is saved first,
/// and with the sync on the restored progress replaces the other PCs' too.
#[tauri::command]
pub fn backup_apply(app: AppHandle, db: State<'_, Db>, path: String) -> R<()> {
    let (j, _) = backup::inspect(&path)?;
    {
        let mut c = db.lock();
        crate::autobackup::safety(&app, &c, "restaurar")?;
        backup::restore(&mut c, &j)?;
        crate::pcsync::start_new_epoch(&c)?;
    }
    wake(&app);
    poke_keeper(&app);
    Ok(())
}

#[tauri::command]
pub fn reset_progress(app: AppHandle, db: State<'_, Db>) -> R<()> {
    {
        let mut c = db.lock();
        crate::autobackup::safety(&app, &c, "zerar")?;
        backup::reset_progress(&mut c)?;
        crate::pcsync::start_new_epoch(&c)?;
    }
    wake(&app);
    poke_keeper(&app);
    Ok(())
}

/* ---------------- agenda ---------------- */

#[tauri::command]
pub fn agenda_range(db: State<'_, Db>, from: String, to: String) -> R<Vec<Occ>> {
    let (Some(a), Some(b)) = (parse_key(&from), parse_key(&to)) else { return Err("datas inválidas".into()) };
    let c = db.lock();
    agenda::occurrences(&c, a, b)
}

#[tauri::command]
pub fn occ_mark(app: AppHandle, db: State<'_, Db>, event_id: String, occ: String, done: bool) -> R<Vec<Op>> {
    let ops = {
        let c = db.lock();
        let o = agenda::find_occ(&c, &event_id, &occ)?.ok_or("ocorrência não encontrada")?;
        agenda::mark(&c, &o, done)?
    };
    events::patch(&app, &ops);
    events::avisos_changed(&app);
    crate::tray::refresh(&app);
    Ok(ops)
}

/* ---------------- reminders ---------------- */

#[derive(Serialize)]
pub struct AvisoState {
    active: Vec<Aviso>,
    missed: Vec<Aviso>,
    snooze_min: f64,
}

/// What the popup shows: active reminders and the unread missed ones.
#[tauri::command]
pub fn aviso_state(db: State<'_, Db>) -> R<AvisoState> {
    let c = db.lock();
    let missed: Vec<Aviso> = db::notif_list(&c, 200)?
        .into_iter()
        .filter(|a| a.status == "missed" && !a.read && a.fire_at > now_ms() - 48.0 * 3_600_000.0)
        .collect();
    Ok(AvisoState { active: db::notif_active(&c)?, missed, snooze_min: db::desk(&c).snooze_min.max(1.0) })
}

#[tauri::command]
pub fn aviso_fit(app: AppHandle, height: f64) {
    aviso::fit(&app, height);
}

#[tauri::command]
pub fn aviso_action(app: AppHandle, db: State<'_, Db>, id: i64, action: String, minutes: Option<f64>) -> R<()> {
    let mut ops: Vec<Op> = vec![];
    let mut nav: Option<(String, Option<String>)> = None;
    {
        let c = db.lock();
        let a = db::notif_get(&c, id).ok_or("aviso não encontrado")?;
        match action.as_str() {
            "done" => match agenda::find_occ(&c, &a.event_id, &a.occ)? {
                Some(o) => ops = agenda::mark(&c, &o, true)?,
                None => db::notif_set_status(&c, id, "done", None)?,
            },
            "snooze" => {
                let m = minutes.unwrap_or_else(|| db::desk(&c).snooze_min).clamp(1.0, 24.0 * 60.0);
                db::notif_set_status(&c, id, "snoozed", Some((now_ms() + m * 60_000.0) as i64))?;
            }
            "open" => {
                db::notif_set_status(&c, id, "opened", None)?;
                let block = a.event_id.strip_prefix("rotina-").map(str::to_string);
                nav = Some((a.go.clone().unwrap_or_else(|| "hoje".into()), block));
            }
            _ => db::notif_set_status(&c, id, "dismissed", None)?,
        }
    }
    events::patch(&app, &ops);
    if let Some((route, block)) = nav {
        events::navigate(&app, &route, block);
    }
    events::avisos_changed(&app);
    wake(&app);
    Ok(())
}

/// "OK" on the missed-reminders card.
#[tauri::command]
pub fn aviso_missed_ok(app: AppHandle, db: State<'_, Db>, open: bool) -> R<()> {
    {
        let c = db.lock();
        c.execute("UPDATE notifications SET read=1 WHERE status='missed'", []).map_err(db::err)?;
    }
    if open {
        events::navigate(&app, "avisos", None);
    }
    events::avisos_changed(&app);
    Ok(())
}

/// Fires a sample reminder right now (Ajustes and the tray).
pub fn fire_test(app: &AppHandle) {
    let db = app.state::<Db>();
    {
        let c = db.lock();
        let now = now_ms();
        let a = Aviso {
            id: 0,
            event_id: "teste".into(),
            occ: michi_core::time::today_key(),
            title: "Aviso de teste".into(),
            jp: "試験".into(),
            body: "É assim que o Michi te chama na hora da rotina.".into(),
            kind: Kind::Rotina,
            color: Kind::Rotina.color().into(),
            go: Some("hoje".into()),
            at: michi_core::time::hhmm(now),
            dur: 0,
            fire_at: now,
            status: "shown".into(),
            snooze_until: None,
            read: false,
        };
        let _ = db::notif_insert(&c, &format!("teste|{now}"), &a);
    }
    aviso::show(app);
    let desk = db::desk(&db.lock());
    if desk.sound {
        crate::sound::chime(desk.volume);
    }
    events::avisos_changed(app);
}

#[tauri::command]
pub fn aviso_test(app: AppHandle) {
    fire_test(&app);
}

#[tauri::command]
pub fn sound_test(volume: f64) {
    crate::sound::chime(volume);
}

#[tauri::command]
pub fn avisos_list(db: State<'_, Db>, limit: Option<i64>) -> R<Vec<Aviso>> {
    let c = db.lock();
    db::notif_list(&c, limit.unwrap_or(300))
}

#[tauri::command]
pub fn avisos_unread(db: State<'_, Db>) -> i64 {
    db::notif_unread(&db.lock())
}

#[tauri::command]
pub fn avisos_read_all(app: AppHandle, db: State<'_, Db>) -> R<()> {
    db::notif_mark_all_read(&db.lock())?;
    events::avisos_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn avisos_clear(app: AppHandle, db: State<'_, Db>) -> R<()> {
    db::notif_clear(&db.lock())?;
    events::avisos_changed(&app);
    Ok(())
}

/* ---------------- app ---------------- */

#[tauri::command]
pub fn autostart_get(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub fn autostart_set(app: AppHandle, on: bool) -> R<bool> {
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }.map_err(db::err)?;
    Ok(al.is_enabled().unwrap_or(false))
}

#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> R<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("só links http(s)".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(db::err)
}

#[derive(Serialize)]
pub struct AppInfo {
    version: String,
    data_dir: String,
}

#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: app.path().app_data_dir().map(|p| p.display().to_string()).unwrap_or_default(),
    }
}

#[tauri::command]
pub fn reveal_data_dir(app: AppHandle) -> R<()> {
    let dir = app.path().app_data_dir().map_err(db::err)?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(db::err)
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
