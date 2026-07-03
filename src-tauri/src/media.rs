//! "Is the user actually engaged, just not typing?" detection.
//!
//! Input-idle alone can't tell a phone call away from the desk (truly away)
//! apart from watching a video tutorial or a screencast (present, but no
//! keyboard/mouse for minutes). We treat active media playback as presence: if
//! any MPRIS player reports `PlaybackStatus == "Playing"`, the person is
//! engaged, so we don't auto-pause their task even though input is idle.
//!
//! Best-effort over the session D-Bus; any failure (no bus, no player) is
//! simply "no media", so behaviour falls back to plain input-idle.

/// True if any MPRIS media player is currently Playing.
pub fn any_playing() -> bool {
    any_playing_inner().unwrap_or(false)
}

fn any_playing_inner() -> zbus::Result<bool> {
    use zbus::blocking::{Connection, Proxy};

    let bus = Connection::session()?;
    let dbus = Proxy::new(
        &bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;

    let names: Vec<String> = dbus.call("ListNames", &())?;
    for name in names.iter().filter(|n| n.starts_with("org.mpris.MediaPlayer2.")) {
        let player = match Proxy::new(
            &bus,
            name.as_str(),
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        ) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if let Ok(status) = player.get_property::<String>("PlaybackStatus") {
            if status == "Playing" {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
