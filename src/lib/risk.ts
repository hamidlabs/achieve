// Which planned tasks need attention, and how badly. Used by the break overlay
// to surface at-risk work ("planned 4h, invested 0m, and the day's buffer is
// gone"). The backend runs the same idea for notifications; keep them in sync.

import type { Snapshot, Task } from "./types";

export type RiskLevel = "urgent" | "behind" | "ok";

export interface RiskItem {
  task: Task;
  level: RiskLevel;
  /** Minutes of estimated work still to do. */
  remaining: number;
  /** Fraction of the estimate already tracked (0..1+). */
  progress: number;
}

// Statuses that represent live, plannable work (excludes completed and the
// internal "break" task status).
const LIVE = new Set(["pending", "paused", "reopened", "in_progress", "awaiting"]);

/** Day buffer = time left in the day minus time already committed to plans. */
export function dayBuffer(snap: Snapshot | null): number {
  return (snap?.minutes_left_in_day ?? 0) - (snap?.minutes_committed ?? 0);
}

/** Rank tasks by how urgently they need focus, most urgent first. */
export function assessTasks(tasks: Task[], snap: Snapshot | null): RiskItem[] {
  const buffer = dayBuffer(snap);

  const items = tasks
    .filter((t) => LIVE.has(t.status) && (t.estimate_min ?? 0) > 0)
    .map((t): RiskItem => {
      const est = t.estimate_min ?? 0;
      const remaining = Math.max(0, est - t.tracked_min);
      const progress = est > 0 ? t.tracked_min / est : 1;

      let level: RiskLevel = "ok";
      if (remaining > 0) {
        // Urgent: barely touched AND the day can't absorb it (no buffer left,
        // or the remaining work no longer fits in what's left).
        if (progress < 0.1 && (buffer <= 0 || remaining > buffer)) level = "urgent";
        // Behind: under way but slipping, and it's eating into thin buffer.
        else if (progress < 0.5 && remaining > buffer) level = "behind";
      }
      return { task: t, level, remaining, progress };
    });

  const rank: Record<RiskLevel, number> = { urgent: 0, behind: 1, ok: 2 };
  return items.sort(
    (a, b) => rank[a.level] - rank[b.level] || b.remaining - a.remaining,
  );
}
