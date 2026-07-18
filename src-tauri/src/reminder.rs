//! Task reminder dispatch.
//!
//! Reminders are the app's proactive "don't forget" layer, delivered by email
//! (through the personal-email-worker) and/or a desktop notification. The engine
//! calls [`dispatch`] on a throttle; it does the HTTP off the DB lock.
//!
//! Scheduling strategy (the worker only accepts `scheduledAt` up to 72h ahead):
//!
//! - **Within ~2 min (or overdue):** send immediately. A recurring reminder that
//!   was missed while the app was closed pings once, then resumes on schedule
//!   (never a burst of catch-up mails).
//! - **2 min .. 72h ahead:** hand the email to the worker with `scheduledAt`, so
//!   it fires even if the app is closed. We keep the returned messageId to cancel
//!   it if the reminder is edited/deleted.
//! - **> 72h ahead:** leave it pending; a later pass schedules it once it enters
//!   the 72h window.
//!
//! Desktop notifications can only fire while the app is running, so they are sent
//! locally when the reminder's time arrives (including right after the worker
//! delivered the email, for the "both" channel).

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::{self, ReminderJob};
use crate::email;
use crate::notify;

/// Run one reminder dispatch pass. Cheap when nothing is due.
pub fn dispatch(db: &Arc<Mutex<Connection>>) {
    // Snapshot everything we need under one short lock, then work unlocked.
    let (jobs, ident, url, key, now, soon) = {
        let c = match db.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        let jobs = db::due_reminder_jobs(&c).unwrap_or_default();
        if jobs.is_empty() {
            return;
        }
        let (url, key) = email::worker_creds(&c);
        let now: String = c
            .query_row("SELECT datetime('now')", [], |r| r.get(0))
            .unwrap_or_default();
        let soon: String = c
            .query_row("SELECT datetime('now','+120 seconds')", [], |r| r.get(0))
            .unwrap_or_default();
        (jobs, email::identity(&c), url, key, now, soon)
    };

    for job in jobs {
        process(db, &job, ident.as_ref(), &url, &key, &now, &soon);
    }
}

type Identity = (String, String, String, String); // from, from_name, reply_to, to

fn process(
    db: &Arc<Mutex<Connection>>,
    job: &ReminderJob,
    ident: Option<&Identity>,
    url: &str,
    key: &str,
    now: &str,
    soon: &str,
) {
    // A cancelled reminder that still holds a worker schedule: tear it down.
    if job.status == "cancelled" {
        if let Some(mid) = &job.message_id {
            let _ = email::worker_cancel(url, key, mid);
        }
        with_lock(db, |c| db::clear_reminder_message(c, job.id));
        return;
    }

    // The task went away (deleted, or a one-off that's now done): cancel the
    // reminder, and any worker schedule with it. Recurring-task reminders live on.
    let task_gone = job.task_status == "deleted"
        || (job.task_status == "completed" && job.task_recurrence.as_deref() != Some("daily"));
    if task_gone {
        if let Some(mid) = &job.message_id {
            let _ = email::worker_cancel(url, key, mid);
        }
        with_lock(db, |c| db::cancel_reminder(c, job.id));
        return;
    }

    let wants_email = job.channel != "notification";
    let wants_notif = job.channel != "email";
    let overdue = job.remind_at.as_str() <= now;
    let due_soon = job.remind_at.as_str() <= soon;

    // Already-scheduled reminder whose time has passed: the worker delivered the
    // email at scheduledAt. Fire the local notification (if wanted) and advance.
    if job.status == "scheduled" {
        if !overdue {
            return; // shouldn't be fetched, but be safe
        }
        if wants_notif {
            fire_notification(job);
        }
        advance_or_complete(db, job);
        return;
    }

    // pending / failed.
    if due_soon {
        // Fire now.
        let mut email_ok = true;
        if wants_email {
            match ident {
                Some(id) => {
                    let (subject, html) = render(job, id);
                    let (from, from_name, reply_to, to) = id;
                    let msg = email::Message {
                        from,
                        from_name,
                        reply_to,
                        to,
                        subject: &subject,
                        html: &html,
                        scheduled_at: None,
                    };
                    if let Err(e) = email::worker_send(url, key, &msg) {
                        email_ok = false;
                        eprintln!("[reminder] send failed (id {}): {e}", job.id);
                        with_lock(db, |c| db::mark_reminder_failed(c, job.id, &e));
                    }
                }
                None => {
                    // No recipient configured: can't email. Don't loop on it.
                    eprintln!("[reminder] no recipient configured; skipping email for id {}", job.id);
                }
            }
        }
        if wants_notif {
            fire_notification(job);
        }
        if email_ok {
            advance_or_complete(db, job);
        }
        return;
    }

    // In the future but within 72h: schedule the email on the worker. (Pure
    // notification reminders can't be scheduled server-side; they fire locally
    // when due, so leave them pending.)
    if wants_email {
        let Some(id) = ident else {
            return; // no recipient; leave pending
        };
        let iso = match with_lock_val(db, |c| db::to_iso8601(c, &job.remind_at)) {
            Some(Some(s)) => s,
            _ => return,
        };
        let (subject, html) = render(job, id);
        let (from, from_name, reply_to, to) = id;
        let msg = email::Message {
            from,
            from_name,
            reply_to,
            to,
            subject: &subject,
            html: &html,
            scheduled_at: Some(&iso),
        };
        match email::worker_send(url, key, &msg) {
            Ok(mid) => {
                with_lock(db, |c| db::mark_reminder_scheduled(c, job.id, &mid));
                println!("[reminder] scheduled id {} for {}", job.id, job.remind_at_local);
            }
            Err(e) => {
                eprintln!("[reminder] schedule failed (id {}): {e}", job.id);
                with_lock(db, |c| db::mark_reminder_failed(c, job.id, &e));
            }
        }
    }
}

