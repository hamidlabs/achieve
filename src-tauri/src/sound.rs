//! Backend audio cues. WebKitGTK's in-page audio (both `<audio>` and the Web
//! Audio API) is unreliable under Tauri on Linux, so instead of playing cues in
//! the webview we play them from Rust through the system audio, which is
//! known-good. The cue files are embedded in the binary and materialised to a
//! temp cache the first time they're used.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

const PRE_BREAK: &[u8] = include_bytes!("../../src/assets/sounds/on_pre_break.wav");
const STOP_BREAK: &[u8] = include_bytes!("../../src/assets/sounds/on_stop_break.wav");
const WARNING: &[u8] = include_bytes!("../../src/assets/sounds/warning.mp3");

/// Map a cue name (matching the frontend) to its embedded bytes and extension.
fn cue(name: &str) -> Option<(&'static [u8], &'static str)> {
    match name {
        "pre_break" => Some((PRE_BREAK, "wav")),
        "stop_break" => Some((STOP_BREAK, "wav")),
        "warning" => Some((WARNING, "mp3")),
        _ => None,
    }
}

/// Write a cue to a stable temp file (once) and return its path.
fn ensure_file(name: &str, bytes: &[u8], ext: &str) -> std::io::Result<PathBuf> {
    let mut dir = std::env::temp_dir();
    dir.push("achieve-sounds");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.{ext}"));
    if !path.exists() {
        std::fs::File::create(&path)?.write_all(bytes)?;
    }
    Ok(path)
}

/// Play a file through the first available system player. Runs on a worker
/// thread so it neither blocks the UI nor leaves a zombie process behind.
fn play_path(path: PathBuf) {
    std::thread::spawn(move || {
        // (binary, args that precede the file). Tried in order; the first that
        // exists and exits cleanly wins. aplay is WAV-only, hence last.
        let players: [(&str, &[&str]); 4] = [
            ("pw-play", &[]),
            ("paplay", &[]),
            ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "quiet"]),
            ("aplay", &["-q"]),
        ];
        for (bin, args) in players {
            match Command::new(bin).args(args).arg(&path).status() {
                Ok(s) if s.success() => return,
                _ => continue, // missing binary or decode failure: try the next
            }
        }
    });
}

/// Play a named cue, best-effort. Unknown names and IO failures are ignored.
pub fn play(name: &str) {
    if let Some((bytes, ext)) = cue(name) {
        if let Ok(path) = ensure_file(name, bytes, ext) {
            play_path(path);
        }
    }
}
