//! Automatic activity capture: the scientific ground truth of where time goes.
//!
//! On niri we subscribe to `niri msg --json event-stream`, which emits one JSON
//! object per line. We track the window map and, whenever focus changes, write
//! an append-only row into `focus_log` (closing the previous open row). Later
//! (Phase 2) the reconciliation popup labels each focus span as work or
//! distraction and maps it onto a task.
//!
//! Idle detection (away-from-keyboard) needs the Wayland ext-idle-notify
//! protocol; that lands in Phase 2. See `note_idle_todo` below.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;

/// Spawn the focus tracker if we are on a supported compositor.
pub fn spawn_focus_tracker(db: Arc<Mutex<Connection>>) {
    let compositor = detect_compositor();
    match compositor {
        Compositor::Niri => {
            thread::spawn(move || {
                if let Err(e) = run_niri_stream(db) {
                    eprintln!("[achieve] niri focus tracker stopped: {e}");
                }
            });
        }
        other => {
            // Phase 2 will add sway/hyprland IPC and a GNOME/KDE fallback.
            eprintln!(
                "[achieve] focus tracking not yet implemented for {other:?}; \
                 time will be self-reported only for now."
            );
        }
    }
}

#[derive(Debug)]
enum Compositor {
    Niri,
    Sway,
    Hyprland,
    Other,
}

fn detect_compositor() -> Compositor {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let d = desktop.to_lowercase();
    if d.contains("niri") || std::env::var("NIRI_SOCKET").is_ok() {
        Compositor::Niri
    } else if std::env::var("SWAYSOCK").is_ok() {
        Compositor::Sway
    } else if std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok() {
        Compositor::Hyprland
    } else {
        Compositor::Other
    }
}

fn now() -> String {
    // Match db.rs: UTC "YYYY-MM-DD HH:MM:SS" so SQLite date()/julianday() parse.
    Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn run_niri_stream(db: Arc<Mutex<Connection>>) -> anyhow::Result<()> {
    let mut cmd = Command::new("niri");
    cmd.args(["msg", "--json", "event-stream"])
        .stdout(Stdio::piped());

    // Ask the kernel to kill this child if the achieve process dies, so the
    // event-stream subprocess never orphans (and never keeps inherited fds).
    #[cfg(target_os = "linux")]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout from niri"))?;
    let reader = BufReader::new(stdout);

    // id -> (app_id, title)
    let mut windows: HashMap<u64, (Option<String>, Option<String>)> = HashMap::new();
    let mut focused: Option<u64> = None;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Each event is { "VariantName": payload }.
        let (event, payload) = match v.as_object().and_then(|o| o.iter().next()) {
            Some((k, p)) => (k.as_str(), p),
            None => continue,
        };

        match event {
            "WindowsChanged" => {
                if let Some(arr) = payload.get("windows").and_then(|w| w.as_array()) {
                    for w in arr {
                        ingest_window(&mut windows, w);
                        if w.get("is_focused").and_then(|b| b.as_bool()) == Some(true) {
                            focused = w.get("id").and_then(|i| i.as_u64());
                        }
                    }
                    if let Some(id) = focused {
                        record_focus(&db, windows.get(&id));
                    }
                }
            }
            "WindowOpenedOrChanged" => {
                if let Some(w) = payload.get("window") {
                    ingest_window(&mut windows, w);
                    if w.get("is_focused").and_then(|b| b.as_bool()) == Some(true) {
                        let id = w.get("id").and_then(|i| i.as_u64());
                        if id != focused {
                            focused = id;
                            if let Some(id) = id {
                                record_focus(&db, windows.get(&id));
                            }
                        }
                    }
                }
            }
            "WindowFocusChanged" => {
                let id = payload.get("id").and_then(|i| i.as_u64());
                focused = id;
                match id {
                    Some(id) => record_focus(&db, windows.get(&id)),
                    None => record_focus(&db, None), // focus left all windows
                }
            }
            "WindowClosed" => {
                if let Some(id) = payload.get("id").and_then(|i| i.as_u64()) {
                    windows.remove(&id);
                }
            }
            _ => {}
        }
    }

    let _ = child.wait();
    Ok(())
}

fn ingest_window(
    windows: &mut HashMap<u64, (Option<String>, Option<String>)>,
    w: &Value,
) {
    if let Some(id) = w.get("id").and_then(|i| i.as_u64()) {
        let app_id = w
            .get("app_id")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        let title = w
            .get("title")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());
        windows.insert(id, (app_id, title));
    }
}

/// Close the previous open focus row and open a new one for the now-focused win.
fn record_focus(
    db: &Arc<Mutex<Connection>>,
    win: Option<&(Option<String>, Option<String>)>,
) {
    let conn = match db.lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    let ts = now();
    let _ = conn.execute(
        "UPDATE focus_log SET end_at = ?1 WHERE end_at IS NULL",
        params![ts],
    );
    let (app_id, title) = match win {
        Some((a, t)) => (a.clone(), t.clone()),
        None => (None, None),
    };
    // Auto-attribute this span to whatever task is currently being tracked, so
    // the dashboard can show exactly which apps were used during that task.
    let active_task: Option<i64> = conn
        .query_row(
            "SELECT id FROM tasks WHERE status='in_progress' LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    let _ = conn.execute(
        "INSERT INTO focus_log (app_id, title, start_at, task_id) VALUES (?1, ?2, ?3, ?4)",
        params![app_id, title, ts, active_task],
    );
}

// Phase 2: Wayland ext-idle-notify based idle detection lands here. On idle we
// close the open focus row; on return we open a reconciliation prompt asking
// what the away time was.
#[allow(dead_code)]
fn note_idle_todo() {}
