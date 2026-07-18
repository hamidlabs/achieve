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
    AppStat, Bar, BreakSettings, CategoryStat, Dashboard, DayPlan, FocusSpan, PauseStat,
    PlannedActual, Snapshot, Task, TimelineSpan,
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
    // Presence ledger: record the gap since we were last running as away time,
    // then backfill/seal every completed day so Focus + Untracked + Away tile
    // the whole day as real rows in the database (never computed at read time).
    record_downtime(&conn)?;
    reattribute_all_history(&conn)?;
    seal_past_days(&conn)?;
    reconcile_today(&conn)?;
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

        -- Append-only log of time the user was AWAY from the machine (idle), a
        -- third presence bucket disjoint from segments (tracked) and focus_log
        -- (untracked), so the three never double-count.
        CREATE TABLE IF NOT EXISTS away_log (
            id       INTEGER PRIMARY KEY,
            start_at TEXT NOT NULL,
            end_at   TEXT
        );

        -- Task reminders. `remind_at` is stored UTC (like every other timestamp)
        -- so it compares directly with now(); recurrence is advanced in LOCAL
        -- wall-clock so "9am daily" stays 9am across DST. One row per reminder:
        -- when a recurring one fires it is advanced in place to the next slot.
        CREATE TABLE IF NOT EXISTS reminders (
            id          INTEGER PRIMARY KEY,
            task_id     INTEGER NOT NULL REFERENCES tasks(id),
            remind_at   TEXT NOT NULL,        -- next fire time, UTC 'YYYY-MM-DD HH:MM:SS'
            rrule       TEXT,                 -- NULL=one-shot | daily|weekdays|weekly|biweekly|monthly|yearly | every:N:days|weeks|months
            rrule_until TEXT,                 -- inclusive end bound, UTC datetime; NULL=no end
            rrule_count INTEGER,              -- remaining fires incl. the next one; NULL=unbounded
            channel     TEXT NOT NULL DEFAULT 'both', -- email | notification | both
            note        TEXT,                 -- optional custom line in the reminder
            status      TEXT NOT NULL DEFAULT 'pending', -- pending|scheduled|sent|cancelled|failed
            message_id  TEXT,                 -- worker/Brevo messageId when scheduled (for cancel)
            last_error  TEXT,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_reminders_task ON reminders(task_id);
        CREATE INDEX IF NOT EXISTS idx_reminders_due ON reminders(status, remind_at);

        -- Free-form notes attached to a task. Each note is an independent,
        -- separately-editable markdown entry stamped with when it was written, so
        -- a task accrues a dated journal you can add to at any moment and search
        -- back through later. created_at/updated_at are UTC like every timestamp.
        CREATE TABLE IF NOT EXISTS notes (
            id         INTEGER PRIMARY KEY,
            task_id    INTEGER NOT NULL REFERENCES tasks(id),
            body_md    TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_notes_task ON notes(task_id);
        CREATE INDEX IF NOT EXISTS idx_notes_created ON notes(created_at);
        "#,
    )?;

    // Migration: attribute each automatic focus span to the task that was
    // active when it was captured, so the dashboard can show exact per-task app
    // usage. Older DBs lack the column; add it if missing.
    if !column_exists(conn, "focus_log", "task_id")? {
        conn.execute("ALTER TABLE focus_log ADD COLUMN task_id INTEGER", [])?;
    }
    // Migration: record WHY a span of away time exists (idle at the desk,
    // suspend, or machine-off/app-not-running), so the away bucket is a real,
    // inspectable ledger rather than a number computed at read time.
    if !column_exists(conn, "away_log", "reason")? {
        conn.execute("ALTER TABLE away_log ADD COLUMN reason TEXT", [])?;
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
    // An away span left open by an unclean shutdown: we can't know when the user
    // returned, so drop it (end = start) rather than invent phantom away time.
    conn.execute("UPDATE away_log SET end_at = start_at WHERE end_at IS NULL", [])?;
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
    // Record the away span itself, starting at the moment input actually stopped
    // (now - idle_secs), so "time away from the machine" shows up as its own
    // bucket instead of vanishing between the capped segments.
    let away_start: String = conn.query_row(
        "SELECT datetime('now', ?1)",
        params![cutoff],
        |r| r.get(0),
    )?;
    open_away(conn, &away_start, "idle")?;
    Ok(closed_seg > 0 || closed_focus > 0)
}

/// Open an away span starting at `start_at` (with a reason: "idle" | "suspend"),
/// unless one is already open (idle is detected once per episode, but this stays
/// idempotent).
pub fn open_away(conn: &Connection, start_at: &str, reason: &str) -> Result<()> {
    let already_open: bool = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM away_log WHERE end_at IS NULL)", [], |r| r.get(0))?;
    if !already_open {
        conn.execute(
            "INSERT INTO away_log (start_at, end_at, reason) VALUES (?1, NULL, ?2)",
            params![start_at, reason],
        )?;
    }
    Ok(())
}

/// Close the open away span at `end_at`, never before it started.
pub fn close_open_away(conn: &Connection, end_at: &str) -> Result<()> {
    conn.execute(
        "UPDATE away_log
            SET end_at = CASE WHEN start_at > ?1 THEN start_at ELSE ?1 END
          WHERE end_at IS NULL",
        params![end_at],
    )?;
    Ok(())
}

/// Total away minutes whose span falls in the local-date window [start,end],
/// the open span capped at `now`.
fn away_minutes(conn: &Connection, start_date: &str, end_date: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(CAST((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440 AS INTEGER)),0)
         FROM away_log
         WHERE date(start_at,'localtime') BETWEEN ?2 AND ?3",
        params![now(), start_date, end_date],
        |r| r.get(0),
    )?)
}

// ---------------------------------------------------------------------------
// Presence materialization.
//
// The day is partitioned into three presence buckets: Focus (`segments`),
// Untracked (`focus_log` with no task), and Away (`away_log`). Away is never a
// number computed at read time; every stretch the user was NOT actively at the
// machine (idle at the desk, the machine suspended, or the app not running at
// all) is written as a real `away_log` row. `seal_day` recomputes a finished
// day's away rows as the exact complement of its present time, so the three
// tables tile the whole day in the database and can be inspected and trusted.
// ---------------------------------------------------------------------------

/// Half-open interval `[start, end)` in unix epoch seconds.
type Iv = (i64, i64);

