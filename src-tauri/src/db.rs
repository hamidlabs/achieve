//! SQLite persistence and the append-only time ledger.
//!
//! Design notes:
//! - Time is NOT stored as a running counter. Every work interval is an
//!   append-only row in `segments` with a start and (once stopped) an end.
//!   A task's tracked time is always derived as the sum of its segments. This
//!   makes completion non-terminal: a completed task can be reopened and simply
//!   accrue more segments, and nothing is ever double counted.
//! - `focus_log` is the automatic ground truth (which app/window had focus),
//!   captured passively. Reconciliation maps a span onto a task or marks it as
//!   distraction.
//! - Timestamps are stored as UTC "YYYY-MM-DD HH:MM:SS" so SQLite's date()
//!   and julianday() (and the 'localtime' modifier) parse them reliably.

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;

use crate::model::{
    AppStat, Bar, BreakSettings, CategoryStat, Dashboard, DayPlan, FocusSpan, PlannedActual,
    Snapshot, Task,
};

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    seed(&conn)?;
    recover(&conn)?;
    ensure_recurring(&conn)?;
    ensure_break(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS categories (
            id        INTEGER PRIMARY KEY,
            name      TEXT NOT NULL,
            color     TEXT NOT NULL DEFAULT '#6b6bff',
            parent_id INTEGER REFERENCES categories(id),
            sort      INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS tasks (
            id           INTEGER PRIMARY KEY,
            category_id  INTEGER REFERENCES categories(id),
            title        TEXT NOT NULL,
            body_md      TEXT NOT NULL DEFAULT '',
            estimate_min INTEGER,
            status       TEXT NOT NULL DEFAULT 'pending',
            recurrence   TEXT,
            plan_date    TEXT,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS segments (
            id       INTEGER PRIMARY KEY,
            task_id  INTEGER NOT NULL REFERENCES tasks(id),
            start_at TEXT NOT NULL,
            end_at   TEXT,
            source   TEXT NOT NULL DEFAULT 'manual',
            reason   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_segments_task ON segments(task_id);

        CREATE TABLE IF NOT EXISTS focus_log (
            id       INTEGER PRIMARY KEY,
            app_id   TEXT,
            title    TEXT,
            start_at TEXT NOT NULL,
            end_at   TEXT,
            label    TEXT
        );

        CREATE TABLE IF NOT EXISTS day_plans (
            date              TEXT PRIMARY KEY,
            intentions        TEXT NOT NULL DEFAULT '',
            available_minutes INTEGER NOT NULL DEFAULT 480,
            stop_time         TEXT
        );

        CREATE TABLE IF NOT EXISTS app_settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;

    // Migration: attribute each automatic focus span to the task that was
    // active when it was captured, so the dashboard can show exact per-task app
    // usage. Older DBs lack the column; add it if missing.
    if !column_exists(conn, "focus_log", "task_id")? {
        conn.execute("ALTER TABLE focus_log ADD COLUMN task_id INTEGER", [])?;
    }
    Ok(())
}

/// Whether `table` already has a column named `col` (for idempotent migrations).
fn column_exists(conn: &Connection, table: &str, col: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let name: String = r.get(1)?;
        if name == col {
            return Ok(true);
        }
    }
    Ok(false)
}

fn seed(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
    if count == 0 {
        let defaults = [
            ("Job X", "#6b6bff"),
            ("Job Y", "#37b6a6"),
            ("Courses", "#e0a23c"),
            ("Self-improvement", "#c06bd8"),
        ];
        for (i, (name, color)) in defaults.iter().enumerate() {
            conn.execute(
                "INSERT INTO categories (name, color, sort) VALUES (?1, ?2, ?3)",
                params![name, color, i as i64],
            )?;
        }
    }

    let task_count: i64 = conn.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
    if task_count == 0 {
        let day = today(conn);
        let samples = [
            (1_i64, "Ship the invoice export", "Finish the **CSV + PDF** export.\n\n- [ ] map columns\n- [ ] totals row\n- [ ] email to client", Some(90_i64), None),
            (2, "Client Y: review PR backlog", "Clear the review queue before standup.", Some(120), None),
            (3, "Course: lesson 4 + exercises", "Linear algebra, chapter 4.", Some(60), Some("daily")),
            (4, "Read 20 pages", "Deep Work, continue.", Some(40), Some("daily")),
        ];
        for (cat, title, body, est, rec) in samples {
            conn.execute(
                "INSERT INTO tasks (category_id, title, body_md, estimate_min, status, recurrence, plan_date, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?7)",
                params![cat, title, body, est, rec, day, now()],
            )?;
        }
    }
    Ok(())
}

/// On startup, close any segment/focus row left open by an unclean exit so we
/// never count hours the user was away. Open work segments are capped to one
/// pomodoro (25 min) past their start.
fn recover(conn: &Connection) -> Result<()> {
    let n = now();
    conn.execute(
        "UPDATE segments SET end_at = ?1
         WHERE end_at IS NULL AND (julianday(?1) - julianday(start_at)) * 1440 < 25",
        params![n],
    )?;
    conn.execute(
        "UPDATE segments SET end_at = datetime(start_at, '+25 minutes') WHERE end_at IS NULL",
        [],
    )?;
    conn.execute(
        "UPDATE tasks SET status = 'paused' WHERE status = 'in_progress'",
        [],
    )?;
    conn.execute("UPDATE focus_log SET end_at = ?1 WHERE end_at IS NULL", params![n])?;
    Ok(())
}

/// The seat has been idle for `idle_secs`: stop both the open work segment AND
/// the open focus span at the moment input actually ceased (now - idle_secs,
/// never before they started), and pause the active task. This is how a task
/// left running overnight stops accruing hours the user was asleep for, and how
/// idle time is kept out of the "untracked" (distraction) total. Returns true if
/// anything was closed (so the caller can refresh the UI).
pub fn pause_for_idle(conn: &Connection, idle_secs: i64) -> Result<bool> {
    let cutoff = format!("-{idle_secs} seconds");
    let cap = |table: &str| -> Result<usize> {
        Ok(conn.execute(
            &format!(
                "UPDATE {table}
                    SET end_at = CASE WHEN start_at > datetime('now', ?1) THEN start_at
                                      ELSE datetime('now', ?1) END
                  WHERE end_at IS NULL"
            ),
            params![cutoff],
        )?)
    };
    // Mark the capped work segment as auto-idle for transparency.
    let closed_seg = conn.execute(
        "UPDATE segments
            SET end_at = CASE WHEN start_at > datetime('now', ?1) THEN start_at
                              ELSE datetime('now', ?1) END,
                reason = COALESCE(reason, 'auto-idle')
          WHERE end_at IS NULL",
        params![cutoff],
    )?;
    let closed_focus = cap("focus_log")?;
    if closed_seg > 0 {
        conn.execute(
            "UPDATE tasks SET status = 'paused', updated_at = ?1 WHERE status = 'in_progress'",
            params![now()],
        )?;
    }
    Ok(closed_seg > 0 || closed_focus > 0)
}

/// Roll daily recurring tasks into today: if a 'daily' task is from a previous
/// day (or unscheduled), reset it to pending for today. One row per recurring
/// task, reappearing each day; per-day time lives in segments.
fn ensure_recurring(conn: &Connection) -> Result<()> {
    let day = today(conn);
    conn.execute(
        "UPDATE tasks SET status = 'pending', plan_date = ?1, updated_at = ?2
         WHERE recurrence = 'daily' AND (plan_date IS NULL OR plan_date < ?1)",
        params![day, now()],
    )?;
    Ok(())
}

fn now() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
// "today"/"tomorrow" and all wall-clock math go through SQLite's localtime,
// which uses the OS timezone reliably. chrono::Local mis-resolved the timezone
// inside the AppImage, so we do NOT use it for dates.
fn today(conn: &Connection) -> String {
    conn.query_row("SELECT date('now','localtime')", [], |r| r.get(0))
        .unwrap_or_default()
}
fn tomorrow(conn: &Connection) -> String {
    conn.query_row("SELECT date('now','localtime','+1 day')", [], |r| r.get(0))
        .unwrap_or_default()
}
/// Current local time as minutes from midnight.
fn local_now_min(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT CAST(strftime('%H','now','localtime') AS INTEGER)*60 + CAST(strftime('%M','now','localtime') AS INTEGER)",
        [], |r| r.get(0),
    ).unwrap_or(0)
}
/// Current local hour (0-23).
fn local_hour(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT CAST(strftime('%H','now','localtime') AS INTEGER)",
        [],
        |r| r.get(0),
    )
    .unwrap_or(12)
}

