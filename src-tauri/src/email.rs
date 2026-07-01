//! Daily summary email via the Brevo transactional API.
//!
//! Once a day (default 08:00 local, covering the previous full day) the engine
//! builds the same dashboard aggregation the History window uses, renders it as
//! a professional HTML digest, and sends it so the user gets an at-a-glance
//! answer to "where did my time go yesterday?" without opening the app.
//!
//! Secrets live in `app_settings` (the data-dir DB, never the repo). They are
//! seeded from environment variables on startup, so the app can be launched once
//! with the Brevo credentials and every later autostart reads them from the DB.

use rusqlite::Connection;
use std::env;
use std::fmt::Write as _;

use crate::db;
use crate::model::{CategoryStat, Dashboard, PauseStat, PlannedActual};

const BREVO_URL: &str = "https://api.brevo.com/v3/smtp/email";
const DEFAULT_FROM: &str = "easyboard@hamidslab.com";
const DEFAULT_FROM_NAME: &str = "Easyboard";

// Palette mirrored from the app so the email reads like the same product.
const INK: &str = "#1d1d1f";
const MUTED: &str = "#86868b";
const FAINT: &str = "#aeaeb2";
const LINE: &str = "#e6e6eb";
const BG: &str = "#f2f2f5";
const ACCENT: &str = "#18a89e";
const UNTRACKED: &str = "#9aa0aa";

/// Everything needed to send one email, assembled while holding the DB lock so
/// the actual HTTP call can happen after the lock is released.
pub struct Payload {
    pub api_key: String,
    pub from: String,
    pub from_name: String,
    pub reply_to: String,
    pub to: String,
    pub subject: String,
    pub html: String,
}

/// Persist Brevo credentials / recipient from env on startup (only when the env
/// var is present and non-empty), then auto-enable once a key + recipient exist.
pub fn seed_from_env(conn: &Connection) {
    let pairs = [
        ("BREVO_API_KEY", "brevo_api_key"),
        ("EMAIL_FROM", "email_from"),
        ("EMAIL_FROM_NAME", "email_from_name"),
        ("EMAIL_REPLY_TO", "email_reply_to"),
        ("EMAIL_TO", "email_to"),
        ("EMAIL_HOUR", "email_hour"),
    ];
    for (var, key) in pairs {
        if let Ok(v) = env::var(var) {
            let v = v.trim();
            if !v.is_empty() {
                let _ = db::put_setting(conn, key, v);
            }
        }
    }
    // Sensible defaults for anything still unset.
    if db::setting(conn, "email_hour").is_none() {
        let _ = db::put_setting(conn, "email_hour", "8");
    }
    if db::setting(conn, "email_offset").is_none() {
        let _ = db::put_setting(conn, "email_offset", "1");
    }
    // Turn it on automatically once configured, unless explicitly disabled.
    let configured = has(conn, "brevo_api_key") && has(conn, "email_to");
    if configured && db::setting(conn, "email_enabled").is_none() {
        let _ = db::put_setting(conn, "email_enabled", "1");
    }
}

fn has(conn: &Connection, key: &str) -> bool {
    db::setting(conn, key).map(|v| !v.trim().is_empty()).unwrap_or(false)
}

/// Cheap gate the engine can call every tick: is a daily summary due right now?
/// The scheduled path waits until the configured hour; the startup catch-up path
/// (`ignore_hour`) does not, so a PC booted after the send hour, or even before
/// it, still gets the day's summary on launch instead of missing it entirely.
fn is_due(conn: &Connection, ignore_hour: bool) -> bool {
    if db::setting(conn, "email_enabled").as_deref() != Some("1") {
        return false;
    }
    if !has(conn, "brevo_api_key") || !has(conn, "email_to") {
        return false;
    }
    if !ignore_hour {
        let hour = db::setting(conn, "email_hour").and_then(|v| v.parse().ok()).unwrap_or(8);
        if db::hour_now(conn) < hour {
            return false;
        }
    }
    // Once per local day.
    let today = db::today(conn);
    db::setting(conn, "email_last_sent").as_deref() != Some(today.as_str())
}

/// Scheduled daily send: due only once the configured hour has passed.
pub fn should_send(conn: &Connection) -> bool {
    is_due(conn, false)
}

