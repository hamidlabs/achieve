//! Launch-on-login. The whole point of Achieve is that it greets you each day
//! without you remembering to open it, so on first run we add an XDG autostart
//! entry pointing at our own executable. Idempotent and best-effort.

pub fn ensure_autostart() {
    // Prefer the real AppImage path (set by the runtime) over the extracted
    // temp exe, so the entry survives across runs.
    let exec = std::env::var("APPIMAGE")
        .ok()
        .or_else(|| std::env::current_exe().ok().map(|p| p.display().to_string()));
    let exec = match exec {
        Some(e) => e,
        None => return,
    };
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return,
    };

    let dir = format!("{home}/.config/autostart");
    let path = format!("{dir}/achieve.desktop");
    // Launch via APPIMAGE_EXTRACT_AND_RUN=1 (the documented run method) so we
    // don't depend on FUSE being ready this early in the session.
    let exec_line = format!("Exec=env APPIMAGE_EXTRACT_AND_RUN=1 {exec}");

    // Already pointing at the right exec? leave it.
    if std::fs::read_to_string(&path)
        .map(|c| c.contains(&exec_line))
        .unwrap_or(false)
    {
        return;
    }

    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Achieve\n\
         Comment=Proactive day-companion\n\
         {exec_line}\n\
         Icon=achieve\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    );
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(&path, content).is_ok() {
        eprintln!("[achieve] autostart enabled ({path})");
    }
}