fn tracked_minutes(conn: &Connection, task_id: i64) -> Result<i64> {
    let secs: i64 = conn.query_row(
        "SELECT COALESCE(SUM(CAST((julianday(COALESCE(end_at, ?1)) - julianday(start_at)) * 86400 AS INTEGER)), 0)
         FROM segments WHERE task_id = ?2",
        params![now(), task_id],
        |r| r.get(0),
    )?;
    Ok(secs / 60)
}

pub fn create_category(conn: &Connection, name: &str, color: &str) -> Result<i64> {
    let sort: i64 =
        conn.query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM categories", [], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO categories (name, color, sort) VALUES (?1, ?2, ?3)",
        params![name, color, sort],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Delete a category. Tasks in it are NOT deleted; they become uncategorized.
pub fn delete_category(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET category_id = NULL WHERE category_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM categories WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn list_categories(conn: &Connection) -> Result<Vec<crate::model::Category>> {
    let mut stmt =
        conn.prepare("SELECT id, name, color, parent_id FROM categories ORDER BY sort, id")?;
    let rows = stmt.query_map([], |r| {
        Ok(crate::model::Category {
            id: r.get(0)?,
            name: r.get(1)?,
            color: r.get(2)?,
            parent_id: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn list_tasks(conn: &Connection) -> Result<Vec<Task>> {
    let day = today(conn);
    let mut stmt = conn.prepare(
        r#"
        SELECT t.id, t.category_id, c.name, c.color, t.title, t.body_md,
               t.estimate_min, t.status, t.recurrence, t.plan_date
        FROM tasks t LEFT JOIN categories c ON c.id = t.category_id
        WHERE (t.plan_date = ?1 OR t.plan_date IS NULL)
          AND t.status <> 'break' AND t.id <> COALESCE(?2, -1)
        ORDER BY
            CASE t.status WHEN 'in_progress' THEN 0 WHEN 'paused' THEN 1
                WHEN 'pending' THEN 2 WHEN 'reopened' THEN 2 ELSE 4 END,
            t.id
        "#,
    )?;
    let btid = break_task_id(conn);
    let rows = stmt.query_map(params![day, btid], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<String>>(9)?,
        ))
    })?;

    let mut tasks = Vec::new();
    for row in rows {
        let (id, cid, cname, ccolor, title, body, est, status, rec, plan) = row?;
        let tracked_min = tracked_minutes(conn, id)?;
        tasks.push(Task {
            id,
            category_id: cid,
            category_name: cname,
            category_color: ccolor,
            title,
            body_md: body,
            estimate_min: est,
            status,
            recurrence: rec,
            plan_date: plan,
            tracked_min,
        });
    }
    Ok(tasks)
}

/// Tasks scheduled for a future day (rescheduled / planned ahead). These are
/// excluded from `list_tasks` (today-only), so the hub surfaces them in their
/// own "Upcoming" section with a one-tap "Today".
pub fn list_upcoming(conn: &Connection) -> Result<Vec<Task>> {
    let day = today(conn);
    let mut stmt = conn.prepare(
        r#"
        SELECT t.id, t.category_id, c.name, c.color, t.title, t.body_md,
               t.estimate_min, t.status, t.recurrence, t.plan_date
        FROM tasks t LEFT JOIN categories c ON c.id = t.category_id
        WHERE t.plan_date > ?1 AND t.status != 'completed'
        ORDER BY t.plan_date, t.id
        "#,
    )?;
    let rows = stmt.query_map(params![day], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, Option<String>>(8)?,
            r.get::<_, Option<String>>(9)?,
        ))
    })?;
    let mut tasks = Vec::new();
    for row in rows {
        let (id, cid, cname, ccolor, title, body, est, status, rec, plan) = row?;
        let tracked_min = tracked_minutes(conn, id)?;
        tasks.push(Task {
            id, category_id: cid, category_name: cname, category_color: ccolor,
            title, body_md: body, estimate_min: est, status, recurrence: rec,
            plan_date: plan, tracked_min,
        });
    }
    Ok(tasks)
}

pub fn create_task(
    conn: &Connection,
    category_id: Option<i64>,
    title: &str,
    body_md: &str,
    estimate_min: Option<i64>,
    recurrence: Option<&str>,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO tasks (category_id, title, body_md, estimate_min, status, recurrence, plan_date, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?7)",
        params![category_id, title, body_md, estimate_min, recurrence, today(conn), now()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_task(
    conn: &Connection,
    id: i64,
    category_id: Option<i64>,
    title: &str,
    body_md: &str,
    estimate_min: Option<i64>,
    recurrence: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET category_id=?1, title=?2, body_md=?3, estimate_min=?4, recurrence=?5, updated_at=?6 WHERE id=?7",
        params![category_id, title, body_md, estimate_min, recurrence, now(), id],
    )?;
    Ok(())
}

pub fn delete_task(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM segments WHERE task_id = ?1", params![id])?;
    conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    Ok(())
}

fn close_open_segment(conn: &Connection, reason: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE segments SET end_at = ?1, reason = COALESCE(?2, reason) WHERE end_at IS NULL",
        params![now(), reason],
    )?;
    Ok(())
}

pub fn start_task(conn: &Connection, task_id: i64) -> Result<()> {
    close_open_segment(conn, None)?;
    conn.execute(
        "UPDATE tasks SET status='paused', updated_at=?1 WHERE status='in_progress'",
        params![now()],
    )?;
    conn.execute(
        "INSERT INTO segments (task_id, start_at, source) VALUES (?1, ?2, 'manual')",
        params![task_id, now()],
    )?;
    conn.execute(
        "UPDATE tasks SET status='in_progress', updated_at=?1 WHERE id=?2",
        params![now(), task_id],
    )?;
    Ok(())
}

pub fn pause_task(conn: &Connection, task_id: i64, reason: &str) -> Result<()> {
    close_open_segment(conn, Some(reason))?;
    conn.execute(
        "UPDATE tasks SET status='paused', updated_at=?1 WHERE id=?2",
        params![now(), task_id],
    )?;
    Ok(())
}

/// The active task has reached its estimate: stop the clock exactly at the
/// estimate boundary (cap the open segment so the task's total tracked equals
/// the estimate, never into the future) and move it to `awaiting`, so it stops
/// accruing and waits for the user to extend or finish. This is why a task left
/// running past its estimate (you stepped away / the popup went unanswered)
/// never bleeds phantom hours. Returns false if nothing was tracking.
pub fn pause_at_estimate(conn: &Connection, task_id: i64, estimate_min: i64) -> Result<bool> {
    let open: Option<i64> = conn
        .query_row(
            "SELECT id FROM segments WHERE task_id=?1 AND end_at IS NULL",
            params![task_id],
            |r| r.get(0),
        )
        .ok();
    let seg_id = match open {
        Some(id) => id,
        None => return Ok(false),
    };
    // Seconds already banked in this task's CLOSED segments; the open one may
    // run only long enough to bring the total up to the estimate.
    let closed_secs: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(CAST((julianday(end_at)-julianday(start_at))*86400 AS INTEGER)),0)
             FROM segments WHERE task_id=?1 AND end_at IS NOT NULL",
            params![task_id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let allow_secs = (estimate_min * 60 - closed_secs).max(0);
    conn.execute(
        "UPDATE segments
            SET end_at = MIN(datetime(start_at, ?2), strftime('%Y-%m-%d %H:%M:%S','now')),
                reason = COALESCE(reason, 'reached-estimate')
          WHERE id = ?1",
        params![seg_id, format!("+{allow_secs} seconds")],
    )?;
    conn.execute(
        "UPDATE tasks SET status='awaiting', updated_at=?1 WHERE id=?2",
        params![now(), task_id],
    )?;
    Ok(true)
}

/// Grant more time to an `awaiting` (or still-running) task: bump its estimate by
/// `minutes` and make sure the clock is running again (open a fresh segment +
/// flip to in_progress if nothing is currently open). This is what the popup's
/// +15m / +30m buttons call so the clock resumes.
pub fn extend_active(conn: &Connection, task_id: i64, minutes: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET estimate_min = COALESCE(estimate_min,0) + ?1, updated_at=?2 WHERE id=?3",
        params![minutes, now(), task_id],
    )?;
    let has_open: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM segments WHERE task_id=?1 AND end_at IS NULL)",
        params![task_id],
        |r| r.get(0),
    )?;
    if has_open {
        conn.execute(
            "UPDATE tasks SET status='in_progress', updated_at=?1 WHERE id=?2",
            params![now(), task_id],
        )?;
    } else {
        // Closes any other open segment, opens a new one, sets in_progress.
        start_task(conn, task_id)?;
    }
    Ok(())
}