/// Startup catch-up: due on launch regardless of the hour, so a late boot still
/// sends today's summary. Used once per boot until the first successful send.
pub fn should_send_on_start(conn: &Connection) -> bool {
    is_due(conn, true)
}

/// Build the payload for the daily summary using the configured day offset
/// (default 1 = yesterday). The caller is responsible for gating on is_due.
pub fn build_due_payload(conn: &Connection) -> Option<Payload> {
    let offset = db::setting(conn, "email_offset").and_then(|v| v.parse().ok()).unwrap_or(1);
    build_payload(conn, offset).ok()
}

/// Build a payload for an arbitrary day offset (0 = today, 1 = yesterday). Used
/// by both the scheduler and the manual "send now" command.
pub fn build_payload(conn: &Connection, offset: i64) -> Result<Payload, String> {
    let api_key = db::setting(conn, "brevo_api_key")
        .filter(|v| !v.trim().is_empty())
        .ok_or("no Brevo API key configured")?;
    let to = db::setting(conn, "email_to")
        .filter(|v| !v.trim().is_empty())
        .ok_or("no recipient configured")?;
    let from = db::setting(conn, "email_from").unwrap_or_else(|| DEFAULT_FROM.into());
    let from_name = db::setting(conn, "email_from_name").unwrap_or_else(|| DEFAULT_FROM_NAME.into());
    let reply_to = db::setting(conn, "email_reply_to").unwrap_or_else(|| from.clone());

    let dash = db::dashboard(conn, "day", offset).map_err(|e| e.to_string())?;
    let subject = format!("Your day · {}", pretty_date(&dash.start_date));
    let html = render_html(&dash);

    Ok(Payload { api_key, from, from_name, reply_to, to, subject, html })
}

/// Mark that today's summary has been sent, so it won't send again until the
/// local day rolls over.
pub fn mark_sent(conn: &Connection) {
    let today = db::today(conn);
    let _ = db::put_setting(conn, "email_last_sent", &today);
}

