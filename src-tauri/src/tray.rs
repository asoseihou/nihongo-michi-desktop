//! Tray icon: the app keeps running here when the window is closed, so
//! the scheduler can remind on time.

use crate::agenda::occurrences;
use crate::db::{self, Db};
use crate::events;
use chrono::Duration;
use michi_core::time::{hhmm, local, ms_of, now_ms};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub struct TrayItems {
    next: MenuItem<Wry>,
    pause: MenuItem<Wry>,
}

pub struct TrayState(Mutex<Option<TrayItems>>);

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Abrir Nihongo Michi", true, None::<&str>)?;
    let agenda = MenuItem::with_id(app, "agenda", "📅 Agenda", true, None::<&str>)?;
    let avisos = MenuItem::with_id(app, "avisos", "🔔 Central de avisos", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "Próximo aviso: —", false, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pausar avisos por 1 h", true, None::<&str>)?;
    let test = MenuItem::with_id(app, "test", "Testar aviso", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &agenda,
            &avisos,
            &PredefinedMenuItem::separator(app)?,
            &next,
            &pause,
            &test,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.manage(TrayState(Mutex::new(Some(TrayItems { next, pause }))));
    TrayIconBuilder::with_id("michi")
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("Nihongo Michi")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "open" => events::show_main(app),
            "agenda" => events::navigate(app, "agenda", None),
            "avisos" => events::navigate(app, "avisos", None),
            "pause" => toggle_pause(app),
            "test" => crate::commands::fire_test(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                events::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    refresh(app);
    Ok(())
}

fn toggle_pause(app: &AppHandle) {
    let db = app.state::<Db>();
    {
        let c = db.lock();
        let mut desk = db::desk(&c);
        let now = now_ms();
        desk.pause_until = if desk.pause_until > now { 0.0 } else { now + 3_600_000.0 };
        desk.u = Some(now);
        if let Ok(v) = serde_json::to_value(&desk) {
            let _ = db::doc_put(&c, "desk", &v);
        }
    }
    let _ = tauri::Emitter::emit_to(app, "main", events::RELOAD_DESK, ());
    refresh(app);
}

/// Next reminder text and pause label. Must be called without the DB lock held.
pub fn refresh(app: &AppHandle) {
    let Some(state) = app.try_state::<TrayState>() else { return };
    let db = app.state::<Db>();
    let now = now_ms();
    let (next, paused) = {
        let c = db.lock();
        let desk = db::desk(&c);
        let today = local(now).date();
        let mut best: Option<(f64, String)> = None;
        if let Ok(occs) = occurrences(&c, today, today + Duration::days(2)) {
            for o in occs.iter().filter(|o| !o.done) {
                for &b in &o.reminders {
                    let t = ms_of(crate::scheduler::fire_time(o, b));
                    if t > now && best.as_ref().map(|(x, _)| t < *x).unwrap_or(true) {
                        let label = if o.all_day { o.title.clone() } else { format!("{} {}", o.time, o.title) };
                        best = Some((t, label));
                    }
                }
            }
        }
        let text = match best {
            Some((t, label)) => {
                let day =
                    if local(t).date() == today { String::new() } else { format!("{} ", local(t).format("%d/%m")) };
                format!("Próximo aviso: {day}{} · {label}", hhmm(t))
            }
            None => "Próximo aviso: nenhum nas próximas 48 h".into(),
        };
        (text, desk.pause_until > now)
    };
    if let Ok(g) = state.0.lock() {
        if let Some(items) = g.as_ref() {
            let _ = items.next.set_text(truncate(&next, 70));
            let _ = items.pause.set_text(if paused { "Retomar avisos" } else { "Pausar avisos por 1 h" });
        }
    };
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}
