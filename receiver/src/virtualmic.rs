//! The virtual microphone: making other applications see Earshot as an input device.
//!
//! This is the step that turns the project from a demo into something usable. The two platforms do
//! genuinely different things, and the difference is not cosmetic:
//!
//! - **Linux** can be *asked* to invent a device. A null sink plus a remapped source costs two
//!   `pactl` calls, no driver, no root, no third-party install.
//! - **Windows** cannot. Creating an audio endpoint needs a kernel driver, and shipping one needs a
//!   signed WHQL package — hundreds of euros a year. So Windows borrows somebody else's: VB-Cable
//!   is installed by the user, and Earshot plays into it.
//!
//! Both paths end in the same shape, [`Devices`]: what *we* should open, and what the *user* should
//! pick in Discord. Those are never the same string, which is the thing that confuses people.

/// The outcome of arranging a virtual microphone, whatever it took.
pub struct Devices {
    /// What the receiver asks cpal to open. Matched as a substring of the device name.
    pub device_hint: String,
    /// What the user picks in Discord, OBS or Zoom. Shown in every message we print.
    pub display: String,
    /// True when this run created the device, rather than finding it already there. Always false
    /// on Windows, where the device belongs to VB-Cable and outlives everything we do.
    pub created: bool,
}

#[cfg(target_os = "linux")]
mod platform {
    //! ⚠️ The description cannot contain a space. pipewire-pulse splits `source_properties` on
    //! whitespace whichever way it is quoted, so "Earshot Microphone" arrives as "Earshot". The
    //! description is therefore just the device name, and the printed guidance says the same word.
    //!
    //! The devices deliberately **outlive the process**. Applications remember the input device you
    //! picked, so tearing it down on exit would make Discord silently fall back to the laptop mic
    //! every time. `--remove-virtual-mic` removes them.

    use super::Devices;
    use std::process::Command;

    fn pactl(args: &[&str]) -> Result<String, String> {
        let out = Command::new("pactl").args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "`pactl` not found - Earshot's virtual microphone needs PipeWire or PulseAudio \
                 (install `pulseaudio-utils`)"
                    .to_string()
            } else {
                format!("cannot run pactl: {e}")
            }
        })?;
        if !out.status.success() {
            return Err(format!(
                "pactl {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Names in `pactl list short <kind>` — the second column.
    fn existing(kind: &str) -> Result<Vec<String>, String> {
        Ok(pactl(&["list", "short", kind])?
            .lines()
            .filter_map(|l| l.split('\t').nth(1).map(str::to_string))
            .collect())
    }

    pub fn source_name(name: &str) -> String {
        format!("{name}Mic")
    }

    pub fn ensure(name: &str) -> Result<Devices, String> {
        let source = source_name(name);
        let mut created = false;

        if !existing("sinks")?.iter().any(|s| s.as_str() == name) {
            pactl(&[
                "load-module",
                "module-null-sink",
                &format!("sink_name={name}"),
                &format!("sink_properties=device.description={name}"),
            ])?;
            created = true;
        }

        if !existing("sources")?.contains(&source) {
            pactl(&[
                "load-module",
                "module-remap-source",
                &format!("master={name}.monitor"),
                &format!("source_name={source}"),
                // No space here — see the note at the top of this module.
                &format!("source_properties=device.description={name}"),
            ])?;
            created = true;
        }

        // Read when the output device is opened, not later, so it has to be set before cpal starts.
        std::env::set_var("PULSE_SINK", name);

        Ok(Devices {
            // The audio must go through the PulseAudio path for PULSE_SINK to apply at all; the
            // ALSA "default" device ignores it and plays out of the speakers instead.
            device_hint: "pulse".to_string(),
            display: name.to_string(),
            created,
        })
    }

    /// Toggling the virtual mic off within one process must not leave the variable behind.
    pub fn clear_routing() {
        std::env::remove_var("PULSE_SINK");
    }

    /// Unloads whatever `ensure` created. Matches modules by the arguments they were loaded with,
    /// so nothing needs to be remembered between runs.
    pub fn remove(name: &str) -> Result<bool, String> {
        let source = source_name(name);
        let modules = pactl(&["list", "short", "modules"])?;
        let mut ids = Vec::new();

        for line in modules.lines() {
            let mut cols = line.split('\t');
            let (Some(id), Some(module), Some(args)) = (cols.next(), cols.next(), cols.next())
            else {
                continue;
            };
            let ours = (module == "module-null-sink"
                && args.contains(&format!("sink_name={name}")))
                || (module == "module-remap-source"
                    && args.contains(&format!("source_name={source}")));
            if ours {
                ids.push(id.to_string());
            }
        }

        // Highest id first: the remapped source depends on the sink, so it goes first.
        ids.sort_by(|a, b| b.cmp(a));
        let found = !ids.is_empty();
        for id in ids {
            pactl(&["unload-module", &id])?;
        }
        Ok(found)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    //! Windows has no way to invent an audio endpoint without a signed kernel driver, so Earshot
    //! does not try. It looks for a virtual cable that is already installed and plays into its
    //! playback half; [`crate::cable`] holds the list of cables it recognises, the reasoning, and
    //! the guided path for a machine that has none.
    //!
    //! Nothing is created and nothing is removed; the cable belongs to its own installer.

    use super::Devices;

    pub fn ensure(_name: &str) -> Result<Devices, String> {
        match crate::cable::find()? {
            Some(found) => Ok(Devices {
                device_hint: found.playback_device,
                display: found.capture.to_string(),
                // The cable is installed, not created by us, so it is never "new".
                created: false,
            }),
            // `cable::preflight` normally gets there first and offers to fix this, so reaching here
            // means the tray or a non-interactive run hit it. The message still has to stand alone.
            None => Err(crate::cable::missing_message(
                &crate::audio::output_device_names().unwrap_or_default(),
            )),
        }
    }

    pub fn clear_routing() {}

    /// Nothing to remove — the cable is its vendor's, and uninstalling it is that installer's job.
    pub fn remove(_name: &str) -> Result<bool, String> {
        Ok(false)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod platform {
    use super::Devices;

    pub fn ensure(_name: &str) -> Result<Devices, String> {
        Err("Earshot's virtual microphone is not implemented on this platform yet. \
             On macOS, install BlackHole and point Earshot at it with --device."
            .to_string())
    }

    pub fn clear_routing() {}

    pub fn remove(_name: &str) -> Result<bool, String> {
        Ok(false)
    }
}

/// Arranges a virtual microphone, creating one if the platform allows it.
///
/// Safe to call on every start: on Linux it reuses devices that are already there.
pub fn ensure(name: &str) -> Result<Devices, String> {
    platform::ensure(name)
}

/// Undoes any process-wide routing state set by [`ensure`]. Call when *not* using a virtual mic,
/// so a toggle within one process cannot leave the previous setting applied.
pub fn clear_routing() {
    platform::clear_routing();
}

/// Removes a virtual microphone this program created. `Ok(false)` means there was nothing to do —
/// including on Windows, where the device was never ours.
pub fn remove(name: &str) -> Result<bool, String> {
    platform::remove(name)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[test]
    fn the_source_is_named_after_the_sink_so_remove_can_find_it_again() {
        assert_eq!(super::platform::source_name("Earshot"), "EarshotMic");
    }
}
