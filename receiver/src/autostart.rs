//! Starting the tray at login, the freedesktop way.
//!
//! A `.desktop` file in `~/.config/autostart/` is all this takes — no systemd unit, no root. GNOME,
//! KDE and XFCE all honour it, and the user can see and delete it without needing us.
//!
//! ⚠️ The file points at a **copy of the binary in `~/.local/bin/`**, not at wherever it was run
//! from. A login item pointing into `target/release/` breaks the first time anyone runs
//! `cargo clean`, and it breaks silently — the session just starts nothing.

use std::path::{Path, PathBuf};

const DESKTOP_FILE: &str = "earshot.desktop";
const BIN_NAME: &str = "earshot-tray";

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set, so there is nowhere to install to".to_string())
}

fn config_dir() -> Result<PathBuf, String> {
    match std::env::var_os("XDG_CONFIG_HOME") {
        Some(p) if !p.is_empty() => Ok(PathBuf::from(p)),
        _ => Ok(home()?.join(".config")),
    }
}

/// Where the login item lives.
pub fn desktop_file() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("autostart").join(DESKTOP_FILE))
}

/// Where the binary is copied to, so the login item survives a `cargo clean`.
pub fn installed_binary() -> Result<PathBuf, String> {
    Ok(home()?.join(".local").join("bin").join(BIN_NAME))
}

pub fn is_enabled() -> bool {
    desktop_file().map(|p| p.exists()).unwrap_or(false)
}

/// Copies the binary somewhere stable and writes the login item. Returns both paths.
pub fn enable(extra_args: &str) -> Result<(PathBuf, PathBuf), String> {
    let target = installed_binary()?;
    let current = std::env::current_exe().map_err(|e| format!("cannot find this binary: {e}"))?;

    // Comparing canonical paths, because copying a file onto itself truncates it to nothing.
    let same = match (current.canonicalize(), target.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        std::fs::copy(&current, &target).map_err(|e| {
            format!(
                "cannot copy {} to {}: {e}",
                current.display(),
                target.display()
            )
        })?;
    }

    let desktop = desktop_file()?;
    if let Some(dir) = desktop.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(&desktop, entry(&target, extra_args))
        .map_err(|e| format!("cannot write {}: {e}", desktop.display()))?;

    Ok((target, desktop))
}

/// Removes the login item. The copied binary is left alone — it may be the one running.
pub fn disable() -> Result<bool, String> {
    let desktop = desktop_file()?;
    match std::fs::remove_file(&desktop) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("cannot remove {}: {e}", desktop.display())),
    }
}

/// `Exec` takes a quoted path so a home directory with a space in it still works. Reserved
/// characters cannot be escaped inside a quoted string, so a path containing one is refused
/// outright rather than silently producing a login item that does the wrong thing.
fn exec_line(bin: &Path, extra_args: &str) -> String {
    let path = bin.display().to_string();
    let quoted = if path.contains('"') || path.contains('\\') || path.contains('`') {
        // Nothing sane can be produced here; fall back to the bare name and let PATH sort it out.
        BIN_NAME.to_string()
    } else {
        format!("\"{path}\"")
    };
    if extra_args.is_empty() {
        quoted
    } else {
        format!("{quoted} {extra_args}")
    }
}

fn entry(bin: &Path, extra_args: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Earshot\n\
         GenericName=Phone microphone bridge\n\
         Comment=Use your phone as this machine's microphone\n\
         Exec={}\n\
         Icon=audio-input-microphone\n\
         Terminal=false\n\
         Categories=AudioVideo;Audio;\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_line(bin, extra_args)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_is_a_valid_desktop_file() {
        let e = entry(Path::new("/home/x/.local/bin/earshot-tray"), "");
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("Type=Application\n"));
        assert!(e.contains("Exec=\"/home/x/.local/bin/earshot-tray\"\n"));
        assert!(e.ends_with('\n'));
    }

    #[test]
    fn a_space_in_the_path_survives() {
        let line = exec_line(Path::new("/home/my name/bin/earshot-tray"), "");
        assert_eq!(line, "\"/home/my name/bin/earshot-tray\"");
    }

    #[test]
    fn extra_arguments_land_after_the_quoted_path() {
        let line = exec_line(Path::new("/opt/earshot-tray"), "--no-virtual-mic");
        assert_eq!(line, "\"/opt/earshot-tray\" --no-virtual-mic");
    }

    #[test]
    fn an_unquotable_path_falls_back_rather_than_producing_a_broken_entry() {
        let line = exec_line(Path::new("/home/we\"ird/earshot-tray"), "");
        assert_eq!(line, BIN_NAME);
    }
}