/// Advance a recurring reminder to its next slot, or mark a one-shot done.
fn advance_or_complete(db: &Arc<Mutex<Connection>>, job: &ReminderJob) {
    with_lock(db, |c| {
        let next = match job.rrule.as_deref() {
            Some(rule) if !rule.is_empty() => {
                db::next_occurrence(c, &job.remind_at, rule, job.rrule_until.as_deref(), job.rrule_count)
            }
            _ => None,
        };
        match next {
            Some((next_utc, remaining)) => db::advance_reminder(c, job.id, &next_utc, remaining),
            None => db::mark_reminder_sent(c, job.id),
        }
    });
}

fn fire_notification(job: &ReminderJob) {
    let body = match job.note.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => n.to_string(),
        None => "Reminder".to_string(),
    };
    notify::send(&format!("⏰ {}", job.task_title), &body);
}

/// (subject, html) for a reminder email. Mirrors the digest's calm palette.
fn render(job: &ReminderJob, ident: &Identity) -> (String, String) {
    let _ = ident;
    let title = esc(&job.task_title);
    let subject = format!("⏰ Reminder: {}", job.task_title);
    let when = pretty_when(&job.remind_at_local);
    let accent = job
        .category_color
        .clone()
        .unwrap_or_else(|| "#18a89e".to_string());

    let note_html = match job.note.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(n) => format!(
            r#"<p style="margin:14px 0 0;font-size:14px;line-height:1.5;color:#3a3a3c;">{}</p>"#,
            esc(n)
        ),
        None => String::new(),
    };
    let cat_html = match &job.category_name {
        Some(c) if !c.is_empty() => format!(
            r#"<span style="display:inline-block;margin-top:12px;padding:3px 10px;border-radius:999px;background:{accent}1a;color:{accent};font-size:12px;font-weight:600;">{}</span>"#,
            esc(c)
        ),
        _ => String::new(),
    };
    let repeat_html = match summarize_rrule(job.rrule.as_deref()) {
        Some(s) => format!(
            r#"<p style="margin:6px 0 0;font-size:12px;color:#8a8a8e;">Repeats {}</p>"#,
            esc(&s)
        ),
        None => String::new(),
    };

    let html = format!(
        r#"<!doctype html><html><body style="margin:0;padding:0;background:#f2f2f5;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f2f2f5;padding:28px 12px;">
<tr><td align="center">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:460px;background:#ffffff;border-radius:16px;overflow:hidden;box-shadow:0 1px 3px rgba(0,0,0,0.06);">
  <tr><td style="height:4px;background:{accent};"></td></tr>
  <tr><td style="padding:26px 28px 28px;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;">
    <p style="margin:0 0 4px;font-size:12px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;color:{accent};">Reminder</p>
    <h1 style="margin:0;font-size:22px;line-height:1.25;font-weight:700;color:#1d1d1f;">{title}</h1>
    <p style="margin:10px 0 0;font-size:14px;font-weight:600;color:#1d1d1f;">{when}</p>
    {repeat_html}
    {cat_html}
    {note_html}
    <hr style="border:none;border-top:1px solid #e6e6eb;margin:22px 0 0;">
    <p style="margin:16px 0 0;font-size:12px;color:#aeaeb2;">Sent by Achieve, your day companion.</p>
  </td></tr>
</table>
</td></tr></table>
</body></html>"#
    );
    (subject, html)
}