/// Merge overlapping/adjacent intervals into a disjoint, sorted set.
fn merge_iv(mut v: Vec<Iv>) -> Vec<Iv> {
    v.retain(|(a, b)| b > a);
    v.sort_unstable();
    let mut out: Vec<Iv> = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// `whole` minus every interval in `holes` (both merged internally).
fn subtract_iv(whole: Vec<Iv>, holes: &[Iv]) -> Vec<Iv> {
    let holes = merge_iv(holes.to_vec());
    let mut out = Vec::new();
    for (mut s, e) in merge_iv(whole) {
        for &(hs, he) in &holes {
            if he <= s || hs >= e {
                continue;
            }
            if hs > s {
                out.push((s, hs));
            }
            s = s.max(he);
            if s >= e {
                break;
            }
        }
        if s < e {
            out.push((s, e));
        }
    }
    out
}

/// The `[start_at, end_at)` epoch-second intervals of `table`'s rows overlapping
/// the window `[w0, w1)`, clipped to it. Rows still open (end NULL) are capped at
/// `w1` (the window edge, e.g. `now` for today).
fn load_iv(conn: &Connection, table: &str, w0: i64, w1: i64) -> Result<Vec<Iv>> {
    let sql = format!(
        "SELECT CAST(strftime('%s', start_at) AS INTEGER) AS s,
                CAST(strftime('%s', COALESCE(end_at, datetime(?2,'unixepoch'))) AS INTEGER) AS e
         FROM {table}
         WHERE CAST(strftime('%s', start_at) AS INTEGER) < ?2
           AND CAST(strftime('%s', COALESCE(end_at, datetime(?2,'unixepoch'))) AS INTEGER) > ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![w0, w1], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (s, e) = row?;
        out.push((s.max(w0), e.min(w1)));
    }
    Ok(out)
}

/// Unix epoch (seconds) of local midnight starting the given `YYYY-MM-DD` date.
fn local_midnight_epoch(conn: &Connection, date: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT CAST(strftime('%s', ?1 || ' 00:00:00', 'utc') AS INTEGER)",
        params![date],
        |r| r.get(0),
    )?)
}

fn now_epoch(conn: &Connection) -> i64 {
    conn.query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| r.get(0))
        .unwrap_or(0)
}

/// A UTC epoch second back to the stored "YYYY-MM-DD HH:MM:SS" (UTC) format.
fn epoch_to_utc(conn: &Connection, e: i64) -> Result<String> {
    Ok(conn.query_row(
        "SELECT strftime('%Y-%m-%d %H:%M:%S', ?1, 'unixepoch')",
        params![e],
        |r| r.get(0),
    )?)
}

/// Insert away rows for `[s, e)` (epoch seconds), splitting at local midnights so
/// each row belongs to a single local day (the dashboard aggregates by
/// `date(start_at)`). Sub-minute fragments are dropped.
fn insert_away_split(conn: &Connection, mut s: i64, e: i64, reason: &str) -> Result<()> {
    while s < e {
        let next_mid: i64 = conn.query_row(
            "SELECT CAST(strftime('%s', date(?1,'unixepoch','localtime','+1 day') || ' 00:00:00', 'utc') AS INTEGER)",
            params![s],
            |r| r.get(0),
        )?;
        let seg_end = e.min(next_mid);
        if seg_end - s >= 60 {
            let ss = epoch_to_utc(conn, s)?;
            let ee = epoch_to_utc(conn, seg_end)?;
            conn.execute(
                "INSERT INTO away_log (start_at, end_at, reason) VALUES (?1, ?2, ?3)",
                params![ss, ee, reason],
            )?;
        }
        if seg_end <= s {
            break; // guard against a non-advancing step
        }
        s = seg_end;
    }
    Ok(())
}

/// Heartbeat: remember that the app was alive right now. On the next launch the
/// gap between this and `now` is exactly the time the machine was off / the app
/// was not running, which `record_downtime` books as away.
pub fn touch_seen(conn: &Connection) -> Result<()> {
    set_setting(conn, "last_seen", &now())
}

/// On launch, book the gap since the last heartbeat as away time (machine off /
/// app not running), then re-arm the heartbeat.
fn record_downtime(conn: &Connection) -> Result<()> {
    let now_e = now_epoch(conn);
    if let Some(ls) = get_setting(conn, "last_seen") {
        let last_e: i64 = conn
            .query_row("SELECT CAST(strftime('%s', ?1) AS INTEGER)", params![ls], |r| r.get(0))
            .unwrap_or(0);
        // Only a real absence (>= 1 min) counts; ignore a quick restart.
        if last_e > 0 && now_e - last_e >= 60 {
            insert_away_split(conn, last_e, now_e, "offline")?;
        }
    }
    set_setting(conn, "last_seen", &now())
}

/// Materialize a finished local day's away rows as the exact complement of its
/// present time. Present = segments (Focus) ∪ focus_log (active at the machine);
/// everything else in the 24h is away. Replaces the day's existing away rows so
/// idle, suspend and offline gaps merge into one clean, non-overlapping ledger.
pub fn seal_day(conn: &Connection, date: &str) -> Result<()> {
    let w0 = local_midnight_epoch(conn, date)?;
    seal_window(conn, date, w0, w0 + 86_400)
}

/// Recompute the away rows for `date` inside the window `[w0, w1)` (epoch secs) as
/// the exact complement of present time (segments ∪ focus_log). Replaces the day's
/// existing away rows so idle, suspend and offline gaps become one clean,
/// non-overlapping ledger that can never overlap Focus/Untracked.
fn seal_window(conn: &Connection, date: &str, w0: i64, w1: i64) -> Result<()> {
    if w1 <= w0 {
        return Ok(());
    }
    let mut present = load_iv(conn, "segments", w0, w1)?;
    present.extend(load_iv(conn, "focus_log", w0, w1)?);
    let present = merge_iv(present);
    let away = subtract_iv(vec![(w0, w1)], &present);
    conn.execute("DELETE FROM away_log WHERE date(start_at,'localtime')=?1", params![date])?;
    for (s, e) in away {
        if e - s < 60 {
            continue; // ignore sub-minute gaps (focus-switch blips)
        }
        let ss = epoch_to_utc(conn, s)?;
        let ee = epoch_to_utc(conn, e)?;
        conn.execute(
            "INSERT INTO away_log (start_at, end_at, reason) VALUES (?1, ?2, 'offline')",
            params![ss, ee],
        )?;
    }
    Ok(())
}

/// Keep TODAY's away ledger clean and current: materialize away as the complement
/// of present time from local midnight up to `now`. Input-idle can leave an away
/// span overlapping a focus change that fired without keyboard/mouse; this makes
/// the live day's three buckets tile without overlap, exactly like a sealed past
/// day. Cheap (a handful of rows); the engine runs it each tick.
pub fn reconcile_today(conn: &Connection) -> Result<()> {
    let date = today(conn);
    reattribute_focus(conn, &date)?;
    let w0 = local_midnight_epoch(conn, &date)?;
    seal_window(conn, &date, w0, now_epoch(conn))
}

/// Insert a focus_log row from epoch-second bounds (task_id optional).
fn insert_focus(
    conn: &Connection,
    app: &Option<String>,
    title: &Option<String>,
    s: i64,
    e: i64,
    task: Option<i64>,
) -> Result<()> {
    if e - s < 1 {
        return Ok(());
    }
    let ss = epoch_to_utc(conn, s)?;
    let ee = epoch_to_utc(conn, e)?;
    conn.execute(
        "INSERT INTO focus_log (app_id, title, start_at, end_at, task_id) VALUES (?1,?2,?3,?4,?5)",
        params![app, title, ss, ee, task],
    )?;
    Ok(())
}

