// Human-readable formatting for reminders, shared by the list and editor so the
// summary a user sees while editing matches the row afterwards.

import type { Reminder, Rrule } from "./types";

/** "9:00 AM" from a local "YYYY-MM-DD HH:MM" (or "HH:MM"). */
export function fmtTime(hhmm: string): string {
  const t = hhmm.includes(" ") ? hhmm.split(" ")[1] : hhmm;
  const [hStr, mStr] = t.split(":");
  const h = Number(hStr);
  const m = mStr ?? "00";
  const ampm = h < 12 ? "AM" : "PM";
  const h12 = h % 12 === 0 ? 12 : h % 12;
  return `${h12}:${m} ${ampm}`;
}

function ymd(d: Date): string {
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** "Today", "Tomorrow", or "Mon, Jul 20" for a local "YYYY-MM-DD". */
export function fmtDay(date: string): string {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const tomorrow = new Date(today);
  tomorrow.setDate(today.getDate() + 1);
  if (date === ymd(today)) return "Today";
  if (date === ymd(tomorrow)) return "Tomorrow";
  const d = new Date(date + "T00:00:00");
  const opts: Intl.DateTimeFormatOptions = { weekday: "short", month: "short", day: "numeric" };
  if (d.getFullYear() !== today.getFullYear()) opts.year = "numeric";
  return d.toLocaleDateString(undefined, opts);
}

/** "every day", "every weekday", "every 2 weeks", ... or null for one-shot. */
export function fmtRrule(rrule: Rrule | null): string | null {
  if (!rrule) return null;
  switch (rrule) {
    case "daily":
      return "every day";
    case "weekdays":
      return "every weekday";
    case "weekly":
      return "every week";
    case "biweekly":
      return "every 2 weeks";
    case "monthly":
      return "every month";
    case "yearly":
      return "every year";
  }
  if (rrule.startsWith("every:")) {
    const [, nStr, unit] = rrule.split(":");
    const n = Number(nStr) || 1;
    const u = unit ?? "days";
    const single = u.replace(/s$/, "");
    return n === 1 ? `every ${single}` : `every ${n} ${u}`;
  }
  return rrule;
}

/** One-line summary, e.g. "Every weekday at 9:00 AM" or "Tomorrow at 2:30 PM". */
export function fmtReminder(r: Reminder): string {
  const [date] = r.remind_at_local.split(" ");
  const time = fmtTime(r.remind_at_local);
  const rep = fmtRrule(r.rrule);
  if (rep) {
    const cap = rep.charAt(0).toUpperCase() + rep.slice(1);
    return `${cap} at ${time}`;
  }
  return `${fmtDay(date)} at ${time}`;
}

export function channelLabel(c: string): string {
  return c === "both" ? "Email + notification" : c === "email" ? "Email" : "Notification";
}
