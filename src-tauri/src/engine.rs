//! The engine: a background loop that drives the day proactively.
//!
//! It is decoupled from the UI (the dirwatch lesson). Each tick it derives the
//! privacy-safe snapshot from the append-only ledger, pushes it to the UI, and
//! decides whether to surface a view:
//!   - morning: if today is unplanned, open the planner
//!   - active but nothing tracking: re-nudge to start a task on a gentle cadence
//!   - idle (away from keyboard): cap the open segment + pause, stay quiet
//!   - evening: once the day's stop time passes, open the wind-down check-in
//!
//! All proactive showing goes through `window::show_view`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager};

use crate::db;
use crate::email;
use crate::window;

const TICK: Duration = Duration::from_secs(20);
// If you are active with no task selected, re-offer to start one this often.
const RENUDGE_AFTER: Duration = Duration::from_secs(5 * 60);
// On a failed daily-email send, wait this long before retrying (so a bad key or
// a network blip retries a few times across the morning, not every 20s).
const EMAIL_RETRY: Duration = Duration::from_secs(10 * 60);

pub fn spawn(app: AppHandle, db: Arc<Mutex<Connection>>, idle_flag: PathBuf, idle_secs: i64) {
    thread::spawn(move || {
        // Let the webview finish loading before the first proactive surface.
        thread::sleep(Duration::from_secs(2));
        let mut started = false;
        let mut last_nudge: Option<Instant> = None;
        let mut was_idle = false;
        let mut last_break_prompt: Option<Instant> = None;
        let mut last_email_attempt: Option<Instant> = None;
        // Startup catch-up: send today's summary on this boot even if we launched
        // after the configured hour. Cleared after the first send (or once we see
        // today's is already sent) so it can't re-fire at midnight on later days.
        let mut email_catchup = true;
        let mut current_day = String::new();

        loop {
            let snap = match db.lock().ok().and_then(|c| db::snapshot(&c).ok()) {
                Some(s) => s,
                None => {
                    thread::sleep(TICK);
                    continue;
                }
            };
            let _ = app.emit("snapshot", &snap);

            // Daily summary email. Cheap gate every tick; when due, throttle real
            // attempts so a failing send retries across the morning, not every
            // tick. Build under the lock, send unlocked, then mark sent.
            let want_email = db
                .lock()
                .ok()
                .map(|c| email::should_send(&c) || (email_catchup && email::should_send_on_start(&c)))
                .unwrap_or(false);
            if want_email {
                let due = last_email_attempt.map(|t| t.elapsed() >= EMAIL_RETRY).unwrap_or(true);
                if due {
                    last_email_attempt = Some(Instant::now());
                    let payload = db.lock().ok().and_then(|c| email::build_due_payload(&c));
                    if let Some(p) = payload {
                        match email::send(&p) {
                            Ok(()) => {
                                if let Ok(c) = db.lock() {
                                    email::mark_sent(&c);
                                }
                                email_catchup = false;
                                println!("[email] daily summary sent to {}", p.to);
                            }
                            Err(e) => eprintln!("[email] send failed: {e}"),
                        }
                    }
                }
            } else {
                // Nothing due (disabled, misconfigured, or already sent today):
                // end the startup catch-up so it can't fire at local midnight.
                email_catchup = false;
                last_email_attempt = None;
            }

            // Day rollover: when the local day changes (or at startup, catching a
            // launch after midnight), stop any tracking that crossed midnight and
            // surface yesterday's leftover task on today's list.
            let day = db.lock().ok().map(|c| db::today(&c)).unwrap_or_default();
            if !day.is_empty() && day != current_day {
                current_day = day;
                if let Ok(c) = db.lock() {
                    if db::rollover_day(&c).unwrap_or(false) {
                        if let Ok(s) = db::snapshot(&c) {
                            let _ = app.emit("snapshot", &s);
                        }
                        let _ = app.emit("tasks-changed", ());
                    }
                }
            }

            let visible = app
                .get_webview_window("main")
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false);

            // Idle (away from keyboard): swayidle has raised the flag file. Once
            // per idle episode, cap the open work segment AND focus span at the
            // moment input stopped and pause the active task, so stepping away
            // (or sleeping with a task "running") never accrues phantom hours and
            // idle time is excluded from the untracked total. Stay quiet (no
            // nudges) while away.
            let idle = idle_flag.exists();
            if idle {
                if !was_idle {
                    was_idle = true;
                    if let Ok(c) = db.lock() {
                        if db::pause_for_idle(&c, idle_secs).unwrap_or(false) {
                            if let Ok(s) = db::snapshot(&c) {
                                let _ = app.emit("snapshot", &s);
                            }
                        }
                    }
                }
                thread::sleep(TICK);
                continue;
            }
            was_idle = false;

            // First surface after launch: the unified task hub (planning is
            // merged in, so there's no separate planner anymore).
            if !started {
                started = true;
                let view = std::env::var("ACHIEVE_START_VIEW").unwrap_or_else(|_| "nudge".into());
                window::show_view(&app, &view);
                last_nudge = Some(Instant::now());
                thread::sleep(TICK);
                continue;
            }

            // While a task is actively TRACKING, keep resetting the re-nudge
            // clock so we don't pop the hub. An `awaiting` task (estimate reached,
            // clock stopped) is NOT tracking: it still needs a decision, so let
            // the clock run and treat it like a nudge below.
            if snap.active_task_id.is_some() && !snap.active_awaiting {
                last_nudge = Some(Instant::now());
            }

            // Reached-estimate: a task hit its estimate while tracking. STOP the
            // clock right at the estimate (so an unanswered popup or stepping
            // away can never accrue phantom hours) by moving it to `awaiting`,
            // then surface the popup to extend or finish. Once awaiting, the
            // clock is already stopped so this won't re-fire until the user
            // extends (which re-arms by pushing tracked back under the estimate).
            match (snap.active_task_id, snap.active_estimate_min) {
                (Some(id), Some(est))
                    if est > 0 && !snap.active_awaiting && snap.active_tracked_min >= est =>
                {
                    if let Ok(c) = db.lock() {
                        if db::pause_at_estimate(&c, id, est).unwrap_or(false) {
                            if let Ok(s) = db::snapshot(&c) {
                                let _ = app.emit("snapshot", &s);
                            }
                        }
                    }
                    if !visible {
                        window::show_view(&app, "nudge");
                    }
                }
                _ => {}
            }

            // Rest breaks (ultradian): after a stretch of focused work, gently
            // surface the break prompt; while on a break, surface "break over"
            // once it elapses. Both only when the window is hidden (we don't
            // hijack the screen mid-action), and re-offered on a calm cadence.
            let (due_for_break, break_over) = {
                if let Ok(c) = db.lock() {
                    let bs = db::get_break_settings(&c);
                    let snoozed = db::break_snoozed(&c);
                    let due = bs.enabled
                        && !snap.on_break
                        && snap.active_task_id.is_some()
                        && snap.worked_since_break_min >= bs.work_min
                        && !snoozed;
                    let over = snap.on_break && snap.break_remaining_sec <= 0;
                    (due, over)
                } else {
                    (false, false)
                }
            };
            if (due_for_break || break_over) && !visible {
                let due = last_break_prompt
                    .map(|t| t.elapsed() >= RENUDGE_AFTER)
                    .unwrap_or(true);
                if due {
                    window::show_view(&app, "break");
                    last_break_prompt = Some(Instant::now());
                }
            } else if !due_for_break && !break_over {
                last_break_prompt = None;
            }

            // Nudge to start tracking: you're active (not idle, since we returned
            // above) but nothing is being tracked. Re-offer the task list on a
            // gentle cadence whenever there's work pending OR there's still time
            // left in the day (so an empty list still invites you to add a task,
            // but we don't nag late at night with nothing planned).
            // Surface the hub when something needs attention and the window is
            // hidden: either no task is selected (pick one) OR a task is
            // `awaiting` a decision after reaching its estimate (extend/finish).
            // The awaiting case must re-surface even past the stop time, since an
            // unresolved estimate left open is exactly what we must not forget.
            let needs_attention = snap.active_task_id.is_none() || snap.active_awaiting;
            let worth_surfacing =
                snap.active_awaiting || snap.pending > 0 || snap.minutes_left_in_day > 0;
            if !visible && needs_attention && worth_surfacing {
                let due = last_nudge
                    .map(|t| t.elapsed() >= RENUDGE_AFTER)
                    .unwrap_or(true);
                if due {
                    window::show_view(&app, "nudge");
                    last_nudge = Some(Instant::now());
                }
            }

            thread::sleep(TICK);
        }
    });
}
