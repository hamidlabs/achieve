//! Single adaptive window: it morphs size to fit whichever view is active and
//! re-centers, so the app feels like a family of focused cards rather than one
//! big window. Both the command layer (user navigation) and the engine
//! (proactive nudges) drive it through here.

use tauri::{AppHandle, Emitter, LogicalSize, Manager};

/// While a rest break is running the UI is locked to the break window: any
/// request to show another surface (tray Tasks/Dashboard, engine nudge) is
/// redirected to "break", so nothing else is reachable until the break ends.
fn break_lock(app: &AppHandle, view: &str) -> bool {
    if view == "break" {
        return false;
    }
    app.try_state::<crate::AppState>()
        .and_then(|state| state.db.lock().ok().map(|c| crate::db::on_break(&c)))
        .unwrap_or(false)
}

/// On Wayland a client cannot position itself, and niri keeps a floating
/// window's top-left fixed when it is resized in place, so growing from the
/// small nudge to a big view appears to expand from the corner. Ask niri to
/// re-center the focused (just-resized) window. We retry on a short schedule
/// because the resize is only committed after the webview reflows, a frame or
/// two later. No-op on other compositors (win.center() handles those).
fn niri_recenter() {
    if std::env::var_os("NIRI_SOCKET").is_none() {
        return;
    }
    std::thread::spawn(|| {
        for delay in [70u64, 130, 200, 320, 450] {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            let _ = std::process::Command::new("niri")
                .args(["msg", "action", "center-window"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    });
}

/// Logical (width, height) for each view.
pub fn size_for(view: &str) -> (f64, f64) {
    match view {
        "dashboard" => (880.0, 600.0),
        "editor" => (560.0, 600.0),
        "break" => (440.0, 480.0),
        // "nudge" = the unified task hub (planning + tracking + tasks + done)
        _ => (468.0, 672.0),
    }
}

/// Size the tasks hub to exactly fit its content height (the frontend measures
/// it), so there's no dead space below the list. Width is fixed; height clamped.
pub fn fit_height(app: &AppHandle, height: f64) {
    if let Some(win) = app.get_webview_window("main") {
        let h = height.clamp(200.0, 760.0);
        let _ = win.set_size(LogicalSize::new(468.0, h));
        let _ = win.center();
        niri_recenter();
    }
}

/// Resize + recenter the window to a view's footprint WITHOUT changing the
/// routed view (used to grow the window for an inline overlay like the editor).
pub fn resize_only(app: &AppHandle, view: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let (w, h) = size_for(view);
        let _ = win.set_size(LogicalSize::new(w, h));
        let _ = win.center();
        niri_recenter();
    }
}

/// Show the given view: resize, center, reveal, focus, and tell the frontend.
pub fn show_view(app: &AppHandle, view: &str) {
    // Lock to the break window while a break is in progress.
    let view = if break_lock(app, view) { "break" } else { view };
    if let Some(win) = app.get_webview_window("main") {
        let (w, h) = size_for(view);
        // Switch the view before revealing so we don't flash the previous one.
        let _ = app.emit("navigate", view);
        let _ = win.set_size(LogicalSize::new(w, h));
        let _ = win.center();
        let _ = win.show();
        let _ = win.set_focus();
        niri_recenter();
    }
}
