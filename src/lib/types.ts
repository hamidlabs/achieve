// Shared types mirrored from the Rust backend (src-tauri/src/model.rs).

export type TaskStatus =
  | "pending"
  | "in_progress"
  // Reached its estimate; clock paused, awaiting extend (+15/+30) or finish.
  | "awaiting"
  | "paused"
  | "completed"
  | "reopened";

// Only two surfaces remain: the unified task hub ("nudge") and the dashboard.
export type View = "nudge" | "dashboard" | "break";

export interface Category {
  id: number;
  name: string;
  color: string;
  parent_id: number | null;
}

export interface Task {
  id: number;
  category_id: number | null;
  category_name: string | null;
  category_color: string | null;
  title: string;
  body_md: string;
  estimate_min: number | null;
  status: TaskStatus;
  recurrence: string | null;
  plan_date: string | null;
  tracked_min: number;
}

export interface Snapshot {
  pending: number;
  in_progress: number;
  completed_today: number;
  active_task_id: number | null;
  active_task_title: string | null;
  /** Active task reached its estimate and is paused awaiting extend/finish. */
  active_awaiting: boolean;
  active_since_min: number;
  active_estimate_min: number | null;
  active_tracked_min: number;
  tracked_today_min: number;
  minutes_left_in_day: number;
  minutes_committed: number;
  greeting: string;
  planned_today: boolean;
  /** Continuous tracked work minutes since the last break. */
  worked_since_break_min: number;
  /** True while on a break (the Break task is active). */
  on_break: boolean;
  /** Seconds left in the current break (<=0 once it's over). */
  break_remaining_sec: number;
}

export interface BreakSettings {
  enabled: boolean;
  work_min: number;
  duration_min: number;
  snooze_min: number;
}

export interface DayPlan {
  date: string;
  intentions: string;
  available_minutes: number;
  stop_time: string | null;
}

export interface FocusSpan {
  id: number;
  app_id: string | null;
  title: string | null;
  start_at: string;
  minutes: number;
}

export interface CategoryStat {
  name: string;
  color: string;
  minutes: number;
}

export interface AppStat {
  app: string;
  minutes: number;
}

export interface PauseStat {
  reason: string;
  count: number;
  auto: boolean;
}

export interface PlannedActual {
  title: string;
  color: string;
  category: string;
  body_md: string;
  estimate_min: number;
  tracked_min: number;
  done: boolean;
  untracked: boolean;
  apps: AppStat[];
  pauses: PauseStat[];
}

export interface Bar {
  label: string;
  focus_min: number;
  untracked_min: number;
  top: string;
  top_color: string;
}

export type DashPeriod = "day" | "week" | "month";

export interface TimelineSpan {
  start_min: number;
  end_min: number;
  focus: boolean;
  label: string;
  color: string;
}

export interface Dashboard {
  period: DashPeriod;
  start_date: string;
  end_date: string;
  total_tracked_min: number;
  focus_min: number;
  distraction_min: number;
  completed: number;
  total_tasks: number;
  by_category: CategoryStat[];
  by_app: AppStat[];
  planned_actual: PlannedActual[];
  bars: Bar[];
  timeline: TimelineSpan[];
  day_end_min: number;
}
