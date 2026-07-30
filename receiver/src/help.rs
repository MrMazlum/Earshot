//! The things a user needs told when nothing is working, in one place.
//!
//! Both front-ends print this — the terminal after 25 seconds of silence, the tray from its Help
//! entry — and the Windows half of it is the half no build on the machine this was written on ever
//! runs. Keeping the text here rather than in `main.rs` means a Linux `cargo test` still checks it.
//!
//! Everything in this module is plain ASCII on purpose. A Windows console is not reliably UTF-8:
//! on the Turkish code page (857) or a Western-European one (1252), an em dash or an ellipsis
//! arrives as mojibake, and this is the text somebody reads when they are already frustrated.

/// Why no audio has arrived, in the order the causes are actually likely.
///
/// The firewall leads on Windows for a reason: inbound UDP to a program with no rule is dropped by
/// default, the prompt that would have offered to allow it needs an administrator and is often
/// never shown at all, and the symptom is indistinguishable from a phone that is not sending. On
/// Linux the default is the other way round — nothing blocks inbound unless the user turned a
/// firewall on — so the same hint leads with the network instead.
///
/// Built as a string rather than printed line by line so both branches can be read back in a test.
pub fn nothing_arriving_hint(port: u16, windows: bool) -> String {
    let mut lines: Vec<String> = vec!["  ! Nothing has arrived yet.".into()];
    if windows {
        lines.extend([
            "    If the phone says it is sending, Windows Firewall is the usual reason: it".into(),
            "    drops incoming UDP for a program that has no rule, and the prompt that would".into(),
            "    have asked you needs an administrator. In an Administrator PowerShell:".into(),
            String::new(),
            firewall_rule(port),
            String::new(),
            "    Then check: the phone and this PC on the same Wi-Fi, and this network set to".into(),
            "    Private rather than Public in Windows settings.".into(),
        ]);
    } else {
        lines.extend([
            "    Check the phone and this PC are on the same Wi-Fi, and that the code or".into(),
            "    address in the app matches the one above. If a firewall is running:".into(),
            String::new(),
            format!("      sudo ufw allow {port}/udp"),
        ]);
    }
    lines.extend([
        String::new(),
        "    Guest or client-isolation Wi-Fi blocks device-to-device traffic entirely; a".into(),
        "    phone hotspot with the PC joined to it is a reliable way to rule that out.".into(),
    ]);
    lines.join("\n")
}

/// The one command that fixes the most common Windows failure. Indented to sit inside the hint;
/// callers that want it on its own trim it.
pub fn firewall_rule(port: u16) -> String {
    format!(
        "      New-NetFirewallRule -DisplayName Earshot -Direction Inbound \
         -Protocol UDP -LocalPort {port} -Action Allow"
    )
}

/// The tray's Help box: the same knowledge, laid out for a dialog rather than a scrolling terminal.
pub fn troubleshooting(port: u16, windows: bool) -> String {
    let mut out = String::from("The phone says it is sending, and Earshot says it is waiting.\n\n");
    if windows {
        out.push_str(
            "Nine times out of ten this is Windows Firewall. It silently drops\n\
             incoming UDP for a program that has no rule, and the prompt that would\n\
             have asked you needs an administrator - so it is often never shown.\n\
             \n\
             Open PowerShell as Administrator (right-click Start, \"Terminal\n\
             (Admin)\") and paste this one line:\n\n",
        );
        out.push_str(firewall_rule(port).trim_start());
        out.push_str(
            "\n\n\
             Then check that this network is set to Private rather than Public in\n\
             Windows settings - Public turns the firewall's strictest profile on.\n\n",
        );
    } else {
        out.push_str(
            "Check the phone and this PC are on the same Wi-Fi, and that the code in\n\
             the app matches the one in this menu. If a firewall is running:\n\n",
        );
        out.push_str(&format!("    sudo ufw allow {port}/udp\n\n"));
    }
    out.push_str(
        "Still nothing?\n\
         \n  \
         - Guest Wi-Fi and \"client isolation\" block device-to-device traffic\n    \
           outright, and no firewall rule will help. Turning on the phone's\n    \
           hotspot and joining this PC to it rules that out in a minute.\n  \
         - A PC on both Ethernet and Wi-Fi gets a code for each. Use the one for\n    \
           the network the phone is on.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Windows console is not reliably UTF-8, and this is the text somebody reads when they are
    /// already having a bad time. Doc comments are exempt; anything that reaches a screen is not.
    #[test]
    fn every_word_of_it_is_plain_ascii() {
        for windows in [true, false] {
            for text in [
                nothing_arriving_hint(47811, windows),
                troubleshooting(47811, windows),
            ] {
                assert!(text.is_ascii(), "not ascii (windows={windows}):\n{text}");
            }
        }
        assert!(firewall_rule(47811).is_ascii());
    }

    /// The Windows branch is the one that matters and the one no build on this machine ever runs,
    /// so it is checked from here rather than trusted.
    #[test]
    fn the_windows_hint_hands_over_a_firewall_rule_for_the_actual_port() {
        let hint = nothing_arriving_hint(47899, true);
        assert!(hint.contains("New-NetFirewallRule"), "{hint}");
        assert!(hint.contains("-LocalPort 47899"), "{hint}");
        assert!(!hint.contains("ufw"), "that is the Linux advice: {hint}");
    }

    #[test]
    fn the_linux_hint_does_not_tell_anyone_to_open_powershell() {
        let hint = nothing_arriving_hint(47811, false);
        assert!(hint.contains("sudo ufw allow 47811/udp"), "{hint}");
        assert!(!hint.contains("PowerShell"), "{hint}");
    }

    /// Both platforms share the closing paragraph: guest Wi-Fi defeats everything above it, and
    /// saying so is what stops the next hour going into firewall rules that were never the problem.
    #[test]
    fn both_platforms_mention_client_isolation() {
        for windows in [true, false] {
            assert!(
                nothing_arriving_hint(47811, windows).contains("client-isolation"),
                "windows={windows}"
            );
            assert!(
                troubleshooting(47811, windows).contains("client isolation"),
                "windows={windows}"
            );
        }
    }

    /// The tray shows this in a message box, where a wrong port is a copy-pasted command that
    /// silently does nothing.
    #[test]
    fn the_tray_help_carries_the_real_port_too() {
        assert!(troubleshooting(47899, true).contains("-LocalPort 47899"));
        assert!(troubleshooting(47899, false).contains("47899/udp"));
    }
}
