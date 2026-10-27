//! The things a user needs told when nothing is working, in one place.
//!
//! Both front-ends print this — the terminal after 25 seconds of silence, the tray from its Help
//! entry — and the Windows half of it is the half no build on the machine this was written on ever
//! runs. Keeping the text here rather than in `main.rs` means a Linux `cargo test` still checks it.
//!
//! Everything in this module is plain ASCII on purpose. A Windows console is not reliably UTF-8:
//! on the Turkish code page (857) or a Western-European one (1252), an em dash or an ellipsis
//! arrives as mojibake, and this is the text somebody reads when they are already frustrated.

/// Which platform's advice to give. Passed in rather than read from `cfg!` here, so every branch
/// is exercised by a test on every platform — the same reason the Windows text lives in this file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
}

impl Os {
    /// The platform this binary was built for.
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
}

/// Why no audio has arrived, in the order the causes are actually likely.
///
/// The firewall leads on Windows for a reason: inbound UDP to a program with no rule is dropped by
/// default, the prompt that would have offered to allow it needs an administrator and is often
/// never shown at all, and the symptom is indistinguishable from a phone that is not sending. On
/// Linux the default is the other way round — nothing blocks inbound unless the user turned a
/// firewall on — so the same hint leads with the network instead. macOS sits between the two: its
/// application firewall is off unless the user turned it on, but once on it blocks a program with
/// no rule silently, exactly like Windows and with no one-line command to fix it.
///
/// Built as a string rather than printed line by line so both branches can be read back in a test.
pub fn nothing_arriving_hint(port: u16, os: Os) -> String {
    let mut lines: Vec<String> = vec!["  ! Nothing has arrived yet.".into()];
    match os {
        Os::Windows => lines.extend([
            "    If the phone says it is sending, Windows Firewall is the usual reason: it".into(),
            "    drops incoming UDP for a program that has no rule, and the prompt that would".into(),
            "    have asked you needs an administrator. In an Administrator PowerShell:".into(),
            String::new(),
            firewall_rule(port),
            String::new(),
            "    Then check: the phone and this PC on the same Wi-Fi, and this network set to".into(),
            "    Private rather than Public in Windows settings.".into(),
        ]),
        Os::Linux => lines.extend([
            "    Check the phone and this PC are on the same Wi-Fi, and that the PC the".into(),
            "    app is pointed at is the address above. If a firewall is running:".into(),
            String::new(),
            ufw_rule(port),
            String::new(),
            "    That opens the port to your own network only. If your addresses start".into(),
            "    10. or 172.16-172.31 rather than 192.168, put that range in instead".into(),
            "    (10.0.0.0/8 or 172.16.0.0/12) - the address above shows which you are on.".into(),
        ]),
        Os::Macos => lines.extend([
            "    Check the phone and this PC are on the same Wi-Fi, and that the PC the".into(),
            "    app is pointed at is the address above.".into(),
            String::new(),
            "    If the macOS firewall is on, it can drop incoming UDP for a program with".into(),
            "    no rule and never say so. Open:".into(),
            String::new(),
            macos_firewall_path(),
            String::new(),
            "    and make sure Earshot is allowed to accept incoming connections - add it".into(),
            "    with the + button if it is not in the list.".into(),
        ]),
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
///
/// `-Profile Private -RemoteAddress LocalSubnet` are not decoration. Without them the rule is
/// permanent, applies on every network profile and accepts from any source, so the next coffee-shop
/// Wi-Fi this laptop joins can reach a receiver that does not authenticate its sender. Scoped this
/// way the hole closes when the machine leaves the network it was opened for. It also means the
/// rule does nothing while Windows has the network marked Public - which is why every caller of
/// this says "set the network to Private" in the next breath.
///
/// One physical line: this is pasted into a console, and a backtick continuation that loses its
/// trailing space on the way through a message box fails in a way nobody can read.
pub fn firewall_rule(port: u16) -> String {
    format!(
        "      New-NetFirewallRule -DisplayName Earshot -Direction Inbound \
         -Protocol UDP -LocalPort {port} -Action Allow \
         -Profile Private -RemoteAddress LocalSubnet"
    )
}

/// The Linux equivalent, scoped the same way and for the same reason.
///
/// `ufw` has no `LocalSubnet` token, so the range has to be named. 192.168/16 is the one home
/// routers hand out; the other two private blocks are spelled out by the caller rather than guessed
/// at, because a rule that silently matches nothing is worse than no rule at all.
pub fn ufw_rule(port: u16) -> String {
    format!("      sudo ufw allow from 192.168.0.0/16 to any port {port} proto udp")
}

/// Where the macOS firewall's per-application list lives. There is no command-line equivalent worth
/// handing out: the modern `socketfilterfw` needs root, names the binary by an absolute path that
/// moves, and is undocumented. Sending the user to the pane that Apple keeps is the honest advice.
pub fn macos_firewall_path() -> String {
    "      System Settings > Network > Firewall > Options".to_string()
}

/// The tray's Help box: the same knowledge, laid out for a dialog rather than a scrolling terminal.
pub fn troubleshooting(port: u16, os: Os) -> String {
    let mut out = String::from("The phone says it is sending, and Earshot says it is waiting.\n\n");
    match os {
        Os::Windows => {
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
        }
        Os::Linux => {
            out.push_str(
                "Check the phone and this PC are on the same Wi-Fi, and that the PC the app\n\
                 is pointed at is the address in this menu. If a firewall is running:\n\n",
            );
            out.push_str(ufw_rule(port).trim_start());
            out.push_str(
                "\n\n\
                 That opens the port to your own network only. If your addresses start\n\
                 10. or 172.16-172.31 rather than 192.168, put that range in instead\n\
                 (10.0.0.0/8 or 172.16.0.0/12).\n\n",
            );
        }
        Os::Macos => {
            out.push_str(
                "Check the phone and this PC are on the same Wi-Fi, and that the PC the app\n\
                 is pointed at is the address in this menu.\n\
                 \n\
                 If the macOS firewall is on, it silently drops incoming UDP for a program\n\
                 that has no rule. Open:\n\n",
            );
            out.push_str(macos_firewall_path().trim_start());
            out.push_str(
                "\n\n\
                 and make sure Earshot is allowed to accept incoming connections - add it\n\
                 with the + button if it is not in the list.\n\n",
            );
        }
    }
    out.push_str(
        "Still nothing?\n\
         \n  \
         - Guest Wi-Fi and \"client isolation\" block device-to-device traffic\n    \
           outright, and no firewall rule will help. Turning on the phone's\n    \
           hotspot and joining this PC to it rules that out in a minute.\n  \
         - A PC on both Ethernet and Wi-Fi has an address on each, and the phone\n    \
           only finds the one on its own network. Use that one.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Os; 3] = [Os::Windows, Os::Linux, Os::Macos];

    /// A Windows console is not reliably UTF-8, and this is the text somebody reads when they are
    /// already having a bad time. Doc comments are exempt; anything that reaches a screen is not.
    #[test]
    fn every_word_of_it_is_plain_ascii() {
        for os in ALL {
            for text in [
                nothing_arriving_hint(47811, os),
                troubleshooting(47811, os),
            ] {
                assert!(text.is_ascii(), "not ascii (os={os:?}):\n{text}");
            }
        }
        assert!(firewall_rule(47811).is_ascii());
        assert!(macos_firewall_path().is_ascii());
    }

    /// The Windows branch is the one that matters and the one no build on this machine ever runs,
    /// so it is checked from here rather than trusted.
    #[test]
    fn the_windows_hint_hands_over_a_firewall_rule_for_the_actual_port() {
        let hint = nothing_arriving_hint(47899, Os::Windows);
        assert!(hint.contains("New-NetFirewallRule"), "{hint}");
        assert!(hint.contains("-LocalPort 47899"), "{hint}");
        assert!(!hint.contains("ufw"), "that is the Linux advice: {hint}");
    }

    #[test]
    fn the_linux_hint_does_not_tell_anyone_to_open_powershell() {
        let hint = nothing_arriving_hint(47811, Os::Linux);
        assert!(hint.contains("port 47811 proto udp"), "{hint}");
        assert!(!hint.contains("PowerShell"), "{hint}");
    }

    /// The macOS branch must send the user to the pane Apple keeps and must not borrow the Linux
    /// `ufw` advice, which does not exist on a Mac.
    #[test]
    fn the_macos_hint_points_at_the_macos_firewall_and_not_ufw() {
        for text in [
            nothing_arriving_hint(47811, Os::Macos),
            troubleshooting(47811, Os::Macos),
        ] {
            assert!(text.contains("System Settings"), "{text}");
            assert!(text.contains("Firewall"), "{text}");
            assert!(!text.contains("ufw"), "that is the Linux advice: {text}");
            assert!(!text.contains("PowerShell"), "that is the Windows advice: {text}");
        }
    }

    /// The rules we hand out are scoped to the local network, and this is the test that keeps them
    /// that way. A rule without these is permanent, matches every network profile and accepts from
    /// any source - so the next public Wi-Fi this machine joins can reach a receiver that does not
    /// authenticate whoever is sending to it. Widening it back is a one-word edit; this is what
    /// makes that edit fail the build instead of shipping.
    #[test]
    fn the_rules_we_hand_out_are_scoped_to_the_local_network() {
        let windows = firewall_rule(47811);
        assert!(windows.contains("-Profile Private"), "{windows}");
        assert!(windows.contains("-RemoteAddress LocalSubnet"), "{windows}");

        let linux = ufw_rule(47811);
        assert!(linux.contains("from 192.168.0.0/16"), "{linux}");
        // `ufw allow 47811/udp` is the bare form, and it is open to the world.
        assert!(!linux.contains("allow 47811/udp"), "{linux}");

        // Both front-ends carry the scoping, not just the helper in isolation.
        for text in [
            nothing_arriving_hint(47811, Os::Windows),
            troubleshooting(47811, Os::Windows),
        ] {
            assert!(text.contains("-RemoteAddress LocalSubnet"), "{text}");
        }
        for text in [
            nothing_arriving_hint(47811, Os::Linux),
            troubleshooting(47811, Os::Linux),
        ] {
            assert!(text.contains("from 192.168.0.0/16"), "{text}");
        }
    }

    /// The scoped Windows rule only applies on a Private network, so telling somebody to paste it
    /// without also telling them to check the profile hands them a rule that does nothing.
    #[test]
    fn the_windows_advice_mentions_the_private_profile_it_depends_on() {
        for text in [
            nothing_arriving_hint(47811, Os::Windows),
            troubleshooting(47811, Os::Windows),
        ] {
            assert!(text.contains("Private"), "{text}");
        }
    }

    /// A user on 10.x pastes a 192.168 rule, sees nothing change, and concludes the firewall was
    /// not the problem. Naming the other two ranges is what stops that.
    #[test]
    fn the_linux_advice_names_the_other_private_ranges() {
        for text in [
            nothing_arriving_hint(47811, Os::Linux),
            troubleshooting(47811, Os::Linux),
        ] {
            assert!(text.contains("10.0.0.0/8"), "{text}");
            assert!(text.contains("172.16.0.0/12"), "{text}");
        }
    }

    /// Every platform shares the closing paragraph: guest Wi-Fi defeats everything above it, and
    /// saying so is what stops the next hour going into firewall rules that were never the problem.
    #[test]
    fn every_platform_mentions_client_isolation() {
        for os in ALL {
            assert!(
                nothing_arriving_hint(47811, os).contains("client-isolation"),
                "os={os:?}"
            );
            assert!(
                troubleshooting(47811, os).contains("client isolation"),
                "os={os:?}"
            );
        }
    }

    /// The tray shows this in a message box, where a wrong port is a copy-pasted command that
    /// silently does nothing.
    #[test]
    fn the_tray_help_carries_the_real_port_too() {
        assert!(troubleshooting(47899, Os::Windows).contains("-LocalPort 47899"));
        assert!(troubleshooting(47899, Os::Linux).contains("port 47899 proto udp"));
    }
}