pub fn complete_task(conn: &Connection, task_id: i64) -> Result<()> {
    close_open_segment(conn, None)?;
    conn.execute(
        "UPDATE tasks SET status='completed', updated_at=?1 WHERE id=?2",
        params![now(), task_id],
    )?;
    Ok(())
}

/// Reopen a completed task (e.g. client comes back). Status flips; new segments
/// will add to the same task, history intact.
pub fn reopen_task(conn: &Connection, task_id: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET status='reopened', plan_date=?1, updated_at=?2 WHERE id=?3",
        params![today(conn), now(), task_id],
    )?;
    Ok(())
}

/// Push a task to tomorrow.
pub fn reschedule_task(conn: &Connection, task_id: i64) -> Result<()> {
    set_plan_date(conn, task_id, Some(&tomorrow(conn)))
}

/// Move a task to a specific day (or `None` for no date / someday). If it was
/// the one tracking, stop it first so we don't keep accruing time on a task
/// that's been pushed to later.
pub fn set_plan_date(conn: &Connection, task_id: i64, date: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE segments SET end_at=?1, reason=COALESCE(reason,'rescheduled')
         WHERE end_at IS NULL AND task_id=?2",
        params![now(), task_id],
    )?;
    conn.execute(
        "UPDATE tasks SET status='pending', plan_date=?1, updated_at=?2 WHERE id=?3",
        params![date, now(), task_id],
    )?;
    Ok(())
}

