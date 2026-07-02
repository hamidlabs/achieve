//! Backend audio cues, played with rodio (pure-Rust playback over cpal/ALSA).
//!
//! The Tauri webview on Linux (WebKitGTK) can't reliably play in-page audio, so
//! the frontend asks us (via the `play_sound` command) to play a cue and we
//! decode + play the embedded file here. No temp files, no external players.

use std::io::Cursor;

const PRE_BREAK: &[u8] = include_bytes!("../../src/assets/sounds/on_pre_break.wav");
const STOP_BREAK: &[u8] = include_bytes!("../../src/assets/sounds/on_stop_break.wav");
const WARNING: &[u8] = include_bytes!("../../src/assets/sounds/warning.mp3");

/// Play a named cue, best-effort. Unknown names are ignored.
pub fn play(name: &str) {
    let bytes: &'static [u8] = match name {
        "pre_break" => PRE_BREAK,
        "stop_break" => STOP_BREAK,
        "warning" => WARNING,
        _ => return,
    };

    // Play on a worker thread: rodio's output stream must stay alive for the
    // whole playback, so we hold it on this thread and block (not the UI) until
    // the cue finishes, then let it drop.
    let label = name.to_string();
    std::thread::spawn(move || {
        let (_stream, handle) = match rodio::OutputStream::try_default() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[sound] no audio output: {e}");
                return;
            }
        };
        let sink = match rodio::Sink::try_new(&handle) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[sound] could not open audio sink: {e}");
                return;
            }
        };
        match rodio::Decoder::new(Cursor::new(bytes)) {
            Ok(source) => {
                sink.append(source);
                sink.sleep_until_end();
            }
            Err(e) => eprintln!("[sound] decode {label} failed: {e}"),
        }
    });
}