/// Re-attribute CLOSED untracked focus spans that overlap a work segment to that
/// segment's task, so Focus (segments) and Untracked (null focus spans) never
/// cover the same instant. A focus span captured while a task was tracking was
/// really time on that task (its window just hadn't been re-stamped yet). The
/// currently-open span is left alone (handled by `attribute_open_focus` on start).
fn reattribute_focus(conn: &Connection, date: &str) -> Result<()> {
    let now_utc = now();
    let mut segs: Vec<(i64, i64, i64)> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT CAST(strftime('%s',start_at) AS INTEGER),
                    CAST(strftime('%s',COALESCE(end_at,?2)) AS INTEGER), task_id
             FROM segments WHERE date(start_at,'localtime')=?1",
        )?;
        let rows = stmt.query_map(params![date, now_utc], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        for row in rows {
            let (s, e, t): (i64, i64, i64) = row?;
            if e > s {
                segs.push((s, e, t));
            }
        }
    }
    if segs.is_empty() {
        return Ok(());
    }
    segs.sort_by_key(|x| x.0);
    let mut nulls: Vec<(i64, Option<String>, Option<String>, i64, i64)> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, app_id, title,
                    CAST(strftime('%s',start_at) AS INTEGER),
                    CAST(strftime('%s',end_at) AS INTEGER)
             FROM focus_log
             WHERE date(start_at,'localtime')=?1 AND task_id IS NULL AND end_at IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![date], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        for row in rows {
            nulls.push(row?);
        }
    }
    for (id, app, title, s, e) in nulls {
        let hits: Vec<(i64, i64, i64)> =
            segs.iter().copied().filter(|(ss, se, _)| *se > s && *ss < e).collect();
        if hits.is_empty() {
            continue;
        }
        conn.execute("DELETE FROM focus_log WHERE id=?1", params![id])?;
        let mut cursor = s;
        for (ss, se, task) in hits {
            let os = ss.max(s);
            let oe = se.min(e);
            if os > cursor {
                insert_focus(conn, &app, &title, cursor, os, None)?; // gap before segment
            }
            insert_focus(conn, &app, &title, os.max(cursor), oe, Some(task))?; // tracked part
            cursor = oe.max(cursor);
        }
        if cursor < e {
            insert_focus(conn, &app, &title, cursor, e, None)?; // tail after last segment
        }
    }
    Ok(())
}

/// One-time: re-attribute overlapping focus spans across ALL past days so their
/// dashboards never show tracked and untracked colliding either.
fn reattribute_all_history(conn: &Connection) -> Result<()> {
    if get_setting(conn, "focus_reattributed_v1").is_some() {
        return Ok(());
    }
    let mut days: Vec<String> = Vec::new();
    {
        let mut stmt =
            conn.prepare("SELECT DISTINCT date(start_at,'localtime') FROM focus_log ORDER BY 1")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for d in rows {
            days.push(d?);
        }
    }
    for d in days {
        reattribute_focus(conn, &d)?;
    }
    set_setting(conn, "focus_reattributed_v1", &now())?;
    Ok(())
}

/// Seal (materialize a complete away ledger for) every completed local day that
/// isn't sealed yet, up to and including yesterday. First run backfills all of
/// history; later runs only seal newly-finished days. Today is left live.
pub fn seal_past_days(conn: &Connection) -> Result<()> {
    let today = today(conn);
    let yday: String = conn.query_row("SELECT date(?1,'-1 day')", params![today], |r| r.get(0))?;
    let start_from = match get_setting(conn, "sealed_through") {
        Some(d) => conn
            .query_row("SELECT date(?1,'+1 day')", params![d], |r| r.get(0))
            .unwrap_or(d),
        None => {
            let earliest: Option<String> = conn
                .query_row(
                    "SELECT MIN(d) FROM (
                        SELECT MIN(date(start_at,'localtime')) d FROM segments
                        UNION ALL SELECT MIN(date(start_at,'localtime')) FROM focus_log
                        UNION ALL SELECT MIN(date(start_at,'localtime')) FROM away_log)",
                    [],
                    |r| r.get(0),
                )
                .ok()
                .flatten();
            match earliest {
                Some(d) => d,
                None => return Ok(()), // no data yet
            }
        }
    };
    if start_from > yday {
        return Ok(()); // nothing new to seal
    }
    let mut d = start_from;
    loop {
        seal_day(conn, &d)?;
        if d == yday {
            break;
        }
        d = conn.query_row("SELECT date(?1,'+1 day')", params![d], |r| r.get(0))?;
    }
    set_setting(conn, "sealed_through", &yday)?;
    Ok(())
}

/// Day boundary: tracking must not bleed across midnight. Cap any segment/focus
/// span still open from a previous local day at the midnight that ended that day
/// (so yesterday's time stays in yesterday), and pause any task left
/// in_progress/awaiting from a past day, rolling its plan_date to today so it
/// stops accruing and reappears on today's list to resume. Runs at the day
/// change and at startup (covers the app being launched after midnight).
pub fn rollover_day(conn: &Connection) -> Result<bool> {
    let day = today(conn);
    let cap_to = "datetime(date(start_at,'localtime','+1 day'),'utc')";
    let capped = conn.execute(
        &format!(
            "UPDATE segments
                SET end_at = {cap_to},
                    reason = CASE WHEN COALESCE(reason,'')='' THEN 'day-rollover' ELSE reason END
              WHERE end_at IS NULL AND date(start_at,'localtime') < ?1"
        ),
        params![day],
    )?;
    let _ = conn.execute(
        &format!(
            "UPDATE focus_log SET end_at = {cap_to}
              WHERE end_at IS NULL AND date(start_at,'localtime') < ?1"
        ),
        params![day],
    )?;
    let _ = conn.execute(
        &format!(
            "UPDATE away_log SET end_at = {cap_to}
              WHERE end_at IS NULL AND date(start_at,'localtime') < ?1"
        ),
        params![day],
    )?;
    // Keep the break work-clock anchor from drifting days into the past. The
    // continuous-streak calc already resets across the overnight gap, but this
    // keeps the stored value sane.
    set_setting(conn, "break_anchor", &now())?;
    let rolled = conn.execute(
        "UPDATE tasks SET status='paused', plan_date=?1, updated_at=?2
          WHERE status IN ('in_progress','awaiting') AND plan_date IS NOT NULL AND plan_date < ?1",
        params![day, now()],
    )?;
    Ok(capped > 0 || rolled > 0)
}

/// Roll daily recurring tasks into today: if a 'daily' task is from a previous
/// day (or unscheduled), reset it to pending for today. One row per recurring
/// task, reappearing each day; per-day time lives in segments.
fn ensure_recurring(conn: &Connection) -> Result<()> {
    let day = today(conn);
    conn.execute(
        "UPDATE tasks SET status = 'pending', plan_date = ?1, updated_at = ?2
         WHERE recurrence = 'daily' AND status <> 'deleted' AND (plan_date IS NULL OR plan_date < ?1)",
        params![day, now()],
    )?;
    Ok(())
}