pub fn get_day_plan(conn: &Connection) -> Result<DayPlan> {
    let day = today(conn);
    let plan = conn
        .query_row(
            "SELECT date, intentions, available_minutes, stop_time FROM day_plans WHERE date = ?1",
            params![day],
            |r| {
                Ok(DayPlan {
                    date: r.get(0)?,
                    intentions: r.get(1)?,
                    available_minutes: r.get(2)?,
                    stop_time: r.get(3)?,
                })
            },
        )
        .unwrap_or(DayPlan {
            date: day,
            intentions: String::new(),
            available_minutes: 480,
            stop_time: Some("18:00".into()),
        });
    Ok(plan)
}

pub fn save_day_plan(
    conn: &Connection,
    intentions: &str,
    available_minutes: i64,
    stop_time: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO day_plans (date, intentions, available_minutes, stop_time)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date) DO UPDATE SET intentions=?2, available_minutes=?3, stop_time=?4",
        params![today(conn), intentions, available_minutes, stop_time],
    )?;
    Ok(())
}

pub fn set_stop_time(conn: &Connection, stop_time: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO day_plans (date, stop_time) VALUES (?1, ?2)
         ON CONFLICT(date) DO UPDATE SET stop_time=?2",
        params![today(conn), stop_time],
    )?;
    Ok(())
}

/// A generic app setting (key-value). None if unset.
fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM app_settings WHERE key=?1", params![key], |r| r.get(0))
        .ok()
}

/// The day the week starts on for the dashboard's week view: 0=Sunday..6=Saturday
/// (SQLite %w numbering). Defaults to Monday (1) so weeks run Monday->Sunday.
pub fn get_week_start(conn: &Connection) -> i64 {
    get_setting(conn, "week_start")
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|d| (0..=6).contains(d))
        .unwrap_or(1)
}

pub fn set_week_start(conn: &Connection, day: i64) -> Result<()> {
    let day = day.clamp(0, 6);
    set_setting(conn, "week_start", &day.to_string())
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rest breaks (ultradian). A dedicated "Break" category + task lets break time
// flow through the same append-only ledger, so it shows up in the dashboard
// like any other category. The work-since-break clock is stateless: it sums
// tracked (non-break) minutes since `break_anchor`, which we reset whenever a
// break is taken or skipped. Snooze defers the prompt via `break_snooze_until`.
// ---------------------------------------------------------------------------

/// Create the Break category + task (and default settings) once; idempotent.
fn ensure_break(conn: &Connection) -> Result<()> {
    if let Some(id) = break_task_id(conn) {
        let exists: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)", params![id], |r| r.get(0))?;
        if exists {
            return Ok(());
        }
    }
    let cat_id: i64 = match conn.query_row(
        "SELECT id FROM categories WHERE name='Break' LIMIT 1",
        [],
        |r| r.get(0),
    ) {
        Ok(id) => id,
        Err(_) => {
            conn.execute(
                "INSERT INTO categories (name, color, sort) VALUES ('Break', '#30b0a8', 99)",
                [],
            )?;
            conn.last_insert_rowid()
        }
    };
    // Parked status 'break' keeps it out of the pending/planned lists; it flips
    // to 'in_progress' only while a break is actually running.
    conn.execute(
        "INSERT INTO tasks (category_id, title, body_md, estimate_min, status, recurrence, plan_date, created_at, updated_at)
         VALUES (?1, 'Break', '', NULL, 'break', NULL, NULL, ?2, ?2)",
        params![cat_id, now()],
    )?;
    let tid = conn.last_insert_rowid();
    set_setting(conn, "break_task_id", &tid.to_string())?;
    for (k, v) in [
        ("breaks_enabled", "1"),
        ("break_work_min", "50"),
        ("break_duration_min", "5"),
        ("break_snooze_min", "5"),
    ] {
        conn.execute("INSERT OR IGNORE INTO app_settings (key, value) VALUES (?1, ?2)", params![k, v])?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('break_anchor', ?1)",
        params![now()],
    )?;
    Ok(())
}

pub fn break_task_id(conn: &Connection) -> Option<i64> {
    get_setting(conn, "break_task_id").and_then(|v| v.parse().ok())
}

/// True when a rest break is actively running (the break task is the one
/// tracking). Used to lock the UI to the break window: no other surface is
/// reachable from the tray until the break ends.
pub fn on_break(conn: &Connection) -> bool {
    let active: Option<i64> = conn
        .query_row("SELECT id FROM tasks WHERE status='in_progress' LIMIT 1", [], |r| r.get(0))
        .ok();
    matches!((active, break_task_id(conn)), (Some(a), Some(b)) if a == b)
}

pub fn get_break_settings(conn: &Connection) -> BreakSettings {
    let g = |k: &str, d: i64| -> i64 {
        get_setting(conn, k).and_then(|v| v.parse().ok()).unwrap_or(d)
    };
    BreakSettings {
        enabled: get_setting(conn, "breaks_enabled").map(|v| v != "0").unwrap_or(true),
        work_min: g("break_work_min", 50).clamp(5, 240),
        duration_min: g("break_duration_min", 5).clamp(1, 60),
        snooze_min: g("break_snooze_min", 5).clamp(1, 60),
    }
}

pub fn set_break_settings(conn: &Connection, s: &BreakSettings) -> Result<()> {
    set_setting(conn, "breaks_enabled", if s.enabled { "1" } else { "0" })?;
    set_setting(conn, "break_work_min", &s.work_min.clamp(5, 240).to_string())?;
    set_setting(conn, "break_duration_min", &s.duration_min.clamp(1, 60).to_string())?;
    set_setting(conn, "break_snooze_min", &s.snooze_min.clamp(1, 60).to_string())?;
    Ok(())
}

/// True if the break prompt is currently snoozed.
pub fn break_snoozed(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT value > strftime('%Y-%m-%d %H:%M:%S','now') FROM app_settings WHERE key='break_snooze_until'",
        [],
        |r| r.get::<_, bool>(0),
    )
    .unwrap_or(false)
}

