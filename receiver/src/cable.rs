//! Virtual audio cables on Windows: finding one, and walking the user through installing one.
//!
//! Windows will not let a user-mode program invent a recording device. An audio endpoint is a
//! kernel-mode object, and publishing one needs a driver package signed by Microsoft — an EV
//! certificate, a registered company, and a few hundred euros a year. (Media Foundation opened this
//! up for *cameras* in Windows 11; there is still no audio equivalent, which is why OBS ships a
//! virtual camera and no virtual microphone.)
//!
//! So Earshot borrows a cable somebody else already signed. A virtual cable is two endpoints wired
//! back to back:
//!
//! ```text
//!   Earshot  ->  "CABLE Input"  (a playback device)
//!                     |
//!                     v
//!   Discord  <-  "CABLE Output" (a recording device)   <- the user picks this one
//! ```
//!
//! The names read backwards from the outside and this catches out everybody, every time, so every
//! message in this module says which one is which.
//!
//! Nothing here creates or removes anything: the cable belongs to its own installer. What this
//! module adds over a bare "not found" error is the [`preflight`] path — detect that no cable is
//! installed, explain it in terms of what the user is trying to do, open the download page, and
//! then wait and carry on by itself once the cable appears.
//!
//! Deliberately **not** implemented: downloading and running the installer for the user. It is a
//! third-party kernel driver, VB-Audio publishes no stable checksum to verify a download against,
//! and silently elevating an unverified binary is not a thing this program should teach people to
//! accept. The browser goes to the vendor's own page instead.

/// A virtual cable this program knows how to use.
pub struct Cable {
    /// Shown to the user, so it should be the name on the vendor's website.
    pub product: &'static str,
    /// Substring of the **playback** endpoint's name — the end Earshot plays into.
    pub playback: &'static str,
    /// Substring of the **recording** endpoint's name — the end the user picks in Discord.
    pub capture: &'static str,
}

/// Ordered by preference. VB-Cable first because it is the one the documentation names, it is free,
/// and it is a single device with no mixer to configure.
///
/// Matching is by substring because Windows appends the driver's name to the endpoint, so the real
/// device is `CABLE Input (VB-Audio Virtual Cable)`. The substrings are chosen to be specific
/// enough not to collide with a real sound card: `Line 1` alone would, `Virtual Audio Cable` does
/// not.
pub const KNOWN: &[Cable] = &[
    Cable {
        product: "VB-Cable",
        playback: "CABLE Input",
        capture: "CABLE Output",
    },
    Cable {
        product: "VoiceMeeter",
        playback: "VoiceMeeter Input",
        capture: "VoiceMeeter Output",
    },
    Cable {
        product: "VoiceMeeter",
        playback: "VoiceMeeter Aux Input",
        capture: "VoiceMeeter Aux Output",
    },
    Cable {
        product: "Virtual Audio Cable",
        playback: "Virtual Audio Cable",
        capture: "Virtual Audio Cable",
    },
];

/// The page to send people to. The vendor's own, never a mirror.
pub const DOWNLOAD_PAGE: &str = "https://vb-audio.com/Cable/";

/// A cable that is actually installed on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub product: &'static str,
    /// The endpoint name as the audio host reports it, ready to hand to [`crate::audio::open`].
    pub playback_device: String,
    /// What to tell the user to pick as their microphone.
    pub capture: &'static str,
}

/// Picks the first known cable present in a list of playback device names.
///
/// Pure and case-insensitive, so it is testable on a machine that has no cables and no Windows.
pub fn identify(devices: &[String]) -> Option<Found> {
    for cable in KNOWN {
        let want = cable.playback.to_lowercase();
        if let Some(device) = devices.iter().find(|d| d.to_lowercase().contains(&want)) {
            return Some(Found {
                product: cable.product,
                playback_device: device.clone(),
                capture: cable.capture,
            });
        }
    }
    None
}

/// Asks the audio host what playback devices exist, then [`identify`]s a cable among them.
pub fn find() -> Result<Option<Found>, String> {
    Ok(identify(&crate::audio::output_device_names()?))
}