pub fn now() -> String {
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
// "today"/"tomorrow" and all wall-clock math go through SQLite's localtime,
// which uses the OS timezone reliably. chrono::Local mis-resolved the timezone
// inside the AppImage, so we do NOT use it for dates.
pub fn today(conn: &Connection) -> String {
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

/// Total minutes tracked against a task. `today_only` restricts the sum to
/// segments that STARTED today (local) so a recurring daily task, which reuses
/// the same row every day, shows its progress against the DAILY estimate and
/// resets at local midnight instead of accumulating forever.
fn tracked_minutes(conn: &Connection, task_id: i64, today_only: bool) -> Result<i64> {
    let day_pred = if today_only {
        " AND date(start_at,'localtime')=date('now','localtime')"
    } else {
        ""
    };
    let secs: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(CAST((julianday(COALESCE(end_at, ?1)) - julianday(start_at)) * 86400 AS INTEGER)), 0)
             FROM segments WHERE task_id = ?2{day_pred}"
        ),
        params![now(), task_id],
        |r| r.get(0),
    )?;
    Ok(secs / 60)
}

/// True when the task is a recurring daily task (its estimate is a per-day
/// target, so tracked time is scoped to today).
fn is_daily(conn: &Connection, task_id: i64) -> bool {
    conn.query_row(
        "SELECT recurrence='daily' FROM tasks WHERE id=?1",
        params![task_id],
        |r| r.get(0),
    )
    .unwrap_or(false)
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

/// Rename and/or recolor a category. Tasks reference it by id, so they follow
/// the change automatically.
pub fn update_category(conn: &Connection, id: i64, name: &str, color: &str) -> Result<()> {
    conn.execute(
        "UPDATE categories SET name = ?2, color = ?3 WHERE id = ?1",
        params![id, name, color],
    )?;
    Ok(())
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
          AND t.status <> 'break' AND t.status <> 'deleted' AND t.id <> COALESCE(?2, -1)
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
        let tracked_min = tracked_minutes(conn, id, rec.as_deref() == Some("daily"))?;
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
        let tracked_min = tracked_minutes(conn, id, rec.as_deref() == Some("daily"))?;
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
    // Stop this task's tracking if it was running.
    conn.execute(
        "UPDATE segments SET end_at = ?1, reason = COALESCE(reason, 'deleted')
          WHERE task_id = ?2 AND end_at IS NULL",
        params![now(), id],
    )?;
    // The time ledger is append-only: if this task has ANY tracked history, keep
    // the row + its segments and just soft-delete it (hidden from every list but
    // still counted in the dashboard/history). Only a task that was never
    // tracked is hard-deleted, since there's nothing to preserve.
    let has_history: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM segments WHERE task_id = ?1)",
        params![id],
        |r| r.get(0),
    )?;
    // Cancel this task's live reminders. We only flip status here (cheap, under
    // the lock); the engine's reminder pass calls the worker to cancel any that
    // were already scheduled (they still carry a message_id), then leaves them.
    conn.execute(
        "UPDATE reminders SET status='cancelled', updated_at=?1
          WHERE task_id=?2 AND status IN ('pending','scheduled','failed')",
        params![now(), id],
    )?;
    if has_history {
        conn.execute(
            "UPDATE tasks SET status = 'deleted', plan_date = NULL, recurrence = NULL, updated_at = ?1
              WHERE id = ?2",
            params![now(), id],
        )?;
    } else {
        conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reminders
// ---------------------------------------------------------------------------

/// Map a reminders row (in local-aware column order) to the model.
fn reminder_from_row(r: &rusqlite::Row) -> rusqlite::Result<crate::model::Reminder> {
    Ok(crate::model::Reminder {
        id: r.get(0)?,
        task_id: r.get(1)?,
        remind_at_local: r.get(2)?,
        remind_at: r.get(3)?,
        rrule: r.get(4)?,
        rrule_until: r.get(5)?,
        rrule_count: r.get(6)?,
        channel: r.get(7)?,
        note: r.get(8)?,
        status: r.get(9)?,
    })
}

const REMINDER_COLS: &str = "id, task_id,
     strftime('%Y-%m-%d %H:%M', remind_at, 'localtime') AS remind_at_local,
     remind_at, rrule,
     CASE WHEN rrule_until IS NULL THEN NULL ELSE strftime('%Y-%m-%d', rrule_until, 'localtime') END AS rrule_until_local,
     rrule_count, channel, note, status";

/// Reminders for one task (newest fire first excluded; soonest first), hiding
/// cancelled ones.
pub fn list_reminders(conn: &Connection, task_id: i64) -> Result<Vec<crate::model::Reminder>> {
    let sql = format!(
        "SELECT {REMINDER_COLS} FROM reminders
          WHERE task_id=?1 AND status != 'cancelled'
          ORDER BY CASE status WHEN 'sent' THEN 1 ELSE 0 END, remind_at"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![task_id], reminder_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Convert a local "YYYY-MM-DD HH:MM" (optionally with seconds) to UTC storage
/// form. Returns None if SQLite can't parse it.
fn local_to_utc(conn: &Connection, local_dt: &str) -> Option<String> {
    conn.query_row("SELECT datetime(?1, 'utc')", params![local_dt], |r| r.get(0))
        .ok()
}

/// Create a reminder. `remind_at_local` is "YYYY-MM-DD HH:MM" in local time;
/// `until_local` (if any) is an inclusive local end date "YYYY-MM-DD".
pub fn create_reminder(
    conn: &Connection,
    task_id: i64,
    remind_at_local: &str,
    rrule: Option<&str>,
    until_local: Option<&str>,
    count: Option<i64>,
    channel: &str,
    note: Option<&str>,
) -> Result<i64> {
    let remind_at = local_to_utc(conn, remind_at_local)
        .ok_or_else(|| rusqlite::Error::InvalidParameterName("bad remind_at".into()))?;
    let until_utc = match until_local {
        Some(d) if !d.trim().is_empty() => local_to_utc(conn, &format!("{d} 23:59:59")),
        _ => None,
    };
    conn.execute(
        "INSERT INTO reminders
           (task_id, remind_at, rrule, rrule_until, rrule_count, channel, note, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?8)",
        params![task_id, remind_at, rrule, until_utc, count, channel, note, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update a reminder's schedule/content and reset it to pending so the engine
/// re-evaluates it. The caller cancels any prior worker schedule first.
pub fn update_reminder(
    conn: &Connection,
    id: i64,
    remind_at_local: &str,
    rrule: Option<&str>,
    until_local: Option<&str>,
    count: Option<i64>,
    channel: &str,
    note: Option<&str>,
) -> Result<()> {
    let remind_at = local_to_utc(conn, remind_at_local)
        .ok_or_else(|| rusqlite::Error::InvalidParameterName("bad remind_at".into()))?;
    let until_utc = match until_local {
        Some(d) if !d.trim().is_empty() => local_to_utc(conn, &format!("{d} 23:59:59")),
        _ => None,
    };
    conn.execute(
        "UPDATE reminders SET remind_at=?1, rrule=?2, rrule_until=?3, rrule_count=?4,
             channel=?5, note=?6, status='pending', message_id=NULL, last_error=NULL, updated_at=?7
          WHERE id=?8",
        params![remind_at, rrule, until_utc, count, channel, note, now(), id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Notes (per-task journal + global history/search)
// ---------------------------------------------------------------------------

const NOTE_COLS: &str = "notes.id, notes.task_id, tasks.title AS task_title,
     categories.color AS category_color, notes.body_md,
     strftime('%Y-%m-%d %H:%M', notes.created_at, 'localtime') AS created_local,
     strftime('%Y-%m-%d %H:%M', notes.updated_at, 'localtime') AS updated_local,
     notes.created_at";

fn note_from_row(r: &rusqlite::Row) -> rusqlite::Result<crate::model::Note> {
    Ok(crate::model::Note {
        id: r.get(0)?,
        task_id: r.get(1)?,
        task_title: r.get(2)?,
        category_color: r.get(3)?,
        body_md: r.get(4)?,
        created_local: r.get(5)?,
        updated_local: r.get(6)?,
        created_at: r.get(7)?,
    })
}

/// All notes for one task, newest first.
pub fn list_notes(conn: &Connection, task_id: i64) -> Result<Vec<crate::model::Note>> {
    let sql = format!(
        "SELECT {NOTE_COLS} FROM notes
           JOIN tasks ON tasks.id = notes.task_id
           LEFT JOIN categories ON categories.id = tasks.category_id
          WHERE notes.task_id = ?1
          ORDER BY notes.created_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![task_id], note_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Search notes across every task by note text OR task title. Empty query
/// returns the most recent notes (the default history view). Newest first.
pub fn search_notes(conn: &Connection, query: &str, limit: i64) -> Result<Vec<crate::model::Note>> {
    let q = query.trim();
    let lim = limit.clamp(1, 500);
    if q.is_empty() {
        let sql = format!(
            "SELECT {NOTE_COLS} FROM notes
               JOIN tasks ON tasks.id = notes.task_id
               LEFT JOIN categories ON categories.id = tasks.category_id
              ORDER BY notes.created_at DESC LIMIT ?1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![lim], note_from_row)?;
        return Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    let like = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
    let sql = format!(
        "SELECT {NOTE_COLS} FROM notes
           JOIN tasks ON tasks.id = notes.task_id
           LEFT JOIN categories ON categories.id = tasks.category_id
          WHERE notes.body_md LIKE ?1 ESCAPE '\\' OR tasks.title LIKE ?1 ESCAPE '\\'
          ORDER BY notes.created_at DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![like, lim], note_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Add a note to a task; returns the new note id.
pub fn create_note(conn: &Connection, task_id: i64, body_md: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO notes (task_id, body_md, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![task_id, body_md, now()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Edit a note's body.
pub fn update_note(conn: &Connection, id: i64, body_md: &str) -> Result<()> {
    conn.execute(
        "UPDATE notes SET body_md=?1, updated_at=?2 WHERE id=?3",
        params![body_md, now(), id],
    )?;
    Ok(())
}

/// Delete a note.
pub fn delete_note(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM notes WHERE id=?1", params![id])?;
    Ok(())
}

/// The messageId of a reminder if it currently has a live worker schedule, so
/// the caller can cancel it before editing/deleting. None if not scheduled.
pub fn reminder_message_id(conn: &Connection, id: i64) -> Option<String> {
    conn.query_row(
        "SELECT message_id FROM reminders WHERE id=?1 AND status='scheduled'",
        params![id],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .filter(|s| !s.is_empty())
}

/// Mark a reminder cancelled (terminal). The worker schedule, if any, is
/// cancelled separately by the command/engine.
pub fn cancel_reminder(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET status='cancelled', message_id=NULL, updated_at=?1 WHERE id=?2",
        params![now(), id],
    )?;
    Ok(())
}

// --- reminder dispatch (engine side) ---------------------------------------

/// A reminder plus the task context needed to render and decide on it. Times are
/// UTC (source of truth) with a local copy for the message body.
#[derive(Debug, Clone)]
#[allow(dead_code)] // task_id/plan_date carried for completeness/logging
pub struct ReminderJob {
    pub id: i64,
    pub task_id: i64,
    pub task_title: String,
    pub task_status: String,
    pub task_recurrence: Option<String>,
    pub category_name: Option<String>,
    pub category_color: Option<String>,
    pub plan_date: Option<String>,
    pub remind_at: String,
    pub remind_at_local: String,
    pub rrule: Option<String>,
    pub rrule_until: Option<String>,
    pub rrule_count: Option<i64>,
    pub channel: String,
    pub note: Option<String>,
    pub status: String,
    pub message_id: Option<String>,
}

/// Reminders needing action this tick: pending/failed within the 72h scheduling
/// horizon, scheduled ones whose time has passed, and cancelled ones that still
/// hold a worker schedule to tear down. Small set; safe to scan each pass.
pub fn due_reminder_jobs(conn: &Connection) -> Result<Vec<ReminderJob>> {
    let sql = "SELECT r.id, r.task_id, t.title, t.status, t.recurrence,
                      c.name, c.color, t.plan_date,
                      r.remind_at,
                      strftime('%Y-%m-%d %H:%M', r.remind_at, 'localtime'),
                      r.rrule,
                      r.rrule_until,
                      r.rrule_count, r.channel, r.note, r.status, r.message_id
               FROM reminders r
               JOIN tasks t ON t.id = r.task_id
               LEFT JOIN categories c ON c.id = t.category_id
               WHERE (r.status IN ('pending','failed') AND r.remind_at <= datetime('now','+72 hours'))
                  OR (r.status = 'scheduled' AND r.remind_at <= datetime('now'))
                  OR (r.status = 'cancelled' AND r.message_id IS NOT NULL)
               ORDER BY r.remind_at";
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| {
        Ok(ReminderJob {
            id: r.get(0)?,
            task_id: r.get(1)?,
            task_title: r.get(2)?,
            task_status: r.get(3)?,
            task_recurrence: r.get(4)?,
            category_name: r.get(5)?,
            category_color: r.get(6)?,
            plan_date: r.get(7)?,
            remind_at: r.get(8)?,
            remind_at_local: r.get(9)?,
            rrule: r.get(10)?,
            rrule_until: r.get(11)?,
            rrule_count: r.get(12)?,
            channel: r.get(13)?,
            note: r.get(14)?,
            status: r.get(15)?,
            message_id: r.get(16)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The ISO 8601 (UTC, ...Z) form of a stored UTC datetime, for `scheduledAt`.
pub fn to_iso8601(conn: &Connection, utc_dt: &str) -> Option<String> {
    conn.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S.000Z', ?1)",
        params![utc_dt],
        |r| r.get(0),
    )
    .ok()
}

fn is_weekend_local(conn: &Connection, utc_dt: &str) -> bool {
    let w: i64 = conn
        .query_row("SELECT CAST(strftime('%w', ?1, 'localtime') AS INTEGER)", params![utc_dt], |r| r.get(0))
        .unwrap_or(1);
    w == 0 || w == 6
}

/// One recurrence step from a UTC datetime, computed in LOCAL wall-clock so the
/// time-of-day is preserved across DST. None for one-shot / unrecognised rules.
fn step_once(conn: &Connection, utc_dt: &str, rrule: &str) -> Option<String> {
    let modifier = match rrule {
        "daily" | "weekdays" => "+1 day".to_string(),
        "weekly" => "+7 days".to_string(),
        "biweekly" => "+14 days".to_string(),
        "monthly" => "+1 month".to_string(),
        "yearly" => "+1 year".to_string(),
        s if s.starts_with("every:") => {
            let p: Vec<&str> = s.split(':').collect(); // every:N:unit
            let n: i64 = p.get(1).and_then(|v| v.parse().ok()).filter(|n| *n > 0)?;
            match *p.get(2)? {
                "days" => format!("+{n} days"),
                "weeks" => format!("+{} days", n * 7),
                "months" => format!("+{n} months"),
                _ => return None,
            }
        }
        _ => return None,
    };
    conn.query_row(
        "SELECT datetime(datetime(?1,'localtime',?2),'utc')",
        params![utc_dt, modifier],
        |r| r.get(0),
    )
    .ok()
}

/// Next FUTURE occurrence strictly after now, skipping any overdue slots (so a
/// missed recurring reminder pings once, then resumes on schedule). Respects the
/// end bound and remaining count. Returns (next_utc, remaining_count) or None
/// when the series is over.
pub fn next_occurrence(
    conn: &Connection,
    from_utc: &str,
    rrule: &str,
    until_utc: Option<&str>,
    count: Option<i64>,
) -> Option<(String, Option<i64>)> {
    let now: String = conn.query_row("SELECT datetime('now')", [], |r| r.get(0)).ok()?;
    let mut cur = from_utc.to_string();
    let mut remaining = count;
    loop {
        if remaining == Some(0) {
            return None; // no fires left after the current one
        }
        let mut next = step_once(conn, &cur, rrule)?;
        if rrule == "weekdays" {
            let mut guard = 0;
            while is_weekend_local(conn, &next) && guard < 7 {
                next = step_once(conn, &next, "daily")?;
                guard += 1;
            }
        }
        remaining = remaining.map(|n| n - 1);
        if let Some(u) = until_utc {
            if next.as_str() > u {
                return None;
            }
        }
        cur = next;
        if cur.as_str() > now.as_str() {
            break;
        }
    }
    Some((cur, remaining))
}

pub fn mark_reminder_scheduled(conn: &Connection, id: i64, message_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET status='scheduled', message_id=?1, last_error=NULL, updated_at=?2 WHERE id=?3",
        params![message_id, now(), id],
    )?;
    Ok(())
}

pub fn mark_reminder_sent(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET status='sent', message_id=NULL, last_error=NULL, updated_at=?1 WHERE id=?2",
        params![now(), id],
    )?;
    Ok(())
}

pub fn mark_reminder_failed(conn: &Connection, id: i64, err: &str) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET status='failed', last_error=?1, updated_at=?2 WHERE id=?3",
        params![err, now(), id],
    )?;
    Ok(())
}

/// Advance a recurring reminder to its next slot and re-arm it (pending).
pub fn advance_reminder(conn: &Connection, id: i64, next_utc: &str, remaining: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET remind_at=?1, rrule_count=?2, status='pending', message_id=NULL, last_error=NULL, updated_at=?3 WHERE id=?4",
        params![next_utc, remaining, now(), id],
    )?;
    Ok(())
}

/// Drop the stored worker message id (after a cancel was pushed to the worker).
pub fn clear_reminder_message(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE reminders SET message_id=NULL, updated_at=?1 WHERE id=?2",
        params![now(), id],
    )?;
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
    let n = now();
    close_open_segment(conn, None)?;
    conn.execute(
        "UPDATE tasks SET status='paused', updated_at=?1 WHERE status='in_progress'",
        params![n],
    )?;
    conn.execute(
        "INSERT INTO segments (task_id, start_at, source) VALUES (?1, ?2, 'manual')",
        params![task_id, n],
    )?;
    conn.execute(
        "UPDATE tasks SET status='in_progress', updated_at=?1 WHERE id=?2",
        params![n, task_id],
    )?;
    // The window focused when the clock starts was captured as an untracked span
    // (no task was running). From this instant on it belongs to the task, so
    // split it here; otherwise the pre-existing open focus span keeps task_id
    // NULL until the next window switch and that stretch double-counts as BOTH
    // tracked (segment) and untracked (null focus span).
    attribute_open_focus(conn, task_id, &n)?;
    Ok(())
}

/// Attribute the currently-open focus span to `task_id` from `at` onward. The
/// span opened before the task started, so its time up to `at` stays untracked;
/// split it and open a fresh, task-attributed span for the same window so tracked
/// and untracked never overlap.
fn attribute_open_focus(conn: &Connection, task_id: i64, at: &str) -> Result<()> {
    let open: Option<(i64, Option<String>, Option<String>, String)> = conn
        .query_row(
            "SELECT id, app_id, title, start_at FROM focus_log WHERE end_at IS NULL LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok();
    if let Some((id, app_id, title, start_at)) = open {
        if start_at.as_str() < at {
            conn.execute("UPDATE focus_log SET end_at=?1 WHERE id=?2", params![at, id])?;
            conn.execute(
                "INSERT INTO focus_log (app_id, title, start_at, task_id) VALUES (?1, ?2, ?3, ?4)",
                params![app_id, title, at, task_id],
            )?;
        } else {
            conn.execute("UPDATE focus_log SET task_id=?1 WHERE id=?2", params![task_id, id])?;
        }
    }
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
    // run only long enough to bring the total up to the estimate. For a
    // recurring daily task only TODAY's closed segments count, since its
    // estimate is a per-day target that resets at midnight.
    let day_pred = if is_daily(conn, task_id) {
        " AND date(start_at,'localtime')=date('now','localtime')"
    } else {
        ""
    };
    let closed_secs: i64 = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(CAST((julianday(end_at)-julianday(start_at))*86400 AS INTEGER)),0)
                 FROM segments WHERE task_id=?1 AND end_at IS NOT NULL{day_pred}"
            ),
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

/// Public accessors over app_settings so sibling modules (e.g. email) can read
/// and persist their own configuration without duplicating the SQL.
pub fn setting(conn: &Connection, key: &str) -> Option<String> {
    get_setting(conn, key)
}
pub fn put_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    set_setting(conn, key, value)
}
/// Current local hour (0-23), for time-of-day scheduling.
pub fn hour_now(conn: &Connection) -> i64 {
    local_hour(conn)
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

/// A gap of at least this many minutes with no tracked work ends the current
/// continuous-work streak (the user rested / stepped away). Kept below the
/// 10-minute idle default so an idle episode always resets the break clock,
/// while brief task switches (seconds) do not.
const BREAK_STREAK_GAP_MIN: i64 = 5;

/// Minutes of the CURRENT continuous work streak (non-break tracked time), used
/// to time ultradian rest breaks. Unlike a plain sum since the break anchor,
/// this resets after any real gap in work (a break, going idle, lunch), so the
/// prompt fires after ~work_min of *continuous* focus, not lifetime work.
pub fn worked_since_break(conn: &Connection) -> Result<i64> {
    let n = now();
    let anchor = get_setting(conn, "break_anchor").unwrap_or_else(now);
    let btid = break_task_id(conn).unwrap_or(-1);

    // Non-break segments touching the window since the anchor, oldest first, as
    // (start, end) julian days; open segment ends at now.
    let mut stmt = conn.prepare(
        "SELECT julianday(start_at), julianday(COALESCE(end_at, ?1))
         FROM segments
         WHERE task_id <> ?2 AND COALESCE(end_at, ?1) > ?3
         ORDER BY start_at ASC",
    )?;
    let rows: Vec<(f64, f64)> = stmt
        .query_map(params![n, btid, anchor], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    if rows.is_empty() {
        return Ok(0);
    }

    let anchor_j: f64 = conn.query_row("SELECT julianday(?1)", params![anchor], |r| r.get(0))?;
    let gap = BREAK_STREAK_GAP_MIN as f64 / 1440.0;

    // Find where the trailing streak begins: scan the gaps between consecutive
    // segments and move the streak start past the last large gap.
    let mut streak_start = 0usize;
    for i in 1..rows.len() {
        if rows[i].0 - rows[i - 1].1 >= gap {
            streak_start = i;
        }
    }

    // Sum the streak's tracked minutes, clamping each start to the anchor.
    let mut days = 0.0;
    for &(s, e) in &rows[streak_start..] {
        let s = s.max(anchor_j);
        if e > s {
            days += e - s;
        }
    }
    Ok((days * 1440.0).round() as i64)
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
        Some(id) => tracked_minutes(conn, id, is_daily(conn, id)).unwrap_or(0),
        None => 0,
    };
    let tracked_today_min: i64 = conn.query_row(
        "SELECT COALESCE(SUM(CAST((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440 AS INTEGER)),0)
         FROM segments WHERE date(start_at,'localtime')=date('now','localtime')",
        params![now()],
        |r| r.get(0),
    ).unwrap_or(0);

    let minutes_committed: i64 = remaining_committed(conn, &day)?;

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
    // Minutes of the current CONTINUOUS work streak (resets after a break or any
    // real gap in work), so the break prompt is ultradian, not lifetime work.
    let worked_since_break_min: i64 = worked_since_break(conn).unwrap_or(0);

    // Today's away (idle, not at the machine) time, the third presence bucket.
    let away_today_min: i64 = away_minutes(conn, &day, &day).unwrap_or(0);

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
        away_today_min,
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

    // Focus = time tracked against a task. Untracked (= active, no task) = app
    // time captured while NO task was running, including time spent in Achieve
    // itself (planning, reviewing) — you were at the machine, not on a task, and
    // that time must not vanish. Idle is already excluded (the idle watcher caps
    // the focus span). `not_us` is kept only to keep our own window out of the
    // per-application breakdown below, never out of the untracked total.
    let not_us = "lower(COALESCE(app_id,'')) NOT LIKE '%achieve%'";
    let focus_min = total_tracked_min;
    let untracked_min: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE({bare_minutes},0) FROM focus_log
             WHERE {seg_bare} AND task_id IS NULL"
        ),
        params![n],
        |r| r.get(0),
    )?;
    let distraction_min = untracked_min;

    // Away = time idle / not at the machine (a third presence bucket).
    let away_min: i64 = away_minutes(conn, &start_date, &end_date)?;

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
        }
        if away_min > 0 {
            by_category.push(CategoryStat {
                name: "Away".into(),
                color: "#c3c7cf".into(),
                minutes: away_min,
            });
        }
        if untracked_min > 0 || away_min > 0 {
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

    // Pauses per task, grouped by reason. A reason is stamped on the segment
    // that ended when the clock stopped (the user's typed note, or a system
    // reason). Segments closed by simply switching tasks carry no reason, so
    // filtering to non-empty reasons captures exactly the pause-like stops.
    let mut task_pauses: std::collections::HashMap<i64, Vec<PauseStat>> =
        std::collections::HashMap::new();
    {
        // Reasons the app sets itself (everything else is a user-typed note).
        // Includes a few legacy/one-off tokens from earlier builds and manual
        // data repairs so they never masquerade as the user's own notes.
        let auto_reasons: std::collections::HashSet<&str> = [
            "auto-idle",
            "auto-suspend",
            "day-rollover",
            "reached-estimate",
            "rescheduled",
            "deleted",
            "break-start",
            "break-end",
            "capped-suspend",
            "capped-runaway",
        ]
        .into_iter()
        .collect();
        let mut stmt = conn.prepare(&format!(
            "SELECT s.task_id, s.reason, COUNT(*) AS cnt
             FROM segments s
             WHERE {seg_s} AND s.reason IS NOT NULL AND trim(s.reason) <> ''
             GROUP BY s.task_id, s.reason
             ORDER BY cnt DESC, s.reason",
        ))?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (tid, reason, cnt) = row?;
            if let Some(id) = tid {
                let auto = auto_reasons.contains(reason.as_str());
                task_pauses.entry(id).or_default().push(PauseStat { reason, count: cnt, auto });
            }
        }
        // User-typed reasons first (the interesting ones), then the automatic
        // ones; each group already ordered by frequency from SQL.
        for v in task_pauses.values_mut() {
            v.sort_by_key(|p| p.auto);
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
            "SELECT t.id, t.title, COALESCE(c.color,'#9aa0aa'), COALESCE(c.name,''), COALESCE(t.body_md,''),
                    COALESCE(t.estimate_min,0), t.status,
                    (SELECT COALESCE({seg_minutes},0) FROM segments s WHERE s.task_id=t.id AND {seg_s}) AS tracked
             FROM tasks t LEFT JOIN categories c ON c.id=t.category_id
             WHERE EXISTS (SELECT 1 FROM segments s WHERE s.task_id=t.id AND {seg_s})
             ORDER BY tracked DESC, t.id LIMIT 8",
        );
        let mut stmt = conn.prepare(&sql)?;
        let map = |r: &rusqlite::Row| {
            let status: String = r.get(6)?;
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                status == "completed",
                r.get::<_, i64>(7)?,
            ))
        };
        let rows: Vec<(i64, String, String, String, String, i64, bool, i64)> =
            stmt.query_map(params![n], map)?.collect::<std::result::Result<_, _>>()?;
        rows.into_iter()
            .map(|(id, title, color, category, body_md, estimate_min, done, tracked_min)| {
                let mut apps = task_apps.remove(&id).unwrap_or_default();
                apps.sort_by(|a, b| b.minutes.cmp(&a.minutes));
                apps.truncate(6);
                let pauses = task_pauses.remove(&id).unwrap_or_default();
                PlannedActual {
                    title,
                    color,
                    category,
                    body_md,
                    estimate_min,
                    tracked_min,
                    done,
                    untracked: false,
                    apps,
                    pauses,
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
            category: String::new(),
            body_md: String::new(),
            estimate_min: 0,
            tracked_min: untracked_min,
            done: false,
            untracked: true,
            apps: untracked_apps,
            pauses: Vec::new(),
        });
        planned_actual.sort_by(|a, b| b.tracked_min.cmp(&a.tracked_min));
    }

    // Pick the bucket's dominant label: the top category by tracked minutes,
    // unless untracked or away time outweighs it.
    let pick_top = |cat: Option<(String, String, i64)>, untr: i64, away: i64| -> (String, String) {
        let cat_mins = cat.as_ref().map(|c| c.2).unwrap_or(0);
        if cat_mins >= untr && cat_mins >= away && cat_mins > 0 {
            let c = cat.unwrap();
            (c.0, c.1)
        } else if untr >= away && untr > 0 {
            ("Untracked".into(), "#9aa0aa".into())
        } else if away > 0 {
            ("Away".into(), "#c3c7cf".into())
        } else {
            (String::new(), String::new())
        }
    };

    // hero chart: focus (tracked) vs untracked per bucket -> per-hour (day) or
    // per-day (week).
    let mut bars = Vec::new();
    let mut timeline: Vec<TimelineSpan> = Vec::new();
    let mut day_end_min: i64 = 0;
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
                     FROM focus_log WHERE date(start_at,'localtime')=?2 AND task_id IS NULL"
                ),
                params![n, date], |r| r.get(0))?;
            let away: i64 = away_minutes(conn, &date, &date)?;
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
            let (top, top_color) = pick_top(top_cat, untr, away);
            bars.push(Bar { label, focus_min: focus, untracked_min: untr, away_min: away, top, top_color });
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
               AND task_id IS NULL
             GROUP BY hr",
        ))?;
        let ahours = hour_buckets(&format!(
            "SELECT CAST(strftime('%H',start_at,'localtime') AS INTEGER) AS hr,
                    CAST(SUM((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440) AS INTEGER) AS mins
             FROM away_log WHERE date(start_at,'localtime')='{start_date}'
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
        let active = |h: usize| fhours[h] + uhours[h] + ahours[h] > 0;
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
            let (top, top_color) = pick_top(cat, uhours[h], ahours[h]);
            bars.push(Bar {
                label: format!("{h}"),
                focus_min: fhours[h],
                untracked_min: uhours[h],
                away_min: ahours[h],
                top,
                top_color,
            });
        }

        // Real sessions on the day timeline: each tracked segment (focus, with
        // its category color) and each untracked focus_log span, placed at its
        // exact local start/end minute. start_min from the clock; end_min via
        // duration so an overnight span clamps at midnight (1440) instead of
        // wrapping. Then merge same-kind, same-color spans separated by <=2 min.
        let min_of = |col: &str| {
            format!("CAST(strftime('%H',{col},'localtime') AS INTEGER)*60 + CAST(strftime('%M',{col},'localtime') AS INTEGER)")
        };
        let mut raw: Vec<TimelineSpan> = Vec::new();
        {
            let seg_min = min_of("s.start_at");
            let mut stmt = conn.prepare(&format!(
                "SELECT {seg_min} AS smin,
                        CAST(round((julianday(COALESCE(s.end_at,?1))-julianday(s.start_at))*1440) AS INTEGER) AS dur,
                        COALESCE(c.name,'Uncategorized'), COALESCE(c.color,'#9aa0aa')
                 FROM segments s JOIN tasks t ON t.id=s.task_id LEFT JOIN categories c ON c.id=t.category_id
                 WHERE date(s.start_at,'localtime')='{start_date}'
                 ORDER BY s.start_at"
            ))?;
            let rows = stmt.query_map(params![n], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
            })?;
            for row in rows {
                let (s, dur, name, color) = row?;
                raw.push(TimelineSpan { start_min: s, end_min: (s + dur.max(0)).min(1440), kind: "focus".into(), label: name, color });
            }
        }
        {
            let fl_min = min_of("start_at");
            let mut stmt = conn.prepare(&format!(
                "SELECT {fl_min} AS smin,
                        CAST(round((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440) AS INTEGER) AS dur
                 FROM focus_log
                 WHERE date(start_at,'localtime')='{start_date}' AND task_id IS NULL
                 ORDER BY start_at"
            ))?;
            let rows = stmt.query_map(params![n], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (s, dur) = row?;
                raw.push(TimelineSpan { start_min: s, end_min: (s + dur.max(0)).min(1440), kind: "untracked".into(), label: "Untracked".into(), color: "#e6a23c".into() });
            }
        }
        {
            let aw_min = min_of("start_at");
            let mut stmt = conn.prepare(&format!(
                "SELECT {aw_min} AS smin,
                        CAST(round((julianday(COALESCE(end_at,?1))-julianday(start_at))*1440) AS INTEGER) AS dur
                 FROM away_log
                 WHERE date(start_at,'localtime')='{start_date}'
                 ORDER BY start_at"
            ))?;
            let rows = stmt.query_map(params![n], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (s, dur) = row?;
                raw.push(TimelineSpan { start_min: s, end_min: (s + dur.max(0)).min(1440), kind: "away".into(), label: "Away".into(), color: "#9aa0aa".into() });
            }
        }
        raw.sort_by_key(|s| s.start_min);
        for s in raw {
            match timeline.last_mut() {
                Some(prev)
                    if prev.kind == s.kind
                        && prev.color == s.color
                        && s.start_min - prev.end_min <= 2 =>
                {
                    prev.end_min = prev.end_min.max(s.end_min);
                }
                _ => timeline.push(s),
            }
        }
        // Drop blocks that are still under a minute after merging (instant blips).
        timeline.retain(|s| s.end_min - s.start_min >= 1);

        // x-axis right edge = the stop time (default 6pm), extended past it if
        // activity ran later. Start is always midnight (0).
        let stop_min = stop_hhmm.as_deref().and_then(parse_hhmm_min).unwrap_or(18 * 60);
        let last_end = timeline.iter().map(|s| s.end_min).max().unwrap_or(0);
        day_end_min = stop_min.max(last_end).clamp(60, 1440);
    }

    Ok(Dashboard {
        period,
        start_date,
        end_date,
        total_tracked_min,
        focus_min,
        distraction_min,
        away_min,
        completed,
        total_tasks,
        by_category,
        by_app,
        planned_actual,
        bars,
        timeline,
        day_end_min,
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

/// Work still committed for the rest of the day: the sum of REMAINING minutes
/// (estimate minus time already tracked) over live tasks planned for today.
/// Using remaining work, not full estimates, is what makes "buffer =
/// minutes_left - committed" mean actual free time: a task you've nearly
/// finished no longer eats the whole day.
fn remaining_committed(conn: &Connection, day: &str) -> Result<i64> {
    let mut stmt = conn.prepare(
        "SELECT id, COALESCE(estimate_min, 0), recurrence FROM tasks
         WHERE status IN ('pending','paused','reopened','in_progress','awaiting')
           AND (plan_date = ?1 OR plan_date IS NULL)",
    )?;
    let rows = stmt.query_map(params![day], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?))
    })?;
    let mut total = 0i64;
    for row in rows {
        let (id, est, rec) = row?;
        if est <= 0 {
            continue;
        }
        let tracked = tracked_minutes(conn, id, rec.as_deref() == Some("daily")).unwrap_or(0);
        total += (est - tracked).max(0);
    }
    Ok(total)
}

/// The day's working window: local `HH:MM` of the first tracked segment's start
/// and the last segment's end for `date` (excluding the Break task, so it's
/// actual work). None if nothing was tracked that day.
pub fn work_bounds(conn: &Connection, date: &str) -> Option<(String, String)> {
    let btid = break_task_id(conn);
    conn.query_row(
        "SELECT strftime('%H:%M', MIN(start_at), 'localtime'),
                strftime('%H:%M', MAX(COALESCE(end_at, strftime('%Y-%m-%d %H:%M:%S','now'))), 'localtime')
         FROM segments
         WHERE date(start_at,'localtime') = ?1 AND task_id <> COALESCE(?2, -1)",
        params![date, btid],
        |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?)),
    )
    .ok()
    .and_then(|(a, b)| match (a, b) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    })
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