/// POST the payload to Brevo. Blocking; call off the DB lock.
pub fn send(p: &Payload) -> Result<(), String> {
    let body = serde_json::json!({
        "sender": { "name": p.from_name, "email": p.from },
        "to": [{ "email": p.to }],
        "replyTo": { "email": p.reply_to },
        "subject": p.subject,
        "htmlContent": p.html,
    });
    let resp = ureq::post(BREVO_URL)
        .set("api-key", &p.api_key)
        .set("accept", "application/json")
        .set("content-type", "application/json")
        .send_json(body);
    match resp {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, r)) => {
            let detail = r.into_string().unwrap_or_default();
            Err(format!("Brevo returned {code}: {detail}"))
        }
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_html(d: &Dashboard) -> String {
    let tracked = d.total_tracked_min;
    let untracked = d.distraction_min;
    let focus_pct = {
        let t = d.focus_min + d.distraction_min;
        if t > 0 { (d.focus_min * 100 + t / 2) / t } else { 0 }
    };

    let mut s = String::new();
    let _ = write!(
        s,
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"></head>
<body style="margin:0;padding:0;background:{BG};">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;">Tracked {tracked_lbl} · Focus {focus_pct}% · Untracked {untr_lbl} · {done}/{total} done</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:{BG};padding:24px 12px;">
<tr><td align="center">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="max-width:600px;width:100%;background:#ffffff;border:1px solid {LINE};border-radius:16px;overflow:hidden;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;">
<tr><td style="padding:28px 32px 8px 32px;">
  <div style="font-size:11px;font-weight:700;letter-spacing:.12em;text-transform:uppercase;color:{ACCENT};">Achieve · Daily summary</div>
  <div style="font-size:24px;font-weight:700;color:{INK};margin-top:6px;line-height:1.2;">{date}</div>
  <div style="font-size:14px;color:{MUTED};margin-top:4px;">Here's where your day went.</div>
</td></tr>
"#,
        BG = BG,
        LINE = LINE,
        ACCENT = ACCENT,
        INK = INK,
        MUTED = MUTED,
        tracked_lbl = fmt_min(tracked),
        untr_lbl = fmt_min(untracked),
        focus_pct = focus_pct,
        done = d.completed,
        total = d.total_tasks,
        date = esc(&pretty_date(&d.start_date)),
    );

    // KPI row.
    let _ = write!(s, r#"<tr><td style="padding:16px 24px 4px 24px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr>"#);
    kpi(&mut s, "Tracked", &fmt_min(tracked), ACCENT);
    kpi(&mut s, "Focus", &format!("{focus_pct}%"), INK);
    kpi(&mut s, "Untracked", &fmt_min(untracked), UNTRACKED);
    kpi(&mut s, "Done", &format!("{}/{}", d.completed, d.total_tasks), INK);
    let _ = write!(s, "</tr></table></td></tr>");

    // Categories.
    if !d.by_category.is_empty() {
        section_title(&mut s, "Where your time went");
        let max = d.by_category.iter().map(|c| c.minutes).max().unwrap_or(1).max(1);
        let total: i64 = d.by_category.iter().map(|c| c.minutes).sum::<i64>().max(1);
        let _ = write!(s, r#"<tr><td style="padding:0 32px 8px 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0">"#);
        for c in &d.by_category {
            bar_row(&mut s, &c.name, cat_color(c), c.minutes, max, total, false);
        }
        let _ = write!(s, "</table></td></tr>");
    }

    // Tasks.
    let real_tasks: Vec<&PlannedActual> = d.planned_actual.iter().filter(|p| !p.untracked && p.tracked_min > 0).collect();
    if !real_tasks.is_empty() {
        section_title(&mut s, "Tasks");
        let _ = write!(s, r#"<tr><td style="padding:0 32px 4px 32px;">"#);
        for p in real_tasks {
            task_block(&mut s, p);
        }
        let _ = write!(s, "</td></tr>");
    }

    // Applications.
    if !d.by_app.is_empty() {
        section_title(&mut s, "Applications");
        let max = d.by_app.iter().map(|a| a.minutes).max().unwrap_or(1).max(1);
        let total: i64 = d.by_app.iter().map(|a| a.minutes).sum::<i64>().max(1);
        let _ = write!(s, r#"<tr><td style="padding:0 32px 8px 32px;"><table role="presentation" width="100%" cellpadding="0" cellspacing="0">"#);
        for a in d.by_app.iter().take(8) {
            bar_row(&mut s, &app_name(&a.app), ACCENT, a.minutes, max, total, true);
        }
        let _ = write!(s, "</table></td></tr>");
    }

    let _ = write!(
        s,
        r#"<tr><td style="padding:20px 32px 28px 32px;border-top:1px solid {LINE};">
  <div style="font-size:12px;color:{FAINT};">Sent by Achieve, your proactive day-companion. Totals are final for the full day.</div>
</td></tr>
</table></td></tr></table></body></html>"#,
        LINE = LINE,
        FAINT = FAINT,
    );
    s
}

fn kpi(s: &mut String, label: &str, value: &str, color: &str) {
    let _ = write!(
        s,
        r#"<td width="25%" style="padding:8px 6px;" valign="top">
  <div style="background:#f7f7f9;border:1px solid {LINE};border-radius:10px;padding:12px 10px;">
    <div style="font-size:10px;font-weight:700;letter-spacing:.06em;text-transform:uppercase;color:{MUTED};">{label}</div>
    <div style="font-size:20px;font-weight:700;color:{color};margin-top:4px;line-height:1;">{value}</div>
  </div>
</td>"#,
        LINE = LINE,
        MUTED = MUTED,
        color = color,
        label = esc(label),
        value = esc(value),
    );
}

fn section_title(s: &mut String, title: &str) {
    let _ = write!(
        s,
        r#"<tr><td style="padding:18px 32px 8px 32px;"><div style="font-size:11px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;color:{MUTED};">{title}</div></td></tr>"#,
        MUTED = MUTED,
        title = esc(title),
    );
}

fn bar_row(s: &mut String, name: &str, color: &str, minutes: i64, max: i64, total: i64, small_dot: bool) {
    let fill = ((minutes.max(0) * 100) / max).clamp(2, 100);
    let pct = (minutes.max(0) * 100 + total / 2) / total;
    let dot = if small_dot { 8 } else { 10 };
    let _ = write!(
        s,
        r#"<tr>
  <td width="16" valign="middle" style="padding:5px 0;"><div style="width:{dot}px;height:{dot}px;border-radius:50%;background:{color};"></div></td>
  <td valign="middle" style="padding:5px 8px;font-size:13px;color:{INK};">{name}</td>
  <td width="120" valign="middle" style="padding:5px 0;">
    <div style="background:#ececef;border-radius:4px;height:6px;width:100%;"><div style="background:{color};height:6px;border-radius:4px;width:{fill}%;"></div></div>
  </td>
  <td width="52" align="right" valign="middle" style="padding:5px 0 5px 10px;font-size:12px;color:{MUTED};font-variant-numeric:tabular-nums;">{time}</td>
  <td width="34" align="right" valign="middle" style="padding:5px 0 5px 6px;font-size:12px;color:{FAINT};font-variant-numeric:tabular-nums;">{pct}%</td>
</tr>"#,
        dot = dot,
        color = color,
        INK = INK,
        MUTED = MUTED,
        FAINT = FAINT,
        name = esc(name),
        fill = fill,
        time = esc(&fmt_min(minutes)),
        pct = pct,
    );
}

fn task_block(s: &mut String, p: &PlannedActual) {
    let est = if p.estimate_min > 0 {
        format!(" of ~{}", fmt_min(p.estimate_min))
    } else {
        String::new()
    };
    let done_badge = if p.done {
        r#"<span style="display:inline-block;font-size:10px;font-weight:700;color:#1f9d55;background:#e8f7ee;border-radius:999px;padding:2px 7px;margin-left:6px;">DONE</span>"#.to_string()
    } else {
        String::new()
    };
    // Solid colors only (8-digit hex alpha renders inconsistently in email):
    // category-colored text + border on a neutral light tint.
    let cat_badge = if !p.category.trim().is_empty() {
        let c = if p.color.trim().is_empty() { UNTRACKED } else { p.color.trim() };
        format!(
            r#"<span style="display:inline-block;font-size:10px;font-weight:700;color:{c};background:#f4f4f6;border:1px solid {c};border-radius:999px;padding:2px 8px;">{name}</span>"#,
            c = c,
            name = esc(&p.category),
        )
    } else {
        String::new()
    };

    let _ = write!(
        s,
        r#"<div style="border:1px solid {LINE};border-radius:12px;padding:12px 14px;margin:8px 0;">
  <div style="font-size:14px;font-weight:600;color:{INK};line-height:1.3;">{title}{done}</div>
  <div style="margin-top:6px;font-size:12px;color:{MUTED};">{cat}<span style="margin-left:{catsp}">{time} tracked{est}</span></div>"#,
        LINE = LINE,
        INK = INK,
        MUTED = MUTED,
        title = esc(&p.title),
        done = done_badge,
        cat = cat_badge,
        catsp = if cat_badge_present(p) { "8px" } else { "0" },
        time = esc(&fmt_min(p.tracked_min)),
        est = esc(&est),
    );

    if !p.apps.is_empty() {
        let apps: Vec<String> = p
            .apps
            .iter()
            .take(4)
            .map(|a| format!("{} {}", esc(&app_name(&a.app)), esc(&fmt_min(a.minutes))))
            .collect();
        let _ = write!(
            s,
            r#"<div style="margin-top:8px;font-size:12px;color:{MUTED};"><span style="color:{FAINT};">Apps:</span> {list}</div>"#,
            MUTED = MUTED,
            FAINT = FAINT,
            list = apps.join(" &nbsp;·&nbsp; "),
        );
    }

    if let Some(summary) = pause_summary(&p.pauses) {
        let _ = write!(
            s,
            r#"<div style="margin-top:6px;font-size:12px;color:{MUTED};"><span style="color:{FAINT};">Pauses:</span> {summary}</div>"#,
            MUTED = MUTED,
            FAINT = FAINT,
            summary = summary,
        );
    }

    let _ = write!(s, "</div>");
}

fn cat_badge_present(p: &PlannedActual) -> bool {
    !p.category.trim().is_empty()
}

/// "3 pauses · going for dinner, reached estimate ×6" (user notes first).
fn pause_summary(pauses: &[PauseStat]) -> Option<String> {
    if pauses.is_empty() {
        return None;
    }
    let total: i64 = pauses.iter().map(|p| p.count).sum();
    let parts: Vec<String> = pauses
        .iter()
        .map(|p| {
            let label = pause_label(&p.reason, p.auto);
            if p.count > 1 {
                format!("{} ×{}", esc(&label), p.count)
            } else {
                esc(&label)
            }
        })
        .collect();
    let noun = if total == 1 { "pause" } else { "pauses" };
    Some(format!("{total} {noun} · {}", parts.join(", ")))
}

fn pause_label(reason: &str, auto: bool) -> String {
    if !auto {
        return reason.to_string();
    }
    match reason {
        "auto-idle" => "Went idle",
        "auto-suspend" => "System asleep",
        "day-rollover" => "Rolled to next day",
        "reached-estimate" => "Reached estimate",
        "rescheduled" => "Rescheduled",
        "deleted" => "Deleted",
        "break-start" => "Break started",
        "break-end" => "Break ended",
        "capped-suspend" => "Capped after suspend",
        "capped-runaway" => "Capped runaway timer",
        other => other,
    }
    .to_string()
}

fn cat_color(c: &CategoryStat) -> &str {
    if c.color.trim().is_empty() {
        UNTRACKED
    } else {
        c.color.trim()
    }
}

fn fmt_min(min: i64) -> String {
    let m = min.max(0);
    let h = m / 60;
    let r = m % 60;
    if h > 0 {
        if r > 0 {
            format!("{h}h {r}m")
        } else {
            format!("{h}h")
        }
    } else {
        format!("{r}m")
    }
}

/// Turn "2026-06-30" into "Tuesday, June 30".
fn pretty_date(ymd: &str) -> String {
    let parts: Vec<i64> = ymd.split('-').filter_map(|p| p.parse().ok()).collect();
    if parts.len() != 3 {
        return ymd.to_string();
    }
    let (y, m, day) = (parts[0], parts[1], parts[2]);
    // Zeller-ish weekday via a fixed epoch (days since 2000-01-01, a Saturday).
    let months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let weekdays = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    let dow = day_of_week(y, m, day);
    let mon = months.get((m - 1).clamp(0, 11) as usize).copied().unwrap_or("");
    let wd = weekdays.get(dow as usize).copied().unwrap_or("");
    format!("{wd}, {mon} {day}")
}

/// Sakamoto's algorithm: 0 = Sunday .. 6 = Saturday.
fn day_of_week(y: i64, m: i64, d: i64) -> i64 {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let idx = (m - 1).clamp(0, 11) as usize;
    ((y + y / 4 - y / 100 + y / 400 + t[idx] + d) % 7 + 7) % 7
}

fn app_name(app_id: &str) -> String {
    let key = app_id.trim();
    let mapped = match key.to_lowercase().as_str() {
        "firefox" | "org.mozilla.firefox" | "firefox-esr" => Some("Firefox"),
        "zen" => Some("Zen Browser"),
        "google-chrome" | "google-chrome-stable" => Some("Chrome"),
        "chromium" => Some("Chromium"),
        "brave-browser" => Some("Brave"),
        "code" | "code-oss" | "code-url-handler" => Some("VS Code"),
        "cursor" => Some("Cursor"),
        "kitty" => Some("Kitty"),
        "alacritty" => Some("Alacritty"),
        "foot" => Some("Foot"),
        "ghostty" | "com.mitchellh.ghostty" => Some("Ghostty"),
        "slack" | "com.slack.slack" => Some("Slack"),
        "discord" => Some("Discord"),
        "spotify" => Some("Spotify"),
        "obsidian" | "md.obsidian.obsidian" => Some("Obsidian"),
        "zoom" => Some("Zoom"),
        "thunderbird" => Some("Thunderbird"),
        _ => None,
    };
    if let Some(m) = mapped {
        return m.to_string();
    }
    let last = key.rsplit('.').next().unwrap_or(key);
    let cleaned = last.replace(['-', '_'], " ");
    cleaned
        .split_whitespace()
        .map(|w| {
            let mut ch = w.chars();
            match ch.next() {
                Some(f) => f.to_uppercase().collect::<String>() + ch.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Minimal HTML escaping for user-supplied text (task titles, pause notes).
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
