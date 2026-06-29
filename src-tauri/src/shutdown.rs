//! Best-effort shutdown awareness.
//!
//! We listen for logind's `PrepareForShutdown(true)` D-Bus signal and, when the
//! machine is about to power off, flush the ledger by closing any open work and
//! focus segments at the current time. This is the tertiary safety net: the
//! evening check-in is the real wind-down, and `db::recover()` on next launch
//! also closes dangling segments. If the system bus or logind is unavailable we
//! simply skip, never crashing the app.

use std::sync::{Arc, Mutex};
use std::thread;

use rusqlite::Connection;

pub fn spawn_shutdown_guard(db: Arc<Mutex<Connection>>) {
    thread::spawn(move || {
        if let Err(e) = run(db) {
            eprintln!("[achieve] shutdown guard unavailable: {e}");
        }
    });
}

/// Suspend awareness: logind emits `PrepareForSleep(true)` just BEFORE the
/// machine suspends (and `(false)` on resume). Catching the `true` edge lets us
/// flush the ledger at the real moment the machine froze, so a task left
/// "tracking" while the laptop sleeps overnight never counts the hours the user
/// was away. swayidle can't help here: it is frozen along with everything else
/// during suspend, so the idle timeout never fires. We cap open work + focus
/// spans at now and pause the active task; on resume nothing is accruing.
pub fn spawn_sleep_guard(db: Arc<Mutex<Connection>>) {
    thread::spawn(move || {
        if let Err(e) = run_sleep(db) {
            eprintln!("[achieve] sleep guard unavailable: {e}");
        }
    });
}

fn run_sleep(db: Arc<Mutex<Connection>>) -> zbus::Result<()> {
    use zbus::blocking::{Connection as Bus, Proxy};

    let bus = Bus::system()?;
    let proxy = Proxy::new(
        &bus,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;

    for msg in proxy.receive_signal("PrepareForSleep")? {
        let body = msg.body();
        if let Ok(true) = body.deserialize::<bool>() {
            if let Ok(conn) = db.lock() {
                let _ = conn.execute(
                    "UPDATE segments SET end_at = strftime('%Y-%m-%d %H:%M:%S','now'),
                            reason = COALESCE(reason,'auto-suspend') WHERE end_at IS NULL",
                    [],
                );
                let _ = conn.execute(
                    "UPDATE focus_log SET end_at = strftime('%Y-%m-%d %H:%M:%S','now') WHERE end_at IS NULL",
                    [],
                );
                let _ = conn.execute(
                    "UPDATE tasks SET status='paused',
                            updated_at=strftime('%Y-%m-%d %H:%M:%S','now') WHERE status='in_progress'",
                    [],
                );
            }
        }
    }
    Ok(())
}

fn run(db: Arc<Mutex<Connection>>) -> zbus::Result<()> {
    use zbus::blocking::{Connection as Bus, Proxy};

    let bus = Bus::system()?;
    let proxy = Proxy::new(
        &bus,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;

    for msg in proxy.receive_signal("PrepareForShutdown")? {
        let body = msg.body();
        if let Ok(true) = body.deserialize::<bool>() {
            if let Ok(conn) = db.lock() {
                let _ = conn.execute(
                    "UPDATE segments SET end_at = strftime('%Y-%m-%d %H:%M:%S','now') WHERE end_at IS NULL",
                    [],
                );
                let _ = conn.execute(
                    "UPDATE focus_log SET end_at = strftime('%Y-%m-%d %H:%M:%S','now') WHERE end_at IS NULL",
                    [],
                );
            }
        }
    }
    Ok(())
}
