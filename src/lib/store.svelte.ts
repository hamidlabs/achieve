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
  // Bumped on each surface so the tasks hub re-fits its height to content.
  fitTick: 0,
  // How many Overlay dialogs/popovers are open. While > 0 the hub suspends its
  // list auto-fit (the Overlay owns the window height so its content fits).
  overlayCount: 0,
});

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

/** Navigate the adaptive window to a view (resizes via the backend). */
export async function go(view: View) {
  store.view = view;
  try {
    await api.setView(view);
  } catch {
    /* dev/standalone: just switch the view locally */
  }
}