/// Begin a break: pause the current task (remembering it to resume), then open a
/// segment on the Break task so the rest is tracked like any other activity.
pub fn start_break(conn: &Connection) -> Result<()> {
    let btid = match break_task_id(conn) {
        Some(id) => id,
        None => return Ok(()),
    };
    let resume: Option<i64> = conn
        .query_row(
            "SELECT id FROM tasks WHERE status='in_progress' AND id<>?1 LIMIT 1",
            params![btid],
            |r| r.get(0),
        )
        .ok();
    set_setting(conn, "break_resume_task_id", &resume.map(|i| i.to_string()).unwrap_or_default())?;
    close_open_segment(conn, None)?;
    conn.execute("UPDATE tasks SET status='paused', updated_at=?1 WHERE status='in_progress'", params![now()])?;
    conn.execute("INSERT INTO segments (task_id, start_at, source) VALUES (?1, ?2, 'auto')", params![btid, now()])?;
    conn.execute("UPDATE tasks SET status='in_progress', updated_at=?1 WHERE id=?2", params![now(), btid])?;
    Ok(())
}

/// End a break: close the break segment, reset the work clock, and optionally
/// resume the task that was running before.
pub fn end_break(conn: &Connection, resume: bool) -> Result<()> {
    let btid = match break_task_id(conn) {
        Some(id) => id,
        None => return Ok(()),
    };
    close_open_segment(conn, Some("break-end"))?;
    conn.execute("UPDATE tasks SET status='break', updated_at=?1 WHERE id=?2", params![now(), btid])?;
    set_setting(conn, "break_anchor", &now())?;
    set_setting(conn, "break_snooze_until", &now())?;
    if resume {
        if let Some(prev) = get_setting(conn, "break_resume_task_id").and_then(|v| v.parse::<i64>().ok()) {
            // Don't resume if it was completed/rescheduled in the meantime.
            let ok: bool = conn
                .query_row("SELECT status IN ('paused','pending','reopened') FROM tasks WHERE id=?1", params![prev], |r| r.get(0))
                .unwrap_or(false);
            if ok {
                start_task(conn, prev)?;
            }
        }
    }
    set_setting(conn, "break_resume_task_id", "")?;
    Ok(())
}

/// Snooze the break prompt for `min` minutes.
pub fn snooze_break(conn: &Connection, min: i64) -> Result<()> {
    let until: String = conn.query_row(
        "SELECT datetime('now', ?1)",
        params![format!("+{} minutes", min.clamp(1, 120))],
        |r| r.get(0),
    )?;
    set_setting(conn, "break_snooze_until", &until)
}

/// Skip this break: just reset the work clock so the next prompt is a full work
/// interval away.
pub fn skip_break(conn: &Connection) -> Result<()> {
    set_setting(conn, "break_anchor", &now())?;
    set_setting(conn, "break_snooze_until", &now())
}

