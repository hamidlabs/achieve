//! Data types shared with the Svelte frontend (mirrored in src/lib/types.ts).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub category_color: Option<String>,
    pub title: String,
    pub body_md: String,
    pub estimate_min: Option<i64>,
    pub status: String,
    pub recurrence: Option<String>,
    pub plan_date: Option<String>,
    /// Sum of all tracked segments for this task, including any open one.
    pub tracked_min: i64,
}

/// Privacy-safe summary shown before the user taps Reveal. Counts only, never
/// task content, so it is safe to have on screen during a shared meeting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub pending: i64,
    pub in_progress: i64,
    pub completed_today: i64,
    pub active_task_id: Option<i64>,
    pub active_task_title: Option<String>,
    /// The active task reached its estimate and is paused awaiting a decision
    /// (extend or finish). Its clock is stopped; `active_since_min` is 0.
    pub active_awaiting: bool,
    pub active_since_min: i64,
    /// Estimate of the active task (None if it was entered without one).
    pub active_estimate_min: Option<i64>,
    /// Total tracked minutes on the active task across ALL its segments
    /// (not just the current one) so overrun is measured against real effort.
    pub active_tracked_min: i64,
    /// Total minutes tracked today across all tasks (for the day summary).
    pub tracked_today_min: i64,
    pub minutes_left_in_day: i64,
    pub minutes_committed: i64,
    pub greeting: String,
    pub planned_today: bool,
    /// Minutes of continuous tracked work since the last break (drives break timing).
    pub worked_since_break_min: i64,
    /// Minutes spent away from the machine (idle) today: the third presence bucket.
    pub away_today_min: i64,
    /// True while the active task is the special Break task (you're on a break).
    pub on_break: bool,
    /// Seconds left in the current break (can be <=0 once the break is over).
    pub break_remaining_sec: i64,
}

/// A task reminder. Times are surfaced to the UI in LOCAL wall-clock (what the
/// user picked); `remind_at` (UTC) is the source of truth the engine compares
/// against now(). Recurring reminders advance in place to the next slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub task_id: i64,
    /// Next fire time as local "YYYY-MM-DD HH:MM" (for display + editing).
    pub remind_at_local: String,
    /// Next fire time, UTC "YYYY-MM-DD HH:MM:SS".
    pub remind_at: String,
    /// None = one-shot. Else: daily | weekdays | weekly | biweekly | monthly |
    /// yearly | every:N:days | every:N:weeks | every:N:months.
    pub rrule: Option<String>,
    /// Inclusive recurrence end as local "YYYY-MM-DD", if any.
    pub rrule_until: Option<String>,
    /// Remaining fires (including the next one); None = unbounded.
    pub rrule_count: Option<i64>,
    /// email | notification | both.
    pub channel: String,
    pub note: Option<String>,
    /// pending | scheduled | sent | cancelled | failed.
    pub status: String,
}

/// User-tunable rest-break (ultradian) settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakSettings {
    pub enabled: bool,
    /// Minutes of continuous tracked work before a break is suggested.
    pub work_min: i64,
    /// How long a break runs.
    pub duration_min: i64,
    /// How long "Snooze" defers the prompt.
    pub snooze_min: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayPlan {
    pub date: String,
    pub intentions: String,
    pub available_minutes: i64,
    pub stop_time: Option<String>,
}

/// A span of automatic focus capture awaiting the user's one-tap label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusSpan {
    pub id: i64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub start_at: String,
    pub minutes: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryStat {
    pub name: String,
    pub color: String,
    pub minutes: i64,
}

/// Time spent in one application (the automatic ground truth from focus_log).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStat {
    pub app: String,
    pub minutes: i64,
}

/// One kind of pause recorded against a task, grouped by its reason. A reason is
/// stamped on the segment that ENDED when the task's clock stopped: either the
/// note the user typed when pausing, or a system reason (idle, suspend, reaching
/// the estimate, day rollover). `auto` distinguishes the two so the UI can show
/// the user's own reasons prominently and the automatic ones quietly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseStat {
    pub reason: String,
    pub count: i64,
    pub auto: bool,
}

/// A task (or the synthetic "Untracked" bucket) with its actual tracked time and
/// the apps it was spent in, so the dashboard can show where each task's time went.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedActual {
    pub title: String,
    pub color: String,
    pub category: String,
    pub body_md: String,
    pub estimate_min: i64,
    pub tracked_min: i64,
    pub done: bool,
    /// true for the synthetic "Untracked" row (active time with no task = distraction).
    pub untracked: bool,
    pub apps: Vec<AppStat>,
    /// Pauses recorded for this task in the window, grouped by reason (user notes
    /// first, then automatic ones), most frequent first.
    pub pauses: Vec<PauseStat>,
}

/// One column of the hero activity chart (an hour of the day, or a day of the
/// week), split into focused (tracked) vs untracked (no task) minutes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub label: String,
    pub focus_min: i64,
    pub untracked_min: i64,
    /// Away (idle, not at the machine) minutes in this bucket.
    pub away_min: i64,
    /// Dominant category (or "Untracked"/"Away") in this bucket, for the tooltip.
    pub top: String,
    pub top_color: String,
}

/// One real tracking session on the day timeline: the exact local minute it
/// started and ended (minutes from midnight), whether it was focused work or
/// untracked time, and its category/app for the label. Used by the day chart to
/// draw activity at its true clock position instead of hourly buckets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSpan {
    pub start_min: i64,
    pub end_min: i64,
    /// "focus" (tracked), "untracked" (active, no task), or "away" (idle).
    pub kind: String,
    pub label: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dashboard {
    pub period: String, // "day" | "week" | "month"
    pub start_date: String,
    pub end_date: String,
    pub total_tracked_min: i64,
    pub focus_min: i64,
    pub distraction_min: i64,
    /// Minutes away from the machine (idle) in the window: the third presence bucket.
    pub away_min: i64,
    pub completed: i64,
    pub total_tasks: i64,
    pub by_category: Vec<CategoryStat>,
    pub by_app: Vec<AppStat>,
    pub planned_actual: Vec<PlannedActual>,
    pub bars: Vec<Bar>,
    /// Day period only: actual sessions on a 12am->stop-time timeline.
    pub timeline: Vec<TimelineSpan>,
    /// Day period only: right edge of the timeline axis (minutes from midnight)
    /// = the day's stop time, extended to cover any later activity.
    pub day_end_min: i64,
}
