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
use crate::media;
use crate::notify;
use crate::reminder;
use crate::window;

const TICK: Duration = Duration::from_secs(20);
// If you are active with no task selected, re-offer to start one this often.
const RENUDGE_AFTER: Duration = Duration::from_secs(5 * 60);
// Audible untracked cue. You are present and working, but the clock isn't
// running. The cue is a reminder, not an alarm, so it backs off: a grace period
// before the first beep, then widening gaps, then silence (the visual nudge
// keeps coming; the sound gives up rather than nagging all afternoon).
const UNTRACKED_CUE_STEPS: [u64; 5] = [90, 3 * 60, 6 * 60, 12 * 60, 20 * 60];
// On a failed daily-email send, wait this long before retrying (so a bad key or
// a network blip retries a few times across the morning, not every 20s).
const EMAIL_RETRY: Duration = Duration::from_secs(10 * 60);
// How often to check for reminders to schedule/fire (kept independent of TICK so
// it stays ~steady even while the loop spins every 1s during a break).
const REMINDER_INTERVAL: Duration = Duration::from_secs(20);

pub fn spawn(app: AppHandle, db: Arc<Mutex<Connection>>, idle_flag: PathBuf, idle_secs: i64) {
    thread::spawn(move || {
        // Let the webview finish loading before the first proactive surface.
        thread::sleep(Duration::from_secs(2));
        let mut started = false;
        let mut last_nudge: Option<Instant> = None;
        let mut was_idle = false;
        // Last active task id we told the frontend about, to detect engine-side
        // (auto-pause on idle/suspend) transitions and refresh the task list.
        let mut last_active_seen: Option<Option<i64>> = None;
        let mut last_break_prompt: Option<Instant> = None;
        // Untracked-cue state: when the current untracked stretch began, and how
        // many cues we've played in it. Both reset the moment a task tracks.
        let mut untracked_since: Option<Instant> = None;
        let mut untracked_cues = 0usize;
        // Play the "break over" cue once per break, from the engine so it fires
        // even if the break window is hidden or the webview throttled its
        // countdown. Reset when the break ends.
        let mut break_chimed_over = false;
        // Throttle snapshot emits to the UI: the loop can tick every second
        // during a break, but the frontend never needs a re-render that often.
        let mut last_emit: Option<Instant> = None;
        // At-risk task notification cadence.
        let mut last_risk_check: Option<Instant> = None;
        let mut last_risk_notify: Option<Instant> = None;
        let mut last_email_attempt: Option<Instant> = None;
        let mut last_reminder_check: Option<Instant> = None;
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
            if last_emit.map(|t| t.elapsed() >= Duration::from_secs(2)).unwrap_or(true) {
                let _ = app.emit("snapshot", &snap);
                last_emit = Some(Instant::now());
            }

            // Presence heartbeat: mark the app alive so the next launch can book
            // the intervening downtime (machine off / app not running) as away.
            // Then keep TODAY's away ledger materialized as the complement of
            // present time, so the live day's buckets tile without overlap.
            if let Ok(c) = db.lock() {
                let _ = db::touch_seen(&c);
                let _ = db::reconcile_today(&c);
            }

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

            // Task reminders: schedule those entering the 72h window, fire due
            // ones (email via the worker + desktop notification), and advance
            // recurrence. HTTP happens off the DB lock inside dispatch.
            if last_reminder_check.map(|t| t.elapsed() >= REMINDER_INTERVAL).unwrap_or(true) {
                last_reminder_check = Some(Instant::now());
                reminder::dispatch(&db);
            }

            // Day rollover: when the local day changes (or at startup, catching a
            // launch after midnight), stop any tracking that crossed midnight and
            // surface yesterday's leftover task on today's list.
            let day = db.lock().ok().map(|c| db::today(&c)).unwrap_or_default();
            if !day.is_empty() && day != current_day {
                current_day = day;
                if let Ok(c) = db.lock() {
                    let rolled = db::rollover_day(&c).unwrap_or(false);
                    // Seal the day that just ended into a complete away ledger.
                    let _ = db::seal_past_days(&c);
                    if rolled {
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
            //
            // But input-idle isn't the same as away: watching a video tutorial or
            // a screencast has no keyboard/mouse for minutes while the person is
            // fully present. So if any media is actively playing we treat that as
            // presence and keep tracking; we only pause on a true away (idle AND
            // nothing playing), e.g. a phone call at the desk. When the media
            // stops and input is still idle, the next tick pauses as usual.
            let idle = idle_flag.exists();
            if idle {
                if !was_idle && !media::any_playing() {
                    was_idle = true;
                    if let Ok(c) = db.lock() {
                        if db::pause_for_idle(&c, idle_secs).unwrap_or(false) {
                            if let Ok(s) = db::snapshot(&c) {
                                let _ = app.emit("snapshot", &s);
                            }
                            // The active task was auto-paused: refresh the list so
                            // the UI drops the "tracking" card instead of freezing.
                            let _ = app.emit("tasks-changed", ());
                        }
                    }
                }
                thread::sleep(TICK);
                continue;
            }
            // Returned from an idle episode: close the open away span at now so
            // the away bucket captures exactly the time the user was gone.
            if was_idle {
                if let Ok(c) = db.lock() {
                    let _ = db::close_open_away(&c, &crate::db::now());
                }
                // Fresh grace period on return: don't greet someone walking back
                // to their desk with a beep they earned while they were gone.
                untracked_since = None;
                untracked_cues = 0;
            }
            was_idle = false;

            // Reconcile the frontend task list whenever the active task changed
            // for a reason the frontend didn't initiate: idle/suspend auto-pause
            // (the suspend guard runs in another thread), estimate pause, or an
            // external edit. Without this the UI keeps a stale "tracking" card
            // with a frozen clock after unlocking / waking. First tick just seeds
            // the value so we don't emit spuriously at startup.
            match last_active_seen {
                Some(prev) if prev != snap.active_task_id => {
                    let _ = app.emit("tasks-changed", ());
                }
                _ => {}
            }
            last_active_seen = Some(snap.active_task_id);

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
            // The "break over" cue plays exactly once when the timer elapses,
            // from here (a native thread) so it lands even if the break window
            // is hidden or on another workspace. The pre-break cue is played by
            // BreakView on mount when the prompt appears.
            if snap.on_break {
                if snap.break_remaining_sec <= 0 && !break_chimed_over {
                    break_chimed_over = true;
                    crate::sound::play("stop_break");
                }
            } else {
                break_chimed_over = false;
            }

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

            // The untracked nudge: beep AND surface, together, as one signal.
            // This runs whether or not the window is already visible, because
            // "visible" usually means "open behind the browser you drifted into"
            // — so we pull it to the front with focus rather than just showing
            // it. A break is deliberate untracked time, so it stays silent.
            if needs_attention && worth_surfacing && !snap.on_break {
                let since = *untracked_since.get_or_insert_with(Instant::now);
                if let Some(&step) = UNTRACKED_CUE_STEPS.get(untracked_cues) {
                    if since.elapsed() >= Duration::from_secs(step) {
                        untracked_cues += 1;
                        let muted = db
                            .lock()
                            .ok()
                            .and_then(|c| db::setting(&c, "sound_muted"))
                            .map(|v| v == "1")
                            .unwrap_or(false);
                        if !muted {
                            crate::sound::play("warning");
                        }
                        window::show_view_front(&app, "nudge");
                        // The popup just happened; don't let the slower
                        // re-nudge clock below fire a second one right after.
                        last_nudge = Some(Instant::now());
                    }
                }
            } else {
                untracked_since = None;
                untracked_cues = 0;
            }

            if !visible && needs_attention && worth_surfacing {
                let due = last_nudge
                    .map(|t| t.elapsed() >= RENUDGE_AFTER)
                    .unwrap_or(true);
                if due {
                    window::show_view(&app, "nudge");
                    last_nudge = Some(Instant::now());
                }
            }

            // At-risk nudge: a big planned task barely started while the day's
            // buffer is already gone (over-committed). Notify regardless of what
            // is currently tracking, checked ~once a minute and fired at most
            // every 30 minutes so it informs without nagging.
            if last_risk_check.map(|t| t.elapsed() >= Duration::from_secs(60)).unwrap_or(true) {
                last_risk_check = Some(Instant::now());
                let buffer = snap.minutes_left_in_day - snap.minutes_committed;
                if snap.minutes_left_in_day > 0 && buffer <= 0 {
                    let worst = db
                        .lock()
                        .ok()
                        .and_then(|c| db::list_tasks(&c).ok())
                        .and_then(|tasks| {
                            tasks
                                .into_iter()
                                .filter(|t| {
                                    matches!(
                                        t.status.as_str(),
                                        "pending" | "paused" | "reopened" | "in_progress" | "awaiting"
                                    )
                                })
                                // Barely started (<10% of a real estimate).
                                .filter(|t| {
                                    let est = t.estimate_min.unwrap_or(0);
                                    est > 0 && t.tracked_min * 10 < est
                                })
                                .max_by_key(|t| t.estimate_min.unwrap_or(0))
                                .map(|t| (t.title, t.estimate_min.unwrap_or(0)))
                        });
                    if let Some((title, est)) = worst {
                        let due = last_risk_notify
                            .map(|t| t.elapsed() >= Duration::from_secs(30 * 60))
                            .unwrap_or(true);
                        if due {
                            last_risk_notify = Some(Instant::now());
                            let est_h = est / 60;
                            let plan = if est_h >= 1 {
                                format!("~{est_h}h planned")
                            } else {
                                format!("~{est}m planned")
                            };
                            notify::send(
                                "A task is slipping",
                                &format!(
                                    "\u{201c}{title}\u{201d} is barely started ({plan}) and today's buffer is gone. Time to switch?"
                                ),
                            );
                        }
                    }
                }
            }

            // Tick faster during a break so the "break over" cue lands within a
            // second of the timer ending instead of up to a full TICK late.
            thread::sleep(if snap.on_break { Duration::from_secs(1) } else { TICK });
        }
    });
}
