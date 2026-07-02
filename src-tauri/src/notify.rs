//! Best-effort desktop notifications via the freedesktop D-Bus service, used to
//! nudge when a planned task is slipping while the day's buffer is already gone.
//! Runs off-thread and never panics: if the session bus or a notification
//! daemon is unavailable we simply skip.

/// Fire a desktop notification (fire-and-forget).
pub fn send(summary: &str, body: &str) {
    let summary = summary.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        if let Err(e) = notify(&summary, &body) {
            eprintln!("[notify] could not send notification: {e}");
        }
    });
}

fn notify(summary: &str, body: &str) -> zbus::Result<()> {
    use std::collections::HashMap;
    use zbus::blocking::{Connection as Bus, Proxy};
    use zbus::zvariant::Value;

    let bus = Bus::session()?;
    let proxy = Proxy::new(
        &bus,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )?;

    let actions: Vec<&str> = Vec::new();
    let mut hints: HashMap<&str, Value> = HashMap::new();
    // urgency = critical (2), so it isn't silently collapsed while you work.
    hints.insert("urgency", Value::U8(2));

    let _id: u32 = proxy.call(
        "Notify",
        &(
            "Achieve",           // app_name
            0u32,                // replaces_id (0 = new)
            "appointment-soon",  // app_icon (freedesktop name; degrades gracefully)
            summary,             // summary
            body,                // body
            actions,             // actions
            hints,               // hints
            10_000i32,           // expire_timeout (ms)
        ),
    )?;
    Ok(())
}
