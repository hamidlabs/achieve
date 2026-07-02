import { invoke } from "@tauri-apps/api/core";
import type {
  BreakSettings,
  Category,
  Dashboard,
  DayPlan,
  FocusSpan,
  Snapshot,
  Task,
  View,
} from "./types";

// Thin typed wrapper over the Tauri command layer (src-tauri/src/commands.rs).
export const api = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  tasks: () => invoke<Task[]>("list_tasks"),
  upcoming: () => invoke<Task[]>("list_upcoming"),
  categories: () => invoke<Category[]>("list_categories"),
  createCategory: (name: string, color: string) =>
    invoke<number>("create_category", { name, color }),
  updateCategory: (id: number, name: string, color: string) =>
    invoke<void>("update_category", { id, name, color }),
  deleteCategory: (id: number) => invoke<void>("delete_category", { id }),

  // NOTE: Tauri maps camelCase JS args -> snake_case Rust params, so keys here
  // must be camelCase (bodyMd, estimateMin, categoryId, ...).
  createTask: (t: {
    category_id: number | null;
    title: string;
    body_md: string;
    estimate_min: number | null;
    recurrence: string | null;
  }) =>
    invoke<number>("create_task", {
      categoryId: t.category_id,
      title: t.title,
      bodyMd: t.body_md,
      estimateMin: t.estimate_min,
      recurrence: t.recurrence,
    }),
  updateTask: (t: {
    id: number;
    category_id: number | null;
    title: string;
    body_md: string;
    estimate_min: number | null;
    recurrence: string | null;
  }) =>
    invoke<void>("update_task", {
      id: t.id,
      categoryId: t.category_id,
      title: t.title,
      bodyMd: t.body_md,
      estimateMin: t.estimate_min,
      recurrence: t.recurrence,
    }),
  deleteTask: (id: number) => invoke<void>("delete_task", { id }),

  startTask: (taskId: number) => invoke<void>("start_task", { taskId }),
  pauseTask: (taskId: number, reason: string) =>
    invoke<void>("pause_task", { taskId, reason }),
  extendActive: (taskId: number, minutes: number) =>
    invoke<void>("extend_active", { taskId, minutes }),
  completeTask: (taskId: number) => invoke<void>("complete_task", { taskId }),
  reopenTask: (taskId: number) => invoke<void>("reopen_task", { taskId }),
  rescheduleTask: (taskId: number) =>
    invoke<void>("reschedule_task", { taskId }),
  setPlanDate: (taskId: number, date: string | null) =>
    invoke<void>("set_plan_date", { taskId, date }),

  dayPlan: () => invoke<DayPlan>("get_day_plan"),
  saveDayPlan: (p: {
    intentions: string;
    available_minutes: number;
    stop_time: string | null;
  }) =>
    invoke<void>("save_day_plan", {
      intentions: p.intentions,
      availableMinutes: p.available_minutes,
      stopTime: p.stop_time,
    }),
  setStopTime: (stopTime: string) =>
    invoke<void>("set_stop_time", { stopTime }),
  weekStart: () => invoke<number>("get_week_start"),
  setWeekStart: (day: number) => invoke<void>("set_week_start", { day }),
  breakSettings: () => invoke<BreakSettings>("get_break_settings"),
  setBreakSettings: (settings: BreakSettings) =>
    invoke<void>("set_break_settings", { settings }),
  startBreak: () => invoke<void>("start_break"),
  endBreak: (resume: boolean) => invoke<void>("end_break", { resume }),
  snoozeBreak: (minutes: number) => invoke<void>("snooze_break", { minutes }),
  skipBreak: () => invoke<void>("skip_break"),

  focusSpans: () => invoke<FocusSpan[]>("get_focus_spans"),
  labelFocus: (focusId: number, label: string, taskId: number | null) =>
    invoke<void>("label_focus", { focusId, label, taskId }),

  dashboard: (period: "day" | "week" | "month" = "day", offset = 0) =>
    invoke<Dashboard>("get_dashboard", { period, offset }),

  setView: (view: View) => invoke<void>("set_view", { view }),
  resizeWindow: (view: View | "editor") =>
    invoke<void>("resize_window", { view }),
  fitWindow: (height: number) => invoke<void>("fit_window", { height }),
  dismiss: () => invoke<void>("dismiss_popup"),
  quit: () => invoke<void>("quit_app"),
  // Send the daily summary email now (offset: 0=today, 1=yesterday).
  sendSummaryNow: (offset = 1) => invoke<string>("send_summary_now", { offset }),
};