/// "Mon, Jul 20 at 2:30 PM" from a local "YYYY-MM-DD HH:MM".
fn pretty_when(local: &str) -> String {
    let (date, time) = match local.split_once(' ') {
        Some(p) => p,
        None => return esc(local),
    };
    let dparts: Vec<&str> = date.split('-').collect();
    let (y, m, d) = match (
        dparts.first().and_then(|s| s.parse::<i32>().ok()),
        dparts.get(1).and_then(|s| s.parse::<u32>().ok()),
        dparts.get(2).and_then(|s| s.parse::<u32>().ok()),
    ) {
        (Some(y), Some(m), Some(d)) => (y, m, d),
        _ => return esc(local),
    };
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mon = months.get((m.saturating_sub(1)) as usize).copied().unwrap_or("");
    let (h, min) = match time.split_once(':') {
        Some((h, mi)) => (h.parse::<u32>().unwrap_or(0), mi),
        None => (0, "00"),
    };
    let (h12, ampm) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    let _ = y;
    format!("{mon} {d} at {h12}:{min} {ampm}")
}

/// Human phrase for a stored rrule, or None for one-shot.
pub fn summarize_rrule(rrule: Option<&str>) -> Option<String> {
    let r = rrule?;
    if r.is_empty() {
        return None;
    }
    let s = match r {
        "daily" => "every day".to_string(),
        "weekdays" => "every weekday".to_string(),
        "weekly" => "every week".to_string(),
        "biweekly" => "every 2 weeks".to_string(),
        "monthly" => "every month".to_string(),
        "yearly" => "every year".to_string(),
        s if s.starts_with("every:") => {
            let p: Vec<&str> = s.split(':').collect();
            let n: i64 = p.get(1).and_then(|v| v.parse().ok()).unwrap_or(1);
            let unit = p.get(2).copied().unwrap_or("days");
            let unit_s = match (unit, n) {
                ("days", 1) => "day",
                ("days", _) => "days",
                ("weeks", 1) => "week",
                ("weeks", _) => "weeks",
                ("months", 1) => "month",
                ("months", _) => "months",
                _ => unit,
            };
            if n == 1 {
                format!("every {unit_s}")
            } else {
                format!("every {n} {unit_s}")
            }
        }
        other => other.to_string(),
    };
    Some(s)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn with_lock<E: std::fmt::Display, F: FnOnce(&Connection) -> Result<(), E>>(
    db: &Arc<Mutex<Connection>>,
    f: F,
) {
    if let Ok(c) = db.lock() {
        if let Err(e) = f(&c) {
            eprintln!("[reminder] db update failed: {e}");
        }
    }
}

fn with_lock_val<T, F: FnOnce(&Connection) -> T>(db: &Arc<Mutex<Connection>>, f: F) -> Option<T> {
    db.lock().ok().map(|c| f(&c))
}
