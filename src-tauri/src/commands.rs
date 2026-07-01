//! Tauri command layer: the bridge the Svelte frontend calls via `invoke`.

use tauri::{AppHandle, Manager, State};

use crate::db;
use crate::model::{BreakSettings, Category, Dashboard, DayPlan, FocusSpan, Snapshot, Task};
use crate::window;
use crate::AppState;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

macro_rules! with_db {
    ($state:expr, $conn:ident => $body:expr) => {{
        let $conn = $state.db.lock().map_err(err)?;
        $body.map_err(err)
    }};
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> CmdResult<Snapshot> {
    with_db!(state, c => db::snapshot(&c))
}

#[tauri::command]
pub fn list_tasks(state: State<'_, AppState>) -> CmdResult<Vec<Task>> {
    with_db!(state, c => db::list_tasks(&c))
}

#[tauri::command]
pub fn list_upcoming(state: State<'_, AppState>) -> CmdResult<Vec<Task>> {
    with_db!(state, c => db::list_upcoming(&c))
}

#[tauri::command]
pub fn list_categories(state: State<'_, AppState>) -> CmdResult<Vec<Category>> {
    with_db!(state, c => db::list_categories(&c))
}

#[tauri::command]
pub fn create_category(
    state: State<'_, AppState>,
    name: String,
    color: String,
) -> CmdResult<i64> {
    with_db!(state, c => db::create_category(&c, &name, &color))
}

#[tauri::command]
pub fn delete_category(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_db!(state, c => db::delete_category(&c, id))
}

#[tauri::command]
pub fn create_task(
    state: State<'_, AppState>,
    category_id: Option<i64>,
    title: String,
    body_md: String,
    estimate_min: Option<i64>,
    recurrence: Option<String>,
) -> CmdResult<i64> {
    with_db!(state, c => db::create_task(&c, category_id, &title, &body_md, estimate_min, recurrence.as_deref()))
}

#[tauri::command]
pub fn update_task(
    state: State<'_, AppState>,
    id: i64,
    category_id: Option<i64>,
    title: String,
    body_md: String,
    estimate_min: Option<i64>,
    recurrence: Option<String>,
) -> CmdResult<()> {
    with_db!(state, c => db::update_task(&c, id, category_id, &title, &body_md, estimate_min, recurrence.as_deref()))
}

#[tauri::command]
pub fn delete_task(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_db!(state, c => db::delete_task(&c, id))
}

#[tauri::command]
pub fn start_task(state: State<'_, AppState>, task_id: i64) -> CmdResult<()> {
    with_db!(state, c => db::start_task(&c, task_id))
}

#[tauri::command]
pub fn pause_task(state: State<'_, AppState>, task_id: i64, reason: String) -> CmdResult<()> {
    with_db!(state, c => db::pause_task(&c, task_id, &reason))
}

/// Grant more time to the active task (the +15m / +30m buttons): bump its
/// estimate and resume tracking if it was paused at the estimate.
#[tauri::command]
pub fn extend_active(state: State<'_, AppState>, task_id: i64, minutes: i64) -> CmdResult<()> {
    with_db!(state, c => db::extend_active(&c, task_id, minutes))
}

#[tauri::command]
pub fn complete_task(state: State<'_, AppState>, task_id: i64) -> CmdResult<()> {
    with_db!(state, c => db::complete_task(&c, task_id))
}

#[tauri::command]
pub fn reopen_task(state: State<'_, AppState>, task_id: i64) -> CmdResult<()> {
    with_db!(state, c => db::reopen_task(&c, task_id))
}

#[tauri::command]
pub fn reschedule_task(state: State<'_, AppState>, task_id: i64) -> CmdResult<()> {
    with_db!(state, c => db::reschedule_task(&c, task_id))
}

/// Move a task to a specific date ("YYYY-MM-DD") or no date (null = someday).
#[tauri::command]
pub fn set_plan_date(
    state: State<'_, AppState>,
    task_id: i64,
    date: Option<String>,
) -> CmdResult<()> {
    with_db!(state, c => db::set_plan_date(&c, task_id, date.as_deref()))
}

#[tauri::command]
pub fn get_day_plan(state: State<'_, AppState>) -> CmdResult<DayPlan> {
    with_db!(state, c => db::get_day_plan(&c))
}

#[tauri::command]
pub fn save_day_plan(
    state: State<'_, AppState>,
    intentions: String,
    available_minutes: i64,
    stop_time: Option<String>,
) -> CmdResult<()> {
    with_db!(state, c => db::save_day_plan(&c, &intentions, available_minutes, stop_time.as_deref()))
}

#[tauri::command]
pub fn set_stop_time(state: State<'_, AppState>, stop_time: String) -> CmdResult<()> {
    with_db!(state, c => db::set_stop_time(&c, &stop_time))
}

/// The day the week starts on (0=Sunday..6=Saturday); drives the week dashboard.
#[tauri::command]
pub fn get_week_start(state: State<'_, AppState>) -> CmdResult<i64> {
    with_db!(state, c => Ok::<i64, String>(db::get_week_start(&c)))
}

#[tauri::command]
pub fn set_week_start(state: State<'_, AppState>, day: i64) -> CmdResult<()> {
    with_db!(state, c => db::set_week_start(&c, day))
}

// ---- rest breaks ----

#[tauri::command]
pub fn get_break_settings(state: State<'_, AppState>) -> CmdResult<BreakSettings> {
    with_db!(state, c => Ok::<BreakSettings, String>(db::get_break_settings(&c)))
}

#[tauri::command]
pub fn set_break_settings(state: State<'_, AppState>, settings: BreakSettings) -> CmdResult<()> {
    with_db!(state, c => db::set_break_settings(&c, &settings))
}

/// Start a break now (also used by the prompt's "Take break").
#[tauri::command]
pub fn start_break(state: State<'_, AppState>, app: AppHandle) -> CmdResult<()> {
    with_db!(state, c => db::start_break(&c))?;
    let _ = emit_snapshot(&app, &state);
    Ok(())
}

/// End the current break; `resume` re-starts the task that was running before.
#[tauri::command]
pub fn end_break(state: State<'_, AppState>, app: AppHandle, resume: bool) -> CmdResult<()> {
    with_db!(state, c => db::end_break(&c, resume))?;
    let _ = emit_snapshot(&app, &state);
    Ok(())
}

#[tauri::command]
pub fn snooze_break(state: State<'_, AppState>, minutes: i64) -> CmdResult<()> {
    with_db!(state, c => db::snooze_break(&c, minutes))
}

#[tauri::command]
pub fn skip_break(state: State<'_, AppState>) -> CmdResult<()> {
    with_db!(state, c => db::skip_break(&c))
}

/// Push a fresh snapshot to the UI right after a break action so the view flips
/// without waiting for the next engine tick.
fn emit_snapshot(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), String> {
    use tauri::Emitter;
    let snap = with_db!(state, c => db::snapshot(&c))?;
    app.emit("snapshot", &snap).map_err(err)
}

#[tauri::command]
pub fn get_focus_spans(state: State<'_, AppState>) -> CmdResult<Vec<FocusSpan>> {
    with_db!(state, c => db::focus_spans(&c))
}

#[tauri::command]
pub fn label_focus(
    state: State<'_, AppState>,
    focus_id: i64,
    label: String,
    task_id: Option<i64>,
) -> CmdResult<()> {
    with_db!(state, c => db::label_focus(&c, focus_id, &label, task_id))
}

#[tauri::command]
pub fn get_dashboard(
    state: State<'_, AppState>,
    period: Option<String>,
    offset: Option<i64>,
) -> CmdResult<Dashboard> {
    let p = period.unwrap_or_else(|| "day".into());
    let off = offset.unwrap_or(0);
    with_db!(state, c => db::dashboard(&c, &p, off))
}

/// Navigate the single adaptive window to a view (resize + center + show).
#[tauri::command]
pub fn set_view(app: AppHandle, view: String) -> CmdResult<()> {
    window::show_view(&app, &view);
    Ok(())
}

/// Resize the window without changing the routed view (for inline overlays).
#[tauri::command]
pub fn resize_window(app: AppHandle, view: String) -> CmdResult<()> {
    window::resize_only(&app, &view);
    Ok(())
}

/// Size the tasks hub to its measured content height (no dead space).
#[tauri::command]
pub fn fit_window(app: AppHandle, height: f64) -> CmdResult<()> {
    window::fit_height(&app, height);
    Ok(())
}

#[tauri::command]
pub fn dismiss_popup(app: AppHandle) -> CmdResult<()> {
    if let Some(win) = app.get_webview_window("main") {
        win.hide().map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Send the daily summary email immediately for the given day offset (0 = today,
/// 1 = yesterday). Builds under the DB lock, then does the HTTP call unlocked.
#[tauri::command]
pub fn send_summary_now(state: State<'_, AppState>, offset: Option<i64>) -> CmdResult<String> {
    let off = offset.unwrap_or(1);
    let payload = {
        let c = state.db.lock().map_err(err)?;
        crate::email::build_payload(&c, off)?
    };
    crate::email::send(&payload)?;
    {
        let c = state.db.lock().map_err(err)?;
        crate::email::mark_sent(&c);
    }
    Ok(format!("Sent \"{}\" to {}", payload.subject, payload.to))
}