/// What to say when there is no cable. Also the text of the error the engine returns, so a user who
/// never sees the interactive path still gets the whole story.
///
/// `devices` is listed because the one thing worse than "not found" is "not found" with no clue
/// about what *was* found — somebody with a cable under an unrecognised name can then point
/// `--device` straight at it.
pub fn missing_message(devices: &[String]) -> String {
    let found = if devices.is_empty() {
        "  (none - this machine reports no playback devices at all)".to_string()
    } else {
        devices
            .iter()
            .map(|d| format!("  - {d}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "No virtual audio cable is installed.\n\
         \n\
         Windows does not let a program invent a microphone - that needs a signed kernel driver.\n\
         Earshot therefore plays into a virtual cable, and you pick the other end of that cable as\n\
         your microphone. VB-Cable is free and installs once:\n\
         \n    {DOWNLOAD_PAGE}\n\
         \n\
         After installing it (a reboot may be needed), run this again and Earshot will find it.\n\
         \n\
         Already have a different cable? Point Earshot at its playback end by hand:\n\
         \n    earshot-receiver --virtual-mic --device \"<its playback device>\"\n\
         \n\
         Playback devices on this machine right now:\n{found}"
    )
}

/// The steps, once the download page is open. Written as what to click, not what to understand.
///
/// `pub` because it is one half of the guided path and the other half is Windows-only; keeping it
/// private would make it dead code on every other platform.
pub fn install_steps() -> String {
    format!(
        "On the page that just opened:\n\n  \
         1. Download the VB-CABLE driver pack (a .zip)\n  \
         2. Unzip it anywhere\n  \
         3. Right-click VBCABLE_Setup_x64.exe and choose \"Run as administrator\"\n  \
         4. Click \"Install Driver\", then reboot if it asks\n\n\
         If the page did not open by itself, it is:  {DOWNLOAD_PAGE}"
    )
}

/// Everything below is the interactive path, which only makes sense on the platform that needs it.
#[cfg(target_os = "windows")]
mod interactive {
    use super::{find, install_steps, missing_message, Found, DOWNLOAD_PAGE};
    use std::io::Write;
    use std::process::Command;
    use std::time::{Duration, Instant};

    /// How long to keep looking after the user has been sent to the download page. Long enough to
    /// download, unzip, install and reboot-if-needed without the program giving up first; the user
    /// can always Ctrl-C, and a decline is one keypress away before this starts.
    const WAIT: Duration = Duration::from_secs(15 * 60);
    const POLL: Duration = Duration::from_secs(3);

    /// Hands the URL to whatever the user's default browser is.
    ///
    /// `rundll32 url.dll,FileProtocolHandler` rather than `cmd /c start`: `start` needs an empty
    /// first argument to avoid treating a quoted URL as a window title, and Rust quotes every
    /// argument it passes, so that combination is fragile. This one takes the URL as a plain
    /// argument.
    fn open_page() -> Result<(), String> {
        Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", DOWNLOAD_PAGE])
            .status()
            .map_err(|e| format!("could not open a browser: {e}"))?;
        Ok(())
    }

    /// Reads one line. `Ok(None)` means there is nobody there to answer — piped input, or a
    /// service - which must be treated as "no", never as consent.
    fn ask(question: &str) -> Option<String> {
        print!("{question}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim().to_lowercase()),
        }
    }

    /// Polls until a cable shows up, or until [`WAIT`] runs out.
    fn wait_for_cable() -> Result<Option<Found>, String> {
        let started = Instant::now();
        let mut last_note = Instant::now();
        println!();
        println!("Waiting for the cable to appear... (Ctrl-C to stop)");
        while started.elapsed() < WAIT {
            if let Some(found) = find()? {
                return Ok(Some(found));
            }
            if last_note.elapsed() >= Duration::from_secs(30) {
                last_note = Instant::now();
                println!(
                    "  still nothing after {}s - if the installer asked for a reboot, do that and \
                     start Earshot again",
                    started.elapsed().as_secs()
                );
            }
            std::thread::sleep(POLL);
        }
        Ok(None)
    }

    /// Called before the engine starts, when a virtual microphone has been asked for.
    ///
    /// Success means a cable exists *now*, so the engine's own lookup cannot then fail. Doing this
    /// here rather than inside the engine is deliberate: the engine runs on the audio thread's
    /// setup path and has no business printing to a terminal or reading from stdin.
    pub fn preflight() -> Result<(), String> {
        if let Some(found) = find()? {
            println!("Virtual cable   {} ({})", found.product, found.capture);
            return Ok(());
        }

        let devices = crate::audio::output_device_names().unwrap_or_default();
        println!("{}", missing_message(&devices));
        println!();

        let answer = ask("Open the VB-Cable download page now? [Y/n] ");
        // Enter means yes: it is the only way forward from here, and the alternative is retyping a
        // URL by hand. Anything else, including no answer at all, means no.
        if !matches!(answer.as_deref(), Some("") | Some("y") | Some("yes")) {
            return Err("no virtual cable, and the download page was not opened. \
                        Install one and run this again."
                .to_string());
        }

        if let Err(e) = open_page() {
            println!("{e}");
        }
        println!();
        println!("{}", install_steps());

        match wait_for_cable()? {
            Some(found) => {
                println!();
                println!("Found it: {} - carrying on.", found.product);
                println!(
                    "Remember to pick \"{}\" as your microphone in Discord, not \"{}\".",
                    found.capture, found.playback_device
                );
                Ok(())
            }
            None => Err(format!(
                "still no virtual cable after 15 minutes. Finish installing it (and reboot if \
                 asked), then run Earshot again.\n{DOWNLOAD_PAGE}"
            )),
        }
    }
}

#[cfg(target_os = "windows")]
pub use interactive::preflight;

/// Nothing to do anywhere else: Linux creates its own device, and macOS is told to use `--device`.
#[cfg(not(target_os = "windows"))]
pub fn preflight() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The name Windows actually reports is the endpoint plus the driver, not the bare string.
    #[test]
    fn the_real_windows_device_name_is_recognised() {
        let found = identify(&names(&[
            "Speakers (Realtek(R) Audio)",
            "CABLE Input (VB-Audio Virtual Cable)",
        ]))
        .expect("VB-Cable should be found");
        assert_eq!(found.product, "VB-Cable");
        assert_eq!(found.playback_device, "CABLE Input (VB-Audio Virtual Cable)");
        assert_eq!(found.capture, "CABLE Output");
    }

    /// The whole point of the two names: what we open is never what the user picks.
    #[test]
    fn what_we_open_is_never_what_the_user_picks() {
        for cable in KNOWN {
            if cable.product == "Virtual Audio Cable" {
                continue; // VAC numbers its lines; both ends share one name
            }
            assert_ne!(cable.playback, cable.capture, "{}", cable.product);
        }
    }

    #[test]
    fn a_machine_with_no_cable_finds_nothing() {
        assert!(identify(&names(&["Speakers (Realtek(R) Audio)", "HDMI Output"])).is_none());
    }

    /// A sound card with a line output must not be mistaken for Virtual Audio Cable. This is why
    /// the VAC entry matches on the driver name and not on "Line 1".
    #[test]
    fn a_real_line_output_is_not_a_virtual_cable() {
        assert!(identify(&names(&["Line 1 (Sound Blaster)", "Line In"])).is_none());
    }

    #[test]
    fn vb_cable_wins_over_voicemeeter_when_both_are_installed() {
        let found = identify(&names(&[
            "VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)",
            "CABLE Input (VB-Audio Virtual Cable)",
        ]))
        .expect("one of them should be found");
        assert_eq!(found.product, "VB-Cable");
    }

    #[test]
    fn voicemeeter_alone_is_still_usable() {
        let found = identify(&names(&["VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)"]))
            .expect("VoiceMeeter should be found");
        assert_eq!(found.capture, "VoiceMeeter Output");
    }

    /// The message has to carry the address of the fix and the evidence of the problem, or it is
    /// just "no".
    #[test]
    fn the_missing_message_names_the_download_and_lists_what_was_found() {
        let msg = missing_message(&names(&["Speakers (Realtek(R) Audio)"]));
        assert!(msg.contains(DOWNLOAD_PAGE));
        assert!(msg.contains("Speakers (Realtek(R) Audio)"));
        assert!(msg.contains("--device"));
    }

    #[test]
    fn the_missing_message_copes_with_a_machine_that_has_no_outputs() {
        let msg = missing_message(&[]);
        assert!(msg.contains("no playback devices"));
    }

    /// Windows consoles are not reliably UTF-8: a Turkish or Western-European code page renders a
    /// dash or an ellipsis as mojibake, and this text is the first thing a Windows user reads.
    #[test]
    fn every_message_is_plain_ascii() {
        let mut all = vec![missing_message(&names(&["Speakers"])), install_steps()];
        all.push(DOWNLOAD_PAGE.to_string());
        for cable in KNOWN {
            all.push(cable.product.to_string());
            all.push(cable.playback.to_string());
            all.push(cable.capture.to_string());
        }
        for text in all {
            assert!(text.is_ascii(), "not ascii: {text}");
        }
    }
}
