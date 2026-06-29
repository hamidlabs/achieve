//! Idle detection (away-from-keyboard) so we never count time the user wasn't
//! actually working: e.g. a task left "tracking" overnight.
//!
//! On Wayland the ground truth for idleness is the `ext-idle-notify` protocol.
//! Rather than embed a Wayland client, we drive the well-tested `swayidle`
//! daemon (present on niri/sway setups) and use a flag file as the shared signal
//! the engine polls each tick:
//!   - idle for `idle_secs`  -> swayidle creates the flag
//!   - activity resumes      -> swayidle removes the flag
//! The engine, on seeing the flag, caps the open segment at the moment input
//! actually stopped and pauses the task; on activity it can re-nudge.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

/// Where the idle flag lives (next to the database).
pub fn idle_flag_path(data_dir: &Path) -> PathBuf {
    data_dir.join("idle.flag")
}

/// Spawn `swayidle` to maintain the idle flag. No-op (logged) if swayidle is
/// not installed, so the app still runs, just without idle capping.
pub fn spawn_idle_watcher(flag: PathBuf, idle_secs: u32) {
    if which("swayidle").is_none() {
        eprintln!(
            "[achieve] swayidle not found on PATH; idle detection disabled \
             (install swayidle to auto-pause tasks when you step away)"
        );
        return;
    }
    // Clear any stale flag from a previous run before we start watching.
    let _ = std::fs::remove_file(&flag);

    let flag_q = shell_quote(&flag.to_string_lossy());
    thread::spawn(move || {
        let on_idle = format!("touch {flag_q}");
        let on_resume = format!("rm -f {flag_q}");
        let mut cmd = Command::new("swayidle");
        cmd.arg("-w")
            .args(["timeout", &idle_secs.to_string(), &on_idle])
            .args(["resume", &on_resume])
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Die with the parent so we never orphan (same guard as the niri stream).
        #[cfg(target_os = "linux")]
        unsafe {
            use std::os::unix::process::CommandExt;
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }

        match cmd.spawn() {
            Ok(mut child) => {
                let _ = child.wait();
                eprintln!("[achieve] swayidle exited");
            }
            Err(e) => eprintln!("[achieve] failed to spawn swayidle: {e}"),
        }
    });
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(bin))
            .find(|p| p.is_file())
    })
}

/// Single-quote a path for an `sh -c` command line.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
