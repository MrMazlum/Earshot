//! BlackHole on macOS: finding it, and walking the user through installing it.
//!
//! macOS will not let a user-space program invent a recording device any more than Windows will —
//! an audio endpoint is owned by the system, and publishing one means a userspace audio *driver*
//! (an `AudioServerPlugin` in `/Library/Audio/Plug-Ins/HAL`) that the user approves and that needs
//! the machine's administrator. So Earshot borrows a driver somebody else already wrote.
//!
//! That driver is [BlackHole](https://existential.audio/blackhole/), which is free, open source
//! (GPL-3.0 — see the note below) and does one thing: it is a loopback, a device whose output is
//! fed straight back into its own input.
//!
//! ```text
//!   Earshot  ->  "BlackHole 2ch"  (we play into it)
//!                     |
//!                     v
//!   Discord  <-  "BlackHole 2ch"  (the user picks the same device as an input)
//! ```
//!
//! ⚠️ **The two ends share one name**, unlike VB-Cable's `CABLE Input` / `CABLE Output`, because
//! BlackHole is a single device that is both. So unlike Windows there is nothing to disambiguate
//! and no "pick the *other* one" warning to give: the string we open and the string the user picks
//! are the same, and every message here says so.
//!
//! ## Licence
//!
//! BlackHole is GPL-3.0, which sits fine beside this GPL-3.0 program. Even so, Earshot does not
//! bundle it, download it or run its installer — exactly the policy [`crate::cable`] follows on
//! Windows: detect what is installed, explain it in terms of what the user is trying to do, open
//! the vendor's own page, and let the user install it. A third-party system audio driver is not
//! something this program installs for anyone.

/// A BlackHole build this program knows how to use.
pub struct Variant {
    /// Shown to the user; also the number of channels, which is the only thing that differs between
    /// the builds.
    pub product: &'static str,
    /// The device name CoreAudio reports, and the substring we match on. Playback and capture are
    /// the same device, so this is both what we open and what the user picks.
    pub device: &'static str,
}

/// Ordered by preference: the 2-channel build first, because it is the one the documentation names,
/// it is the smallest, and a microphone is mono anyway — the wider builds exist to patch many
/// sources together and cost nothing here but are not what a first-time user installed.
pub const KNOWN: &[Variant] = &[
    Variant {
        product: "BlackHole 2ch",
        device: "BlackHole 2ch",
    },
    Variant {
        product: "BlackHole 16ch",
        device: "BlackHole 16ch",
    },
    Variant {
        product: "BlackHole 64ch",
        device: "BlackHole 64ch",
    },
    Variant {
        product: "BlackHole 128ch",
        device: "BlackHole 128ch",
    },
];

/// The page to send people to. The vendor's own, never a mirror.
pub const DOWNLOAD_PAGE: &str = "https://existential.audio/blackhole/";

/// The Homebrew cask, which is how most people on a Mac will already install this and is worth
/// naming because it needs no download page, no unzip and no dragging.
pub const BREW_CASK: &str = "blackhole-2ch";

/// The first line of [`missing_message`], and the way any other part of the program can tell "no
/// BlackHole" apart from the twenty other reasons starting the engine can fail. The tray needs
/// that: this is the one failure it can offer to *fix*, rather than merely report.
pub const MISSING_HEADLINE: &str = "BlackHole is not installed.";

/// Whether an engine error is the missing-BlackHole one.
pub fn is_missing(error: &str) -> bool {
    error.starts_with(MISSING_HEADLINE)
}

/// A BlackHole install that is actually present on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub product: &'static str,
    /// The device to hand to [`crate::audio::open`] — and the one the user picks in Discord, because
    /// they are the same device.
    pub device: String,
}

/// Picks the first known build present in a list of output device names.
///
/// Pure and case-insensitive, so it is testable on a machine that has no BlackHole and no macOS.
pub fn identify(devices: &[String]) -> Option<Found> {
    for variant in KNOWN {
        let want = variant.device.to_lowercase();
        if let Some(device) = devices.iter().find(|d| d.to_lowercase().contains(&want)) {
            return Some(Found {
                product: variant.product,
                device: device.clone(),
            });
        }
    }
    None
}

