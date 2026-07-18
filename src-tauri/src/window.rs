//! Single adaptive window: it morphs size to fit whichever view is active, so
//! the app feels like a family of focused cards rather than one big window. Both
//! the command layer (user navigation) and the engine (proactive nudges) drive
//! it through here.
//!
//! Focus is left entirely to the compositor: we never raise or self-focus (a
//! Wayland client can't anyway, and the old retry storms only fought niri).
//! Positioning is one exception: niri auto-centers a floating window when it
//! opens, but a resize grows it from its anchored corner, so it drifts off
//! center. After every size change we ask niri once to re-center our hub window
//! (`niri_center`) so the card always stays centered on screen.

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

/// Find one of our niri windows by its exact title. Both our windows share the
/// app_id "Achieve", so the title is what disambiguates the hub ("Achieve"), the
/// break overlay ("Achieve Break"), and the second-monitor veil ("Achieve Veil").
/// Only used to place the break veil on the other monitor.
fn niri_window_id(title: &str) -> Option<u64> {
    let out = std::process::Command::new("niri")
        .args(["msg", "--json", "windows"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let windows: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    for w in windows.as_array()? {
        if w.get("title").and_then(|v| v.as_str()) == Some(title) {
            return w.get("id").and_then(|v| v.as_u64());
        }
    }
    None
}

/// The name of every connected niri output, plus the currently focused one.
fn niri_outputs_and_focused() -> Option<(Vec<String>, String)> {
    let outs = std::process::Command::new("niri")
        .args(["msg", "--json", "outputs"])
        .output()
        .ok()?;
    let outputs: serde_json::Value = serde_json::from_slice(&outs.stdout).ok()?;
    let names: Vec<String> = outputs.as_object()?.keys().cloned().collect();

    let foc = std::process::Command::new("niri")
        .args(["msg", "--json", "focused-output"])
        .output()
        .ok()?;
    let focused: serde_json::Value = serde_json::from_slice(&foc.stdout).ok()?;
    let focused_name = focused.get("name")?.as_str()?.to_string();
    Some((names, focused_name))
}

/// Re-center the main hub window on its output. niri centers a floating window
/// when it first opens, but a resize grows it from its anchored corner, so after
/// every size change we ask niri to center it again. Targeted by title so it only
/// ever touches the hub ("Achieve"), never the break/veil surfaces. Best-effort
/// (any niri hiccup is a silent no-op) and fired exactly once per resize (no
/// retry storm) to stay friendly with niri's animation `slowdown`.
fn niri_center() {
    std::thread::spawn(|| {
        // Let the resize settle first, so niri centers the new size, not the old.
        std::thread::sleep(std::time::Duration::from_millis(60));
        if let Some(id) = niri_window_id("Achieve") {
            let _ = std::process::Command::new("niri")
                .args(["msg", "action", "center-window", "--id", &id.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    });
}

/// Cover the OTHER monitor with the dimming veil during a break. On a single
/// monitor there's nothing to cover, so the veil stays hidden. Best-effort: any
/// niri hiccup just leaves the second screen uncovered, never crashes.
pub fn cover_second_monitor(app: &AppHandle) {
    let veil = match app.get_webview_window("veil") {
        Some(w) => w,
        None => return,
    };
    let other = match niri_outputs_and_focused() {
        Some((names, focused)) => names.into_iter().find(|n| *n != focused),
        None => None,
    };
    let other = match other {
        Some(o) => o,
        None => {
            // Single monitor (or couldn't tell): don't show a stray veil.
            let _ = veil.hide();
            return;
        }
    };
    let _ = veil.set_resizable(true);
    let _ = veil.set_title("Achieve Veil");
    let _ = veil.show();
    // Move it onto the other output, then fullscreen it there. Retry until the
    // surface is mapped and niri can find it by title.
    std::thread::spawn(move || {
        for delay in [90u64, 160, 260, 400, 600] {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            if let Some(id) = niri_window_id("Achieve Veil") {
                let _ = std::process::Command::new("niri")
                    .args(["msg", "action", "move-window-to-monitor", "--id", &id.to_string(), &other])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                std::thread::sleep(std::time::Duration::from_millis(110));
                let _ = veil.set_fullscreen(true);
                break;
            }
        }
    });
}

/// Drop the second-monitor veil when a break ends.
pub fn hide_veil(app: &AppHandle) {
    if let Some(veil) = app.get_webview_window("veil") {
        let _ = veil.set_fullscreen(false);
        let _ = veil.hide();
    }
}

/// Whether a break is actually running right now (not just the prompt showing).
fn is_on_break(app: &AppHandle) -> bool {
    app.try_state::<crate::AppState>()
        .and_then(|state| state.db.lock().ok().map(|c| crate::db::on_break(&c)))
        .unwrap_or(false)
}

/// Logical (width, height) for each view. One fixed, phone-like frame for every
/// non-break surface: a tall card with a persistent bottom nav, content scrolls
/// inside. The dashboard used to open wide (880px); it now shares the hub width
/// so the whole app reads as one consistent mobile surface. Editors/pickers are
/// in-frame overlays, so they don't need their own window size.
pub fn size_for(view: &str) -> (f64, f64) {
    match view {
        "break" => (440.0, 480.0),
        _ => (468.0, 760.0),
    }
}

/// Size the tasks hub to exactly fit its content height (the frontend measures
/// it), so there's no dead space below the list. Width is fixed; height clamped.
/// Re-centers after the resize so the card doesn't drift off screen.
pub fn fit_height(app: &AppHandle, height: f64) {
    if let Some(win) = app.get_webview_window("main") {
        let h = height.clamp(200.0, 760.0);
        let _ = win.set_size(LogicalSize::new(468.0, h));
        niri_center();
    }
}

/// Resize the window to a view's footprint WITHOUT changing the routed view
/// (used to grow the window for an inline overlay like the editor).
pub fn resize_only(app: &AppHandle, view: &str) {
    if let Some(win) = app.get_webview_window("main") {
        let (w, h) = size_for(view);
        let _ = win.set_size(LogicalSize::new(w, h));
        niri_center();
    }
}

/// Show the given view: resize and show, then tell the frontend. Positioning and
/// focus are the compositor's job (niri floats + centers via its rules).
///
/// The break view takes over the whole screen as a full, dimmed overlay. This
/// needs the window to be resizable (its fixed-size hint would otherwise pin it
/// small even when fullscreen) and its title switched to "Achieve Break" so a
/// dedicated niri rule can make it a borderless, un-rounded, edge-to-edge
/// fullscreen window. Every other view is the compact floating card, so we undo
/// both on the way out.
pub fn show_view(app: &AppHandle, view: &str) {
    // Lock to the break window while a break is in progress.
    let view = if break_lock(app, view) { "break" } else { view };
    if let Some(win) = app.get_webview_window("main") {
        // Switch the view before revealing so we don't flash the previous one.
        let _ = app.emit("navigate", view);
        if view == "break" {
            let _ = win.set_resizable(true);
            let _ = win.set_title("Achieve Break");
            let _ = win.set_fullscreen(true);
            let _ = win.show();
            // Dim the OTHER monitor too, but only once a break is actually
            // running (the pre-break prompt shouldn't blank the second screen).
            if is_on_break(app) {
                cover_second_monitor(app);
            } else {
                hide_veil(app);
            }
        } else {
            hide_veil(app);
            let _ = win.set_fullscreen(false);
            let _ = win.set_title("Achieve");
            let _ = win.set_resizable(false);
            let (w, h) = size_for(view);
            let _ = win.set_size(LogicalSize::new(w, h));
            let _ = win.show();
            niri_center();
        }
    }
}
