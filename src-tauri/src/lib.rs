//! Achieve: a proactive day-companion. This library wires the pieces together:
//! the SQLite ledger, the engine loop, automatic focus capture, the shutdown
//! guard, the system tray, and the single adaptive glass window.

mod autostart;
mod commands;
mod db;
mod desktop;
mod engine;
mod idle;
mod model;
mod niri;
mod shutdown;
mod window;

use std::sync::{Arc, Mutex};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager,
};

/// Shared application state. rusqlite's Connection is Send but not Sync, so we
/// guard it with a Mutex and share clones with the background threads.
pub struct AppState {
    pub db: Arc<Mutex<rusqlite::Connection>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            desktop::ensure_niri_float_rule();
            autostart::ensure_autostart();

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("achieve.db");
            let conn = db::open(&db_path).map_err(|e| format!("failed to open database: {e}"))?;
            let db = Arc::new(Mutex::new(conn));
            app.manage(AppState { db: db.clone() });

            // Idle detection: how long with no input before a task auto-pauses.
            let idle_secs: i64 = std::env::var("ACHIEVE_IDLE_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|&s| s >= 30)
                .unwrap_or(600);
            let idle_flag = idle::idle_flag_path(&data_dir);

            niri::spawn_focus_tracker(db.clone());
            idle::spawn_idle_watcher(idle_flag.clone(), idle_secs as u32);
            shutdown::spawn_shutdown_guard(db.clone());
            shutdown::spawn_sleep_guard(db.clone());
            engine::spawn(app.handle().clone(), db.clone(), idle_flag, idle_secs);

            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::list_tasks,
            commands::list_upcoming,
            commands::list_categories,
            commands::create_category,
            commands::delete_category,
            commands::create_task,
            commands::update_task,
            commands::delete_task,
            commands::start_task,
            commands::pause_task,
            commands::extend_active,
            commands::complete_task,
            commands::reopen_task,
            commands::reschedule_task,
            commands::set_plan_date,
            commands::get_day_plan,
            commands::save_day_plan,
            commands::set_stop_time,
            commands::get_week_start,
            commands::set_week_start,
            commands::get_break_settings,
            commands::set_break_settings,
            commands::start_break,
            commands::end_break,
            commands::snooze_break,
            commands::skip_break,
            commands::get_focus_spans,
            commands::label_focus,
            commands::get_dashboard,
            commands::set_view,
            commands::resize_window,
            commands::fit_window,
            commands::dismiss_popup,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running achieve");
}

fn build_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let nudge = MenuItem::with_id(app, "nudge", "Tasks", true, None::<&str>)?;
    let dash = MenuItem::with_id(app, "dashboard", "Dashboard", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&nudge, &dash, &sep, &quit])?;

    TrayIconBuilder::with_id("achieve-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Achieve")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "nudge" => window::show_view(app, "nudge"),
            "dashboard" => window::show_view(app, "dashboard"),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                ..
            } = event
            {
                window::show_view(tray.app_handle(), "nudge");
            }
        })
        .build(app)?;
    Ok(())
}