/// Focus spans awaiting a label (work/distraction), longest first, today only,
/// at least 2 minutes long so we don't nag about quick context switches.
pub fn focus_spans(conn: &Connection) -> Result<Vec<FocusSpan>> {
    let mut stmt = conn.prepare(
        "SELECT id, app_id, title, start_at, mins FROM (
            SELECT id, app_id, title, start_at,
                   CAST((julianday(COALESCE(end_at, ?1)) - julianday(start_at)) * 1440 AS INTEGER) AS mins
            FROM focus_log
            WHERE label IS NULL AND date(start_at,'localtime') = date('now','localtime')
         ) WHERE mins >= 2 ORDER BY mins DESC LIMIT 12",
    )?;
    let rows = stmt.query_map(params![now()], |r| {
        Ok(FocusSpan {
            id: r.get(0)?,
            app_id: r.get(1)?,
            title: r.get(2)?,
            start_at: r.get(3)?,
            minutes: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Label a focus span. If labeled 'work' against a task, credit the time to
/// that task as an auto segment.
pub fn label_focus(
    conn: &Connection,
    focus_id: i64,
    label: &str,
    task_id: Option<i64>,
) -> Result<()> {
    conn.execute(
        "UPDATE focus_log SET label = ?1 WHERE id = ?2",
        params![label, focus_id],
    )?;
    if label == "work" {
        if let Some(tid) = task_id {
            let span: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT start_at, end_at FROM focus_log WHERE id = ?1",
                    params![focus_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .ok();
            if let Some((start, end)) = span {
                conn.execute(
                    "INSERT INTO segments (task_id, start_at, end_at, source) VALUES (?1, ?2, ?3, 'auto')",
                    params![tid, start, end.unwrap_or_else(now)],
                )?;
            }
        }
    }
    Ok(())
}

pub fn snapshot(conn: &Connection) -> Result<Snapshot> {
    let day = today(conn);
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status IN ('pending','paused','reopened') AND (plan_date = ?1 OR plan_date IS NULL)",
        params![day], |r| r.get(0))?;
    let in_progress: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status='in_progress'", [], |r| r.get(0))?;
    let completed_today: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE status='completed' AND date(updated_at,'localtime')=?1",
        params![day], |r| r.get(0))?;

    // The active task is the one tracking OR the one paused at its estimate
    // awaiting a decision; prefer in_progress if both somehow exist.
    let active: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, title, status FROM tasks WHERE status IN ('in_progress','awaiting')
             ORDER BY CASE status WHEN 'in_progress' THEN 0 ELSE 1 END LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let (active_task_id, active_task_title, active_awaiting) = match active {
        Some((id, t, st)) => (Some(id), Some(t), st == "awaiting"),
        None => (None, None, false),
    };
    let active_since_min: i64 = if let Some(id) = active_task_id {
        conn.query_row(
            "SELECT CAST((julianday(?1)-julianday(start_at))*1440 AS INTEGER) FROM segments WHERE task_id=?2 AND end_at IS NULL",
            params![now(), id], |r| r.get(0)).unwrap_or(0)
    } else {
        0
    };
    let active_estimate_min: Option<i64> = if let Some(id) = active_task_id {
        conn.query_row("SELECT estimate_min FROM tasks WHERE id=?1", params![id], |r| r.get(0))
            .ok()
            .flatten()
    } else {
        None
    };
    let active_tracked_min: i64 = match active_task_id {
        Some(id) => tracked_minutes(conn, id).unwrap_or(0),
        None => 0,
    };
    let tracked_today_min: i64 = conn.query_row(
        "SELECT COALESCE(SUM(CAST((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440 AS INTEGER)),0)
         FROM segments WHERE date(start_at,'localtime')=date('now','localtime')",
        params![now()],
        |r| r.get(0),
    ).unwrap_or(0);

    let minutes_committed: i64 = conn.query_row(
        "SELECT COALESCE(SUM(estimate_min),0) FROM tasks
         WHERE status IN ('pending','paused','reopened','in_progress','awaiting') AND (plan_date=?1 OR plan_date IS NULL)",
        params![day], |r| r.get(0))?;

    let stop_time: Option<String> = conn
        .query_row("SELECT stop_time FROM day_plans WHERE date=?1", params![day], |r| r.get(0))
        .ok()
        .flatten();
    let minutes_left_in_day = minutes_until(conn, stop_time.as_deref().unwrap_or("18:00"));

    let planned_today: bool = conn
        .query_row("SELECT COUNT(*) FROM day_plans WHERE date=?1", params![day], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0)
        > 0;

    // Break state.
    let break_tid = break_task_id(conn);
    let on_break = matches!((active_task_id, break_tid), (Some(a), Some(b)) if a == b);
    let bset = get_break_settings(conn);
    let break_remaining_sec: i64 = if on_break {
        let elapsed: i64 = conn
            .query_row(
                "SELECT CAST((julianday(?1)-julianday(start_at))*86400 AS INTEGER)
                 FROM segments WHERE task_id=?2 AND end_at IS NULL",
                params![now(), break_tid.unwrap_or(-1)],
                |r| r.get(0),
            )
            .unwrap_or(0);
        bset.duration_min * 60 - elapsed
    } else {
        0
    };
    let anchor = get_setting(conn, "break_anchor").unwrap_or_else(now);
    let worked_since_break_min: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(CAST((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440 AS INTEGER)),0)
             FROM segments WHERE task_id<>?2 AND start_at>=?3",
            params![now(), break_tid.unwrap_or(-1), anchor],
            |r| r.get(0),
        )
        .unwrap_or(0);

    Ok(Snapshot {
        pending,
        in_progress,
        completed_today,
        active_task_id,
        active_task_title,
        active_awaiting,
        active_since_min,
        active_estimate_min,
        active_tracked_min,
        tracked_today_min,
        minutes_left_in_day,
        minutes_committed,
        greeting: greeting(conn),
        planned_today,
        worked_since_break_min,
        on_break,
        break_remaining_sec,
    })
}

/// Aggregate a single day, a rolling 7-day week, or a calendar month for the
/// dashboard, `offset` periods back from now (0 = current). This is what lets
/// the user browse their working history.
pub fn dashboard(conn: &Connection, period: &str, offset: i64) -> Result<Dashboard> {
    let n = now();
    let offset = offset.max(0);
    let period = match period {
        "week" => "week",
        "month" => "month",
        _ => "day",
    }
    .to_string();

    // Local date range [start_date, end_date] for the requested window.
    let (start_date, end_date): (String, String) = match period.as_str() {
        "month" => {
            let first: String = conn.query_row(
                "SELECT date('now','localtime','start of month',?1)",
                params![format!("-{offset} months")], |r| r.get(0))?;
            let last: String = conn.query_row(
                "SELECT date(?1,'+1 month','-1 day')", params![first], |r| r.get(0))?;
            (first, last)
        }
        "week" => {
            // Calendar week aligned to the user's week-start day (default Monday),
            // not a rolling 7 days. `back` = days from today to this week's start.
            let week_start = get_week_start(conn);
            let today_dow: i64 = conn.query_row(
                "SELECT CAST(strftime('%w','now','localtime') AS INTEGER)", [], |r| r.get(0))?;
            let back = (today_dow - week_start + 7) % 7;
            let start: String = conn.query_row(
                "SELECT date('now','localtime',?1)",
                params![format!("-{} days", back + offset * 7)], |r| r.get(0))?;
            let end: String = conn.query_row(
                "SELECT date(?1,'+6 days')", params![start], |r| r.get(0))?;
            (start, end)
        }
        _ => {
            let d: String = conn.query_row(
                "SELECT date('now','localtime',?1)",
                params![format!("-{offset} days")], |r| r.get(0))?;
            (d.clone(), d)
        }
    };

    // Date predicates over the window. The date strings are SQL-computed
    // 'YYYY-MM-DD' (no injection risk), so they're inlined into the fragments.
    let seg_s = format!("date(s.start_at,'localtime') BETWEEN '{start_date}' AND '{end_date}'");
    let seg_bare = format!("date(start_at,'localtime') BETWEEN '{start_date}' AND '{end_date}'");

    // Minutes of a (possibly open) span, capped at `now` for the open one.
    let span_min = "CAST(SUM((julianday(COALESCE({end},?1))-julianday({start}))*1440) AS INTEGER)";
    let seg_minutes = span_min.replace("{end}", "s.end_at").replace("{start}", "s.start_at");
    let bare_minutes = span_min.replace("{end}", "end_at").replace("{start}", "start_at");

    let total_tracked_min: i64 = conn.query_row(
        &format!("SELECT COALESCE({bare_minutes},0) FROM segments WHERE {seg_bare}"),
        params![n], |r| r.get(0))?;

    // Focus = time tracked against a task. Untracked (= distraction) = active
    // app time captured while NO task was running (idle already excluded by the
    // idle watcher capping the focus span). Our own window never counts.
    let not_us = "lower(COALESCE(app_id,'')) NOT LIKE '%achieve%'";
    let focus_min = total_tracked_min;
    let untracked_min: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE({bare_minutes},0) FROM focus_log
             WHERE {seg_bare} AND task_id IS NULL AND {not_us}"
        ),
        params![n],
        |r| r.get(0),
    )?;
    let distraction_min = untracked_min;

    let completed: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM tasks WHERE status='completed'
             AND date(updated_at,'localtime') BETWEEN '{start_date}' AND '{end_date}'"
        ),
        [], |r| r.get(0))?;
    // "total" = distinct tasks touched in the window (worked on or completed).
    let total_tasks: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM (
                SELECT id FROM tasks WHERE status='completed'
                  AND date(updated_at,'localtime') BETWEEN '{start_date}' AND '{end_date}'
                UNION
                SELECT DISTINCT task_id FROM segments WHERE {seg_bare}
             )"
        ),
        [], |r| r.get(0))?;

    // by category
    let mut by_category = Vec::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT c.name, c.color, {seg_minutes} AS mins
             FROM segments s JOIN tasks t ON t.id=s.task_id LEFT JOIN categories c ON c.id=t.category_id
             WHERE {seg_s}
             GROUP BY c.id HAVING mins > 0 ORDER BY mins DESC",
        ))?;
        let rows = stmt.query_map(params![n], |r| {
            Ok(CategoryStat {
                name: r.get::<_, Option<String>>(0)?.unwrap_or_else(|| "Uncategorized".into()),
                color: r.get::<_, Option<String>>(1)?.unwrap_or_else(|| "#9aa0aa".into()),
                minutes: r.get(2)?,
            })
        })?;
        for row in rows {
            by_category.push(row?);
        }
        if untracked_min > 0 {
            by_category.push(CategoryStat {
                name: "Untracked".into(),
                color: "#9aa0aa".into(),
                minutes: untracked_min,
            });
            by_category.sort_by(|a, b| b.minutes.cmp(&a.minutes));
        }
    }

    // Apps spent in, per task (and for the untracked bucket), from focus_log.
    let mut task_apps: std::collections::HashMap<i64, Vec<AppStat>> =
        std::collections::HashMap::new();
    let mut untracked_apps: Vec<AppStat> = Vec::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT task_id, app_id, {bare_minutes} AS mins
             FROM focus_log
             WHERE {seg_bare} AND app_id IS NOT NULL AND app_id <> '' AND {not_us}
             GROUP BY task_id, app_id HAVING mins > 0 ORDER BY mins DESC",
        ))?;
        let rows = stmt.query_map(params![n], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (tid, app, mins) = row?;
            let stat = AppStat { app, minutes: mins };
            match tid {
                Some(id) => task_apps.entry(id).or_default().push(stat),
                None => untracked_apps.push(stat),
            }
        }
    }

    // by app (automatic ground truth; our own window is excluded)
    let mut by_app = Vec::new();
    {
        let mut stmt = conn.prepare(&format!(
            "SELECT app_id, {bare_minutes} AS mins
             FROM focus_log
             WHERE {seg_bare} AND app_id IS NOT NULL AND app_id <> ''
                   AND lower(app_id) NOT LIKE '%achieve%'
             GROUP BY app_id HAVING mins > 0 ORDER BY mins DESC LIMIT 10",
        ))?;
        let rows = stmt.query_map(params![n], |r| {
            Ok(AppStat { app: r.get(0)?, minutes: r.get(1)? })
        })?;
        for row in rows {
            by_app.push(row?);
        }
    }

    // Tasks with their actual tracked time + the apps the time was spent in.
    // The synthetic "Untracked" row carries the no-task (distraction) time.
    let mut planned_actual: Vec<PlannedActual> = {
        // Every task worked on in the window, by time spent.
        let sql = format!(
            "SELECT t.id, t.title, COALESCE(c.color,'#9aa0aa'), COALESCE(t.estimate_min,0), t.status,
                    (SELECT COALESCE({seg_minutes},0) FROM segments s WHERE s.task_id=t.id AND {seg_s}) AS tracked
             FROM tasks t LEFT JOIN categories c ON c.id=t.category_id
             WHERE EXISTS (SELECT 1 FROM segments s WHERE s.task_id=t.id AND {seg_s})
             ORDER BY tracked DESC, t.id LIMIT 8",
        );
        let mut stmt = conn.prepare(&sql)?;
        let map = |r: &rusqlite::Row| {
            let status: String = r.get(4)?;
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                status == "completed",
                r.get::<_, i64>(5)?,
            ))
        };
        let rows: Vec<(i64, String, String, i64, bool, i64)> =
            stmt.query_map(params![n], map)?.collect::<std::result::Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, title, color, estimate_min, done, tracked_min)| {
                let mut apps = task_apps.remove(&id).unwrap_or_default();
                apps.sort_by(|a, b| b.minutes.cmp(&a.minutes));
                apps.truncate(6);
                PlannedActual {
                    title,
                    color,
                    estimate_min,
                    tracked_min,
                    done,
                    untracked: false,
                    apps,
                }
            })
            .collect()
    };
    if untracked_min > 0 {
        untracked_apps.sort_by(|a, b| b.minutes.cmp(&a.minutes));
        untracked_apps.truncate(6);
        planned_actual.push(PlannedActual {
            title: "Untracked".into(),
            color: "#9aa0aa".into(),
            estimate_min: 0,
            tracked_min: untracked_min,
            done: false,
            untracked: true,
            apps: untracked_apps,
        });
        planned_actual.sort_by(|a, b| b.tracked_min.cmp(&a.tracked_min));
    }

    // Pick the bucket's dominant label: the top category by tracked minutes,
    // unless untracked time outweighs it.
    let pick_top = |cat: Option<(String, String, i64)>, untr: i64| -> (String, String) {
        match cat {
            Some((name, color, mins)) if mins >= untr && mins > 0 => (name, color),
            _ if untr > 0 => ("Untracked".into(), "#9aa0aa".into()),
            _ => (String::new(), String::new()),
        }
    };

    // hero chart: focus (tracked) vs untracked per bucket -> per-hour (day) or
    // per-day (week).
    let mut bars = Vec::new();
    if period != "day" {
        // One bar per day across the window (week = 7, month = 28..31).
        let days: i64 = conn.query_row(
            "SELECT CAST(julianday(?2)-julianday(?1) AS INTEGER)+1",
            params![start_date, end_date], |r| r.get(0))?;
        for i in 0..days {
            let date: String = conn.query_row(
                "SELECT date(?1,?2)", params![start_date, format!("+{i} days")], |r| r.get(0))?;
            let focus: i64 = conn.query_row(
                "SELECT COALESCE(CAST(SUM((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440) AS INTEGER),0)
                 FROM segments WHERE date(start_at,'localtime')=?2",
                params![n, date], |r| r.get(0))?;
            let untr: i64 = conn.query_row(
                &format!(
                    "SELECT COALESCE(CAST(SUM((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440) AS INTEGER),0)
                     FROM focus_log WHERE date(start_at,'localtime')=?2 AND task_id IS NULL AND {not_us}"
                ),
                params![n, date], |r| r.get(0))?;
            let top_cat: Option<(String, String, i64)> = conn.query_row(
                "SELECT COALESCE(c.name,'Uncategorized'), COALESCE(c.color,'#9aa0aa'),
                        CAST(SUM((julianday(COALESCE(s.end_at,?1))-julianday(s.start_at))*1440) AS INTEGER) mins
                 FROM segments s JOIN tasks t ON t.id=s.task_id LEFT JOIN categories c ON c.id=t.category_id
                 WHERE date(s.start_at,'localtime')=?2
                 GROUP BY c.id ORDER BY mins DESC LIMIT 1",
                params![n, date], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok();
            let label = if period == "week" {
                let wd: i64 = conn.query_row(
                    "SELECT CAST(strftime('%w',?1) AS INTEGER)", params![date], |r| r.get(0))?;
                weekday_short(wd).to_string()
            } else {
                date.rsplit('-').next().unwrap_or("").trim_start_matches('0').to_string()
            };
            let (top, top_color) = pick_top(top_cat, untr);
            bars.push(Bar { label, focus_min: focus, untracked_min: untr, top, top_color });
        }
    } else {
        let hour_buckets = |sql: &str| -> Result<[i64; 24]> {
            let mut hours = [0i64; 24];
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map(params![n], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })?;
            for row in rows {
                let (hr, mins) = row?;
                if (0..24).contains(&hr) {
                    hours[hr as usize] = mins.max(0);
                }
            }
            Ok(hours)
        };
        let fhours = hour_buckets(&format!(
            "SELECT CAST(strftime('%H',start_at,'localtime') AS INTEGER) AS hr,
                    CAST(SUM((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440) AS INTEGER) AS mins
             FROM segments WHERE date(start_at,'localtime')='{start_date}'
             GROUP BY hr",
        ))?;
        let uhours = hour_buckets(&format!(
            "SELECT CAST(strftime('%H',start_at,'localtime') AS INTEGER) AS hr,
                    CAST(SUM((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440) AS INTEGER) AS mins
             FROM focus_log WHERE date(start_at,'localtime')='{start_date}'
               AND task_id IS NULL AND {not_us}
             GROUP BY hr",
        ))?;
        // Top category per hour (max tracked minutes in that hour).
        let mut top_by_hour: std::collections::HashMap<usize, (String, String, i64)> =
            std::collections::HashMap::new();
        {
            let mut stmt = conn.prepare(&format!(
                "SELECT CAST(strftime('%H',s.start_at,'localtime') AS INTEGER) hr,
                        COALESCE(c.name,'Uncategorized'), COALESCE(c.color,'#9aa0aa'),
                        CAST(SUM((julianday(COALESCE(s.end_at,?1))-julianday(s.start_at))*1440) AS INTEGER) mins
                 FROM segments s JOIN tasks t ON t.id=s.task_id LEFT JOIN categories c ON c.id=t.category_id
                 WHERE date(s.start_at,'localtime')='{start_date}'
                 GROUP BY hr, c.id",
            ))?;
            let rows = stmt.query_map(params![n], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
            })?;
            for row in rows {
                let (hr, name, color, mins) = row?;
                if (0..24).contains(&hr) {
                    let e = top_by_hour.entry(hr as usize).or_insert((String::new(), String::new(), 0));
                    if mins > e.2 {
                        *e = (name, color, mins);
                    }
                }
            }
        }
        // Window: 12am -> the day's stop ("Ends") time, extended to cover any
        // later activity. No stop set -> the whole day (12am..12am). Always
        // starts at midnight so the axis is a stable, full-day timeline.
        let active = |h: usize| fhours[h] + uhours[h] > 0;
        let stop_hhmm: Option<String> = conn
            .query_row("SELECT stop_time FROM day_plans WHERE date=?1", params![start_date], |r| r.get(0))
            .ok()
            .flatten();
        let stop_last_h: i64 = match stop_hhmm.as_deref().and_then(parse_hhmm_min) {
            Some(m) => ((m + 59) / 60 - 1).clamp(0, 23), // last hour bucket at/under the stop time
            None => 23,                                  // whole day
        };
        let last_active = (0..24).rev().find(|&h| active(h)).map(|h| h as i64).unwrap_or(0);
        let last = stop_last_h.max(last_active).clamp(0, 23) as usize;
        for h in 0..=last {
            let cat = top_by_hour.get(&h).filter(|c| c.2 > 0).cloned();
            let (top, top_color) = pick_top(cat, uhours[h]);
            bars.push(Bar {
                label: format!("{h}"),
                focus_min: fhours[h],
                untracked_min: uhours[h],
                top,
                top_color,
            });
        }
    }

    Ok(Dashboard {
        period,
        start_date,
        end_date,
        total_tracked_min,
        focus_min,
        distraction_min,
        completed,
        total_tasks,
        by_category,
        by_app,
        planned_actual,
        bars,
    })
}

