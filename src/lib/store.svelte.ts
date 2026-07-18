import { api } from "./api";
import type { Category, DayPlan, Snapshot, Task, View } from "./types";

// Shared reactive app state (Svelte 5 runes module).
export const store = $state({
  view: "nudge" as View,
  snapshot: null as Snapshot | null,
  tasks: [] as Task[],
  upcoming: [] as Task[],
  categories: [] as Category[],
  dayplan: null as DayPlan | null,
  revealed: false,
  ready: false,
  backendOk: true,
  // Today card collapsed (chart hidden) vs expanded.
  cardCollapsed: true,
  // Bumped on each surface so a surfaced view can re-run mount-time effects.
  fitTick: 0,
  // How many Overlay dialogs/popovers are open (used to suppress duplicate
  // window chrome / keep the shell calm while a modal owns the screen).
  overlayCount: 0,
  // ---- app-level modals, driven by the bottom nav from any view ----
  // The task editor: null = closed; { task } = open (task null ⇒ add new).
  editor: null as { task: Task | null } | null,
  // Break/rest settings popover.
  settingsOpen: false,
});

/** Open the shared task editor (task = null ⇒ create a new task). */
export function openTaskEditor(task: Task | null = null) {
  store.editor = { task };
}
export function closeTaskEditor() {
  store.editor = null;
}

const PLACEHOLDER_SNAPSHOT: Snapshot = {
  pending: 3,
  in_progress: 1,
  completed_today: 2,
  active_task_id: null,
  active_task_title: null,
  active_awaiting: false,
  active_since_min: 0,
  active_estimate_min: null,
  active_tracked_min: 0,
  tracked_today_min: 0,
  minutes_left_in_day: 232,
  minutes_committed: 310,
  greeting: "Let's make today count",
  planned_today: true,
  worked_since_break_min: 0,
  away_today_min: 0,
  on_break: false,
  break_remaining_sec: 0,
};

export async function refreshAll() {
  try {
    const [snapshot, tasks, upcoming, categories, dayplan] = await Promise.all([
      api.snapshot(),
      api.tasks(),
      api.upcoming(),
      api.categories(),
      api.dayPlan(),
    ]);
    store.snapshot = snapshot;
    store.tasks = tasks;
    store.upcoming = upcoming;
    store.categories = categories;
    store.dayplan = dayplan;
    store.backendOk = true;
  } catch (e) {
    console.warn("[achieve] backend unavailable, placeholder mode:", e);
    store.backendOk = false;
    store.snapshot = PLACEHOLDER_SNAPSHOT;
  }
  store.ready = true;
}

export async function refreshSnapshot() {
  try {
    store.snapshot = await api.snapshot();
  } catch {
    /* keep last */
  }
}

export async function refreshTasks() {
  try {
    const [tasks, upcoming] = await Promise.all([api.tasks(), api.upcoming()]);
    store.tasks = tasks;
    store.upcoming = upcoming;
  } catch {
    /* keep last */
  }
}

export async function refreshCategories() {
  try {
    store.categories = await api.categories();
  } catch {
    /* keep last */
  }
}

/** Switch the visible surface. Every non-break view shares one fixed window
 *  frame, so this is a purely local change: no backend resize/recenter (that
 *  would animate the whole window via niri and read as jank). The frontend
 *  handles the smooth view-to-view transition. */
export function go(view: View) {
  store.view = view;
}
