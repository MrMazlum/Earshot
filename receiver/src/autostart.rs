//! Starting the tray at login.
//!
//! Both platforms do the same two things — put the binary somewhere that will still exist next
//! week, then tell the session to run it — and disagree only about where the second half is
//! recorded. Linux writes a `.desktop` file; Windows writes a registry value.
//!
//! ⚠️ The login item points at a **copy of the binary**, never at wherever it happened to be run
//! from. An entry pointing into `target/release/` breaks the first time anyone runs `cargo clean`,
//! or the first time the user tidies up their Downloads folder — and it breaks *silently*: the
//! session simply starts nothing, with no error anywhere.

use std::path::{Path, PathBuf};

/// Where the login item ended up, for printing back to the user. They should be able to find and
/// delete it without needing this program.
pub struct Installed {
    /// The copy that will actually be run.
    pub binary: PathBuf,
    /// Where the "run this at login" instruction is recorded: a file path on Linux, a registry
    /// value on Windows.
    pub entry: String,
}

/// Copies the running binary to `target`, unless it is already that file.
///
/// Comparing canonical paths first is not an optimisation: copying a file onto itself truncates it
/// to nothing, so `--install` run twice would otherwise destroy the installed copy.
fn install_binary(target: &Path) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| format!("cannot find this binary: {e}"))?;
    let same = match (current.canonicalize(), target.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same {
        return Ok(());
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::copy(&current, target).map_err(|e| {
        format!(
            "cannot copy {} to {}: {e}",
            current.display(),
            target.display()
        )
    })?;
    Ok(())
}

#[cfg(target_os = "linux")]
mod platform {
    //! The freedesktop way: a `.desktop` file in `~/.config/autostart/`. No systemd unit, no root,
    //! and GNOME, KDE and XFCE all honour it.

    use super::{install_binary, Installed};
    use std::path::{Path, PathBuf};

    const DESKTOP_FILE: &str = "earshot.desktop";
    pub const BIN_NAME: &str = "earshot-tray";

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

    pub fn desktop_file() -> Result<PathBuf, String> {
        Ok(config_dir()?.join("autostart").join(DESKTOP_FILE))
    }

    pub fn installed_binary() -> Result<PathBuf, String> {
        Ok(home()?.join(".local").join("bin").join(BIN_NAME))
    }

    pub fn is_enabled() -> bool {
        desktop_file().map(|p| p.exists()).unwrap_or(false)
    }

    pub fn enable(extra_args: &str) -> Result<Installed, String> {
        let target = installed_binary()?;
        install_binary(&target)?;

        let desktop = desktop_file()?;
        if let Some(dir) = desktop.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        std::fs::write(&desktop, entry(&target, extra_args))
            .map_err(|e| format!("cannot write {}: {e}", desktop.display()))?;

        Ok(Installed {
            binary: target,
            entry: desktop.display().to_string(),
        })
    }

    /// The copied binary is left alone — it may well be the one running.
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
    pub fn exec_line(bin: &Path, extra_args: &str) -> String {
        let path = bin.display().to_string();
        let quoted = if path.contains('"') || path.contains('\\') || path.contains('`') {
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

    pub fn entry(bin: &Path, extra_args: &str) -> String {
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
}

#[cfg(target_os = "windows")]
mod platform {
    //! `HKCU\...\CurrentVersion\Run`, which is the one mechanism that needs no administrator, no
    //! scheduled task and no `.lnk` file. A shortcut in the Startup folder would be the more
    //! conventional choice, but creating one means driving `IShellLink` through COM — considerably
    //! more code, and more to go wrong, for an entry the user is *less* likely to find again.
    //!
    //! `HKCU` and not `HKLM`: this starts the tray for the person who asked for it, and needs no
    //! elevation to write. Anyone can see and delete it in Task Manager's Startup tab.

    use super::{install_binary, Installed};
    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_SZ,
    };

    pub const BIN_NAME: &str = "earshot-tray.exe";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "Earshot";

    /// A NUL-terminated UTF-16 string, which is what every `...W` entry point wants.
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Where the copy lives. `%LOCALAPPDATA%` rather than `%APPDATA%`: this is a machine-local
    /// binary, and a roaming profile should not drag an executable between machines.
    pub fn installed_binary() -> Result<PathBuf, String> {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "LOCALAPPDATA is not set, so there is nowhere to install to".to_string())?;
        Ok(base.join("Earshot").join(BIN_NAME))
    }

    /// Opens the Run key. `access` is `KEY_READ` or `KEY_SET_VALUE`.
    ///
    /// The key exists on every Windows installation, so this opens rather than creates it; a
    /// failure here is a real failure and not a first-run condition.
    fn open_run_key(access: u32) -> Result<HKEY, String> {
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `key` is a valid out-pointer.
        let rc = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(RUN_KEY).as_ptr(),
                0,
                access,
                &mut key,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(format!("cannot open HKCU\\{RUN_KEY} (error {rc})"));
        }
        Ok(key)
    }

    pub fn is_enabled() -> bool {
        let Ok(key) = open_run_key(KEY_READ) else {
            return false;
        };
        let mut size: u32 = 0;
        // SAFETY: a null data pointer with a valid size pointer is the documented way to ask only
        // whether the value exists and how big it is.
        let rc = unsafe {
            RegQueryValueExW(
                key,
                wide(VALUE_NAME).as_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        // SAFETY: `key` came from a successful open and is not used again.
        unsafe { RegCloseKey(key) };
        rc == ERROR_SUCCESS
    }

    pub fn enable(extra_args: &str) -> Result<Installed, String> {
        let target = installed_binary()?;
        install_binary(&target)?;

        let command = super::command_line(&target, extra_args);
        let key = open_run_key(KEY_SET_VALUE)?;
        let value = wide(&command);
        // The length is in *bytes* and includes the terminating NUL, which is what makes the value
        // read back as a string rather than a string with a stray character on the end.
        let bytes = value.len() * std::mem::size_of::<u16>();
        // SAFETY: `value` is a NUL-terminated UTF-16 buffer of exactly `bytes` bytes, and it
        // outlives the call.
        let rc = unsafe {
            RegSetValueExW(
                key,
                wide(VALUE_NAME).as_ptr(),
                0,
                REG_SZ,
                value.as_ptr() as *const u8,
                bytes as u32,
            )
        };
        // SAFETY: `key` came from a successful open and is not used again.
        unsafe { RegCloseKey(key) };
        if rc != ERROR_SUCCESS {
            return Err(format!(
                "cannot write the start-at-login entry (error {rc})"
            ));
        }
        Ok(Installed {
            binary: target,
            entry: format!("HKCU\\{RUN_KEY}\\{VALUE_NAME}"),
        })
    }

    /// The copied binary is left alone — it is almost certainly the one running.
    pub fn disable() -> Result<bool, String> {
        let key = open_run_key(KEY_SET_VALUE)?;
        // SAFETY: `key` is valid and the name is NUL-terminated.
        let rc = unsafe { RegDeleteValueW(key, wide(VALUE_NAME).as_ptr()) };
        // SAFETY: `key` came from a successful open and is not used again.
        unsafe { RegCloseKey(key) };
        match rc {
            ERROR_SUCCESS => Ok(true),
            // Already gone is not an error; it is the state the caller wanted.
            _ if !is_enabled() => Ok(false),
            _ => Err(format!(
                "cannot remove the start-at-login entry (error {rc})"
            )),
        }
    }
}

/// The command a login item runs: a quoted path, then any extra arguments.
///
/// Quoting is not optional. The default install path contains the user's name, and
/// `C:\Users\Ada Lovelace\...` unquoted is read by Windows as `C:\Users\Ada` with `Lovelace\...` as
/// its first argument — which then fails as an unknown option, at login, with nowhere to print it.
#[cfg(target_os = "windows")]
fn command_line(bin: &Path, extra_args: &str) -> String {
    let quoted = format!("\"{}\"", bin.display());
    if extra_args.is_empty() {
        quoted
    } else {
        format!("{quoted} {extra_args}")
    }
}

/// Whether Earshot is set to start at login.
pub fn is_enabled() -> bool {
    platform::is_enabled()
}

/// Copies the binary somewhere stable and records the login item.
pub fn enable(extra_args: &str) -> Result<Installed, String> {
    platform::enable(extra_args)
}

/// Removes the login item. `Ok(false)` means there was not one.
pub fn disable() -> Result<bool, String> {
    platform::disable()
}

/// Where the binary is copied to, so the login item survives a `cargo clean` or a tidied Downloads
/// folder.
pub fn installed_binary() -> Result<PathBuf, String> {
    platform::installed_binary()
}

/// Whether Earshot has never installed itself on this machine.
///
/// The installed copy is its own marker, so there is no second piece of state to keep in step. That
/// is the whole point: [`enable`] creates it and only the user deleting it takes it away, so a
/// caller that sets the login item up on first run does so exactly once. Switching "start at login"
/// off in the tray leaves the copy behind — and therefore stays off, instead of being helpfully
/// switched back on at the next launch.
///
/// A path that cannot even be worked out counts as "not the first run": there is nowhere to install
/// to, so there is nothing useful to do about it.
pub fn is_first_run() -> bool {
    platform::installed_binary().is_ok_and(|p| !p.exists())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    mod linux {
        use crate::autostart::platform::{entry, exec_line, BIN_NAME};
        use std::path::Path;

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

    #[cfg(target_os = "windows")]
    mod windows {
        use crate::autostart::command_line;
        use std::path::Path;

        /// The default install path always contains the user's name, so a space in it is the
        /// normal case and not the edge case.
        #[test]
        fn the_path_is_quoted_because_user_names_contain_spaces() {
            let line = command_line(
                Path::new(r"C:\Users\Ada Lovelace\AppData\Local\Earshot\earshot-tray.exe"),
                "",
            );
            assert_eq!(
                line,
                r#""C:\Users\Ada Lovelace\AppData\Local\Earshot\earshot-tray.exe""#
            );
        }

        #[test]
        fn extra_arguments_land_outside_the_quotes() {
            let line = command_line(Path::new(r"C:\a b\earshot-tray.exe"), "--no-virtual-mic");
            assert_eq!(line, r#""C:\a b\earshot-tray.exe" --no-virtual-mic"#);
        }
    }
}