fn weekday_short(w: i64) -> &'static str {
    // SQLite %w: 0 = Sunday .. 6 = Saturday
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        .get(w as usize)
        .copied()
        .unwrap_or("?")
}

/// Parse "HH:MM" into minutes from midnight. None if malformed/out of range.
fn parse_hhmm_min(hhmm: &str) -> Option<i64> {
    let mut p = hhmm.split(':');
    let h: i64 = p.next()?.trim().parse().ok()?;
    let m: i64 = p.next().unwrap_or("0").trim().parse().ok()?;
    if (0..=23).contains(&h) && (0..=59).contains(&m) {
        Some(h * 60 + m)
    } else {
        None
    }
}

fn minutes_until(conn: &Connection, hhmm: &str) -> i64 {
    let parts: Vec<&str> = hhmm.split(':').collect();
    let (h, m): (i64, i64) = match (parts.first(), parts.get(1)) {
        (Some(h), Some(m)) => (h.parse().unwrap_or(18), m.parse().unwrap_or(0)),
        _ => (18, 0),
    };
    let target = h * 60 + m;
    (target - local_now_min(conn)).max(0)
}

fn greeting(conn: &Connection) -> String {
    let h = local_hour(conn);
    match h {
        5..=11 => "Good morning. Let's set the day.",
        12..=16 => "Afternoon. Keep the momentum.",
        17..=21 => "Evening. Let's land the day well.",
        _ => "Still up? Let's keep it gentle.",
    }
    .to_string()
}
