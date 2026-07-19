//! "Is the user actually engaged, just not typing?" detection.
//!
//! Input-idle alone can't tell a phone call away from the desk (truly away)
//! apart from watching a match or a screencast (present, but no keyboard/mouse
//! for a long time). We treat active audio playback as presence, so we don't
//! auto-pause a task while the person is clearly still there.
//!
//! Ground truth is PipeWire, not MPRIS. MPRIS only sees players that bother to
//! register on the session bus: browsers and desktop music players do, but
//! **Waydroid (Android apps), games, Wine, and most streaming apps do not** —
//! which is exactly how a two-hour match watched in an Android streaming app got
//! its task auto-paused and ten minutes of it retroactively erased. PipeWire
//! sees every stream that reaches the speakers, so we ask it whether any output
//! stream is `running` (feeding samples) rather than `idle`. MPRIS is kept as a
//! secondary signal for the rare player that reports Playing while silent.
//!
//! Best-effort throughout: any failure (no pw-dump, no bus, no player) is simply
//! "no media", so behaviour falls back to plain input-idle.

use std::process::Command;

use serde_json::Value;

/// True if the user appears engaged with media right now.
pub fn any_playing() -> bool {
    audio_running().unwrap_or(false) || mpris_playing()
}

/// True if any application is actively feeding audio to the speakers.
///
/// Our OWN cue playback is excluded: the app beeping at you is not evidence
/// that you are still at your desk.
fn audio_running() -> Option<bool> {
    let out = Command::new("pw-dump").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let objects: Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(any_stream_running(&objects))
}

/// The parsing half, split out so it can be tested against real `pw-dump` shapes.
fn any_stream_running(objects: &Value) -> bool {
    let arr = match objects.as_array() {
        Some(a) => a,
        None => return false,
    };
    for o in arr {
        let info = match o.get("info") {
            Some(i) => i,
            None => continue,
        };
        let props = match info.get("props") {
            Some(p) => p,
            None => continue,
        };
        let str_prop = |k: &str| props.get(k).and_then(|v| v.as_str()).unwrap_or("");
        if !str_prop("media.class").starts_with("Stream/Output/Audio") {
            continue;
        }
        // "running" = actually pushing samples; "idle"/"suspended" = silent.
        if info.get("state").and_then(|v| v.as_str()) != Some("running") {
            continue;
        }
        if is_self(str_prop("application.name"))
            || is_self(str_prop("application.process.binary"))
            || is_self(str_prop("node.name"))
        {
            continue;
        }
        return true;
    }
    false
}

/// Our own audio, under any of the names rodio/ALSA might register it with
/// (e.g. "achieve", "ALSA plug-in [achieve]").
fn is_self(name: &str) -> bool {
    name.to_ascii_lowercase().contains("achieve")
}

/// True if any MPRIS media player reports `PlaybackStatus == "Playing"`.
fn mpris_playing() -> bool {
    mpris_playing_inner().unwrap_or(false)
}

fn mpris_playing_inner() -> zbus::Result<bool> {
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

#[cfg(test)]
mod tests {
    use super::any_stream_running;
    use serde_json::json;

    /// Shapes below mirror real `pw-dump` output on this machine (verified with
    /// pw-play: a playing stream reports state "running", a silent one "idle").
    fn node(app: &str, state: &str, class: &str) -> serde_json::Value {
        json!({
            "type": "PipeWire:Interface:Node",
            "info": { "state": state, "props": {
                "media.class": class, "application.name": app, "node.name": app } }
        })
    }

    #[test]
    fn running_stream_counts_as_playing() {
        let d = json!([node("pw-play", "running", "Stream/Output/Audio")]);
        assert!(any_stream_running(&d));
    }

    /// The Waydroid case: an app with no MPRIS registration at all still shows
    /// up here, which is the whole point of using PipeWire as the ground truth.
    #[test]
    fn idle_stream_does_not() {
        let d = json!([node("Brave", "idle", "Stream/Output/Audio")]);
        assert!(!any_stream_running(&d));
    }

    /// Our own cue must not count: the app beeping at you is not evidence that
    /// YOU are still at the desk.
    #[test]
    fn own_playback_is_ignored() {
        let d = json!([
            node("ALSA plug-in [achieve]", "running", "Stream/Output/Audio"),
            node("achieve", "running", "Stream/Output/Audio"),
        ]);
        assert!(!any_stream_running(&d));
    }

    /// A running microphone/input stream or a sink device is not playback.
    #[test]
    fn non_output_streams_are_ignored() {
        let d = json!([
            node("OBS", "running", "Stream/Input/Audio"),
            node("alsa_output.pci", "running", "Audio/Sink"),
        ]);
        assert!(!any_stream_running(&d));
    }

    #[test]
    fn malformed_objects_do_not_panic() {
        let d = json!([json!({"type": "PipeWire:Interface:Node"}), json!({}), json!(3)]);
        assert!(!any_stream_running(&d));
    }
}