/// Asks the audio host what output devices exist, then [`identify`]s BlackHole among them.
pub fn find() -> Result<Option<Found>, String> {
    Ok(identify(&crate::audio::output_device_names()?))
}

/// What to say when there is no BlackHole. Also the text of the error the engine returns, so a user
/// who never sees the interactive path still gets the whole story.
///
/// `devices` is listed because the one thing worse than "not found" is "not found" with no clue
/// about what *was* found — somebody with a loopback under another name can then point `--device`
/// straight at it.
pub fn missing_message(devices: &[String]) -> String {
    let found = if devices.is_empty() {
        "  (none - this machine reports no output devices at all)".to_string()
    } else {
        devices
            .iter()
            .map(|d| format!("  - {d}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "{MISSING_HEADLINE}\n\
         \n\
         macOS will not let a program invent a microphone, so Earshot plays into a virtual\n\
         audio driver that somebody else wrote, and you pick that same device as your\n\
         microphone. BlackHole is free, open source, and installs once:\n\
         \n    brew install --cask {BREW_CASK}\n\
         \n\
         or from its own page:\n\
         \n    {DOWNLOAD_PAGE}\n\
         \n\
         After installing it (a reboot, or `sudo killall coreaudiod`, may be needed), run this\n\
         again and Earshot will find it.\n\
         \n\
         Already have a different loopback? Point Earshot at its output device by hand:\n\
         \n    earshot-receiver --virtual-mic --device \"<its output device>\"\n\
         \n\
         Output devices on this machine right now:\n{found}"
    )
}

/// The steps, once the download page is open. Written as what to click, not what to understand.
///
/// Every line is the answer to somebody actually getting it wrong:
///
/// - BlackHole ships in four builds and only the channel count differs. A user who grabs the 64-
///   channel one from the top of the page gets a device that works but is not the one any guide
///   names, and its name in Discord's list will not be `BlackHole 2ch`.
/// - The downloaded disk image opens with an installer that needs the administrator password; the
///   window can be dismissed with a "Cancel" that looks like it worked.
/// - CoreAudio keeps its device list until it is restarted, so a freshly installed BlackHole may
///   not appear at all until the machine is rebooted or `coreaudiod` is restarted.
///
/// `pub` because it is one half of the guided path and the rest is macOS-only; keeping it private
/// would make it dead code on every other platform.
pub fn install_steps() -> String {
    format!(
        "On the page that just opened (or with Homebrew:  brew install --cask {BREW_CASK}):\n\
         \n  \
         1. Download the 2-channel build:  BlackHole 2ch.  The page also offers 16, 64\n     \
            and 128-channel builds; you do not want them. Any of them works, but only\n     \
            'BlackHole 2ch' is the name every guide - including this one - tells you to\n     \
            pick in Discord.\n\
         \n  \
         2. Open the downloaded .pkg and click through the installer. It asks for your\n     \
            administrator password, because an audio driver goes in /Library.\n\
         \n  \
         3. Reboot, or run this in a terminal to reload the audio system without one:\n\
         \n       sudo killall coreaudiod\n\
         \n     \
            CoreAudio reads the driver list when it starts, so a new device is often\n     \
            invisible until it is restarted.\n\
         \n  \
         4. Start Earshot again. It should now find BlackHole by itself.\n\
         \n\
         If the page did not open by itself, it is:  {DOWNLOAD_PAGE}"
    )
}

/// The whole macOS setup, in the order it has to happen, short enough to fit in a dialog.
pub fn quickstart(device: &str) -> String {
    format!(
        "1. On your phone, open the Earshot app. This Mac appears in its list by\n   \
            itself - tap it, then press Start.\n\
         \n\
          2. In Discord, Zoom, OBS or whatever you are using, open the microphone\n   \
            list and pick:\n\
         \n       \
              {device}\n\
         \n   \
            Earshot plays into BlackHole and you record from the same device, so there\n   \
            is only one name to remember - unlike Windows, where the two ends differ.\n\
         \n\
          3. Talk. The menu-bar icon turns green while audio is actually arriving; blue\n   \
            means Earshot is ready but the phone has not sent anything yet.\n\
         \n    \
            Blue forever, with the phone saying it is sending? That is usually the macOS\n    \
            firewall dropping the audio before it arrives. The Help entry in this menu\n    \
            has the fix."
    )
}

/// Hands the download page to whatever the user's default browser is.
///
/// `open` is macOS's own opener and takes the URL as a plain argument, the way `rundll32` does on
/// Windows. No shell is involved, so nothing needs quoting.
#[cfg(target_os = "macos")]
pub fn open_download_page() -> Result<(), String> {
    std::process::Command::new("open")
        .arg(DOWNLOAD_PAGE)
        .status()
        .map_err(|e| format!("could not open a browser: {e}"))?;
    Ok(())
}

/// Everything below is the interactive path, which only makes sense on the platform that needs it.
#[cfg(target_os = "macos")]
mod interactive {
    use super::{find, install_steps, missing_message, open_download_page, Found, DOWNLOAD_PAGE};
    use std::io::Write;
    use std::time::{Duration, Instant};

    /// How long to keep looking after the user has been sent to the download page. Long enough to
    /// download, install and reboot-without-reboot without the program giving up first; the user can
    /// always Ctrl-C, and a decline is one keypress away before this starts.
    const WAIT: Duration = Duration::from_secs(15 * 60);
    const POLL: Duration = Duration::from_secs(3);

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

    /// Polls until BlackHole shows up, or until [`WAIT`] runs out.
    fn wait_for_blackhole() -> Result<Option<Found>, String> {
        let started = Instant::now();
        let mut last_note = Instant::now();
        println!();
        println!("Waiting for BlackHole to appear... (Ctrl-C to stop)");
        while started.elapsed() < WAIT {
            if let Some(found) = find()? {
                return Ok(Some(found));
            }
            if last_note.elapsed() >= Duration::from_secs(30) {
                last_note = Instant::now();
                println!(
                    "  still nothing after {}s - if you have installed it, CoreAudio may not have \
                     noticed; try  sudo killall coreaudiod  or a reboot",
                    started.elapsed().as_secs()
                );
            }
            std::thread::sleep(POLL);
        }
        Ok(None)
    }

    /// Called before the engine starts, when a virtual microphone has been asked for.
    ///
    /// Success means BlackHole exists *now*, so the engine's own lookup cannot then fail. Doing this
    /// here rather than inside the engine is deliberate: the engine runs on the audio thread's
    /// setup path and has no business printing to a terminal or reading from stdin.
    pub fn preflight() -> Result<(), String> {
        if let Some(found) = find()? {
            println!("Virtual mic     {} (loopback)", found.product);
            return Ok(());
        }

        let devices = crate::audio::output_device_names().unwrap_or_default();
        println!("{}", missing_message(&devices));
        println!();

        let answer = ask("Open the BlackHole download page now? [Y/n] ");
        // Enter means yes: it is the only way forward from here, and the alternative is retyping a
        // URL by hand. Anything else, including no answer at all, means no.
        if !matches!(answer.as_deref(), Some("") | Some("y") | Some("yes")) {
            return Err("no BlackHole, and the download page was not opened. \
                        Install it and run this again."
                .to_string());
        }

        if let Err(e) = open_download_page() {
            println!("{e}");
        }
        println!();
        println!("{}", install_steps());

        match wait_for_blackhole()? {
            Some(found) => {
                println!();
                println!("Found it: {} - carrying on.", found.product);
                println!(
                    "Remember to pick \"{}\" as your microphone in Discord - the same device \
                     Earshot plays into.",
                    found.device
                );
                Ok(())
            }
            None => Err(format!(
                "still no BlackHole after 15 minutes. Finish installing it (and reboot or run \
                 'sudo killall coreaudiod' if asked), then run Earshot again.\n{DOWNLOAD_PAGE}"
            )),
        }
    }
}

#[cfg(target_os = "macos")]
pub use interactive::preflight;

/// Nothing to do anywhere else: Windows borrows VB-Cable ([`crate::cable`]) and Linux invents its
/// own device, so only macOS reaches into this module.
#[cfg(not(target_os = "macos"))]
pub fn preflight() -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// The name CoreAudio reports is exactly the driver's own - no vendor suffix, unlike Windows.
    #[test]
    fn the_real_device_name_is_recognised() {
        let found = identify(&names(&[
            "MacBook Air Speakers",
            "BlackHole 2ch",
        ]))
        .expect("BlackHole should be found");
        assert_eq!(found.product, "BlackHole 2ch");
        assert_eq!(found.device, "BlackHole 2ch");
    }

    /// Unlike the Windows cable, what we open and what the user picks are the same string. That is
    /// the whole difference between the two platforms and worth pinning down.
    #[test]
    fn the_device_we_open_is_the_device_the_user_picks() {
        for variant in KNOWN {
            assert_eq!(
                variant.product, variant.device,
                "{} has two names and should have one",
                variant.product
            );
        }
    }

    #[test]
    fn a_machine_with_no_blackhole_finds_nothing() {
        assert!(identify(&names(&["MacBook Air Speakers", "External Headphones"])).is_none());
    }

    /// The 2-channel build wins even when the wider ones are present and sorted first.
    #[test]
    fn the_two_channel_build_is_preferred_over_the_wider_ones() {
        let found = identify(&names(&[
            "BlackHole 64ch",
            "BlackHole 16ch",
            "BlackHole 2ch",
        ]))
        .expect("one of them should be found");
        assert_eq!(found.product, "BlackHole 2ch");
    }

    /// A wider build alone is still usable; picking it beats finding nothing.
    #[test]
    fn a_wider_build_alone_is_still_usable() {
        let found = identify(&names(&["BlackHole 64ch"])).expect("should be found");
        assert_eq!(found.product, "BlackHole 64ch");
        assert_eq!(found.device, "BlackHole 64ch");
    }

    /// A real output device must not be mistaken for the loopback.
    #[test]
    fn a_speaker_is_not_a_blackhole() {
        assert!(identify(&names(&["MacBook Pro Speakers", "AirPods Pro"])).is_none());
    }

    /// The message has to carry the address of the fix and the evidence of the problem, or it is
    /// just "no".
    #[test]
    fn the_missing_message_names_the_download_and_lists_what_was_found() {
        let msg = missing_message(&names(&["MacBook Air Speakers"]));
        assert!(msg.contains(DOWNLOAD_PAGE), "{msg}");
        assert!(msg.contains(BREW_CASK), "{msg}");
        assert!(msg.contains("MacBook Air Speakers"), "{msg}");
        assert!(msg.contains("--device"), "{msg}");
    }

    #[test]
    fn the_missing_message_copes_with_a_machine_that_has_no_outputs() {
        let msg = missing_message(&[]);
        assert!(msg.contains("no output devices"), "{msg}");
    }

    /// The tray offers to fix exactly one kind of failure, and it tells them apart by this.
    #[test]
    fn a_missing_blackhole_is_recognisable_as_such() {
        assert!(is_missing(&missing_message(&names(&["Speakers"]))));
        assert!(is_missing(&missing_message(&[])));
        assert!(!is_missing("cannot bind 0.0.0.0:47811: address already in use"));
        assert!(!is_missing(""));
    }

    /// Four builds on one page and only the channel count differs; naming the one to take is the
    /// whole value of the step list.
    #[test]
    fn the_install_steps_name_the_build_to_take_and_why_it_matters() {
        let steps = install_steps();
        assert!(steps.contains("BlackHole 2ch"), "{steps}");
        assert!(steps.contains("administrator password"), "{steps}");
        assert!(steps.contains("sudo killall coreaudiod"), "{steps}");
    }

    /// A macOS console is UTF-8, but this text is also copied into the tray's dialogs and the PR
    /// rules out fancy punctuation in anything that reaches a screen. Keep it plain everywhere.
    #[test]
    fn every_message_is_plain_ascii() {
        let mut all = vec![
            missing_message(&names(&["MacBook Air Speakers"])),
            install_steps(),
            quickstart("BlackHole 2ch"),
        ];
        all.push(DOWNLOAD_PAGE.to_string());
        all.push(BREW_CASK.to_string());
        for variant in KNOWN {
            all.push(variant.product.to_string());
            all.push(variant.device.to_string());
        }
        for text in all {
            assert!(text.is_ascii(), "not ascii: {text}");
        }
    }
}
