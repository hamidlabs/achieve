//! Compositor self-configuration.
//!
//! Wayland forbids a client from positioning its own window, so to get the
//! popup floating and centered we add a window-rule to niri's config (matching
//! the window title "Achieve"). This mirrors the dirwatch approach: a clearly
//! marked managed block, a backup before touching the file, and `niri validate`
//! with rollback if we ever produce something invalid.

use std::path::PathBuf;
use std::process::Command;

const BEGIN: &str = "// >>> achieve floating rule (managed) - do not edit inside";
const END: &str = "// <<< end achieve floating rule (managed)";

const RULE: &str = r#"window-rule {
    match title="^Achieve$"
    open-floating true
    // the glass card draws its own frame; suppress niri's ring/border/shadow
    // so a colored focus ring does not appear behind the transparent window.
    focus-ring { off; }
    border { off; }
    shadow { off; }
    // Round + clip the window to the card's radius so its corners are rounded
    // by the compositor and there's no square transparent margin around it.
    geometry-corner-radius 14
    clip-to-geometry true
}"#;

/// Idempotently ensure the niri float rule exists. Best-effort: never panics.
pub fn ensure_niri_float_rule() {
    if !on_niri() {
        return;
    }
    let path = match niri_config_path() {
        Some(p) if p.exists() => p,
        _ => {
            eprintln!("[achieve] no niri config found; skipping float-rule setup");
            return;
        }
    };

    let current = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[achieve] could not read niri config: {e}");
            return;
        }
    };

    let block = format!("{BEGIN}\n{RULE}\n{END}");

    // Decide whether we need to write: append if absent, replace if our managed
    // block exists but its content has drifted from the current RULE.
    let updated = match (current.find(BEGIN), current.find(END)) {
        (Some(b), Some(e)) if e > b => {
            let end_full = e + END.len();
            let existing = &current[b..end_full];
            if existing == block {
                return; // already up to date
            }
            format!("{}{}{}", &current[..b], block, &current[end_full..])
        }
        _ => format!("{}\n{}\n", current.trim_end(), block),
    };

    // Back up before we touch anything.
    let backup = path.with_extension("kdl.achieve.bak");
    if let Err(e) = std::fs::write(&backup, &current) {
        eprintln!("[achieve] could not write niri config backup: {e}");
        return;
    }

    if let Err(e) = std::fs::write(&path, &updated) {
        eprintln!("[achieve] could not write niri config: {e}");
        return;
    }

    // Validate; roll back on failure.
    let ok = Command::new("niri")
        .args(["validate", "--config"])
        .arg(&path)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !ok {
        eprintln!("[achieve] niri validate failed; rolling back config change");
        let _ = std::fs::write(&path, &current);
    } else {
        eprintln!("[achieve] added niri float rule (backup at {})", backup.display());
    }
}

fn on_niri() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|d| d.to_lowercase().contains("niri"))
        .unwrap_or(false)
        || std::env::var("NIRI_SOCKET").is_ok()
}

fn niri_config_path() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("niri").join("config.kdl"));
        }
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config").join("niri").join("config.kdl"))
}
