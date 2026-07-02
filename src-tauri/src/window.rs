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
            niri_center();
        }
    });
}

/// Reveal the popup on niri when it is shown proactively. A Wayland client can't
/// raise or focus itself, and a freshly (re)mapped floating window may land on
/// another workspace or behind the current one, so a proactive popup would
/// "play its sound but never appear". We look our window up by id and tell niri
/// to focus it (which switches to its workspace, bringing it into view) and then
/// center it. Retried on a short schedule because the surface is only mapped a
/// frame or two after show(), and its id can change across hide/show.
fn niri_reveal() {
    if std::env::var_os("NIRI_SOCKET").is_none() {
        return;
    }
    std::thread::spawn(|| {
        for delay in [60u64, 120, 200, 320, 460, 650] {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            if let Some(id) = niri_window_id() {
                let _ = std::process::Command::new("niri")
                    .args(["msg", "action", "focus-window", "--id", &id.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            }
            niri_center();
        }
    });
}

/// Center the currently focused window via niri.
fn niri_center() {
    let _ = std::process::Command::new("niri")
        .args(["msg", "action", "center-window"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Find our popup's niri window id by app_id/title ("Achieve").
fn niri_window_id() -> Option<u64> {
    let out = std::process::Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let windows: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    for w in windows.as_array()? {
        let app_id = w.get("app_id").and_then(|v| v.as_str()).unwrap_or("");
        let title = w.get("title").and_then(|v| v.as_str()).unwrap_or("");
        if app_id == "Achieve" || title == "Achieve" {
            return w.get("id").and_then(|v| v.as_u64());
        }
    }
    None
}

/// Logical (width, height) for each view.
pub fn size_for(view: &str) -> (f64, f64) {
    match view {
        "dashboard" => (880.0, 600.0),
        // Compact task dialog: same width as the hub (no width jump), a touch
        // taller so the centered card floats over the dimmed list behind it.
        "editor" => (468.0, 560.0),
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
/// The break view takes over the whole screen as a fullscreen, semi-transparent
/// overlay (like a professional break app); every other view is the compact
/// floating card, so we drop fullscreen on the way out.
pub fn show_view(app: &AppHandle, view: &str) {
    // Lock to the break window while a break is in progress.
    let view = if break_lock(app, view) { "break" } else { view };
    if let Some(win) = app.get_webview_window("main") {
        // Switch the view before revealing so we don't flash the previous one.
        let _ = app.emit("navigate", view);
        if view == "break" {
            let _ = win.set_fullscreen(true);
        } else {
            let _ = win.set_fullscreen(false);
            let (w, h) = size_for(view);
            let _ = win.set_size(LogicalSize::new(w, h));
            let _ = win.center();
        }
        let _ = win.show();
        let _ = win.set_focus();
        niri_reveal();
    }
}
