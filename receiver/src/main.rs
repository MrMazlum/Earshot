//! `earshot-receiver` — listens for the phone and plays what it hears.
//!
//! This is the terminal front-end. The receiving itself lives in [`earshot::engine`], which the
//! tray application (`earshot-tray`) drives too — keep behaviour changes in there, not here.
//!
//! Scope: one direction, raw PCM, no discovery, no encryption. `--virtual-mic` makes other
//! applications see the phone as an input device.

use earshot::audio;
use earshot::cable;
use earshot::engine::{self, Config, Engine};
use earshot::pairing::Code;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Nothing has arrived for this long and something is probably in the way. Generous on purpose: a
/// user who is still walking over to pick up their phone should not be lectured about firewalls.
const FIRST_PACKET_HINT: Duration = Duration::from_secs(25);

#[derive(Default)]
struct Args {
    config: Config,
    list_devices: bool,
    remove_virtual_mic: bool,
}

const USAGE: &str = "\
earshot-receiver - turns your phone into this machine's microphone

USAGE:
    earshot-receiver [OPTIONS]

OPTIONS:
    -l, --listen <ADDR:PORT>   where to listen        [default: 0.0.0.0:47811]
    -b, --buffer-ms <MS>       jitter buffer depth    [default: 60]
                               lower = less delay, more crackle on a busy network
    -d, --device <NAME>        output device, substring match [default: system default]
        --list-devices         print the output devices and exit
    -v, --verbose              log every dropped/odd packet
    -h, --help                 this text

VIRTUAL MICROPHONE:
    -m, --virtual-mic          make other apps see Earshot as an input device, instead of
                               playing out of the speakers. Then pick it in the input list
                               of Discord, OBS or Zoom
        --virtual-mic-name <N> what to call it, on Linux  [default: Earshot]
        --remove-virtual-mic   delete it and exit. It otherwise stays until you reboot, so
                               apps do not forget which input you chose

    Linux    creates the device itself - nothing to install.
    Windows  borrows a virtual cable, because it cannot create one. With --virtual-mic
             it looks for VB-Cable and, if none is installed, offers to open the
             download page and then waits for it. Earshot plays into 'CABLE Input';
             you pick 'CABLE Output' as your microphone.

TIP:
    earshot-tray puts all of this in the system tray instead, and can start at login.
    Linux only for now.
";

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-l" | "--listen" => args.config.listen = value("--listen")?,
            "-b" | "--buffer-ms" => {
                let v = value("--buffer-ms")?;
                args.config.buffer_ms = v
                    .parse()
                    .map_err(|_| format!("--buffer-ms wants a number, got '{v}'"))?;
            }
            "-d" | "--device" => args.config.device = Some(value("--device")?),
            "--list-devices" => args.list_devices = true,
            "-v" | "--verbose" => args.config.verbose = true,
            "-m" | "--virtual-mic" => args.config.virtual_mic = true,
            "--virtual-mic-name" => args.config.virtual_mic_name = value("--virtual-mic-name")?,
            "--remove-virtual-mic" => args.remove_virtual_mic = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option '{other}'\n\n{USAGE}")),
        }
    }
    if args.config.listen.parse::<std::net::SocketAddr>().is_err()
        && !args.config.listen.contains(':')
    {
        args.config.listen = format!("0.0.0.0:{}", args.config.listen);
    }
    Ok(args)
}

/// Why no audio has arrived, in the order the causes are actually likely.
///
/// The firewall is first on Windows for a reason: inbound UDP to a program with no rule is dropped
/// by default, the prompt that would have offered to allow it needs an administrator and is often
/// never shown at all, and the symptom is indistinguishable from a phone that is not sending. On
/// Linux the default is the other way round — nothing blocks inbound unless the user turned a
/// firewall on — so the same hint leads with the network instead.
///
/// Built as a string rather than printed line by line so both branches can be read back in a test.
/// The Windows text is the one that matters most and the one no local build ever runs.
fn nothing_arriving_hint(port: u16, windows: bool) -> String {
    let mut lines: Vec<String> = vec!["  ! Nothing has arrived yet.".into()];
    if windows {
        lines.extend([
            "    If the phone says it is sending, Windows Firewall is the usual reason: it".into(),
            "    drops incoming UDP for a program that has no rule, and the prompt that would".into(),
            "    have asked you needs an administrator. In an Administrator PowerShell:".into(),
            String::new(),
            format!(
                "      New-NetFirewallRule -DisplayName Earshot -Direction Inbound \
                 -Protocol UDP -LocalPort {port} -Action Allow"
            ),
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

fn print_nothing_arriving_hint(port: u16) {
    println!("{}", nothing_arriving_hint(port, cfg!(target_os = "windows")));
}

/// Prints and exits — but on Windows, waits first.
///
/// Double-clicking an `.exe` in Explorer opens a console window that closes the instant the process
/// ends, so an error message would flash past unread and the user would be left with nothing at all
/// to go on. One keypress is a small price in a terminal; a closed or piped stdin returns
/// immediately, so scripts and CI are unaffected.
fn die(message: &str, code: i32) -> ! {
    eprintln!("{message}");
    if cfg!(target_os = "windows") {
        eprintln!();
        eprint!("Press Enter to close this window.");
        let mut discard = String::new();
        let _ = std::io::stdin().read_line(&mut discard);
    }
    std::process::exit(code);
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => die(&e, 2),
    };

    if args.remove_virtual_mic {
        let name = &args.config.virtual_mic_name;
        match earshot::virtualmic::remove(name) {
            Ok(true) => println!("Removed the virtual microphone '{name}'."),
            // Only Linux has a device of its own to take away. On Windows the cable belongs to
            // VB-Cable, and saying "none was loaded" would imply Earshot had failed at something.
            Ok(false) if cfg!(target_os = "windows") => println!(
                "Nothing to remove. On Windows the virtual microphone is the cable's, not \
                 Earshot's - uninstall VB-Cable through Windows if you want it gone."
            ),
            Ok(false) => println!("No virtual microphone called '{name}' was loaded."),
            Err(e) => die(&format!("error: {e}"), 1),
        }
        return;
    }

    if args.list_devices {
        if let Err(e) = audio::list_devices() {
            die(&format!("cannot list devices: {e}"), 1);
        }
        return;
    }

    if let Err(e) = run(args.config) {
        die(&format!("error: {e}"), 1);
    }
}

fn run(config: Config) -> Result<(), String> {
    let buffer_ms = config.buffer_ms;
    let listen = config.listen.clone();

    println!("Earshot receiver");
    // Before the engine, not inside it: this may print pages of guidance and read from stdin, and
    // the engine's setup path runs on the receive thread where neither belongs. On success a cable
    // now exists, so the engine's own lookup cannot fail. No-op off Windows.
    if config.virtual_mic {
        cable::preflight()?;
    }
    let engine = Engine::start(config)?;
    let ready = engine.ready().clone();

    match &ready.virtual_mic {
        Some(name) => println!(
            "  output   virtual microphone \"{}\" ({} Hz, {} ch){}",
            name,
            ready.out_rate,
            ready.out_channels,
            if ready.virtual_mic_created {
                " - created just now"
            } else {
                ""
            }
        ),
        None => println!(
            "  output   {} @ {} Hz, {} ch",
            ready.out_device, ready.out_rate, ready.out_channels
        ),
    }
    println!("  buffer   {buffer_ms} ms");
    println!();
    let addresses = engine::lan_addresses();
    if let Some(best) = addresses.first() {
        // The code is the headline; the address underneath it is for the times it does not work.
        match Code::new(best.ip, ready.port) {
            Some(code) => {
                println!("  PAIRING CODE      {}", code.grouped());
                println!("  In the phone app, type those nine digits.");
                println!("  (that is {} port {}, if you would rather type it)", best.ip, ready.port);
            }
            // Only an unusual address or a far-off port gets here — see pairing::Code::new.
            None => println!(
                "  In the phone app, type:   {}   port {}   (no pairing code for this address)",
                best.ip, ready.port
            ),
        }
        if addresses.len() > 1 {
            // Ethernet and Wi-Fi both up, say. Which one the phone is on is not ours to know, so
            // offer a code for each rather than guessing.
            println!();
            println!("  This machine is on more than one network. If the code above does not work:");
            for other in &addresses[1..] {
                match Code::new(other.ip, ready.port) {
                    Some(code) => println!("      {}   ({})", code.grouped(), other),
                    None => println!("      {other}   (no pairing code for this address)"),
                }
            }
        }
    } else if let Some(ip) = engine::lan_ip() {
        println!("  In the phone app, try:   {ip}   port {}", ready.port);
        println!("  ! No ordinary LAN address found - that is the default route, and if a VPN");
        println!("    is up the phone will not be able to reach it.");
    } else {
        println!("  listening on {listen} (could not detect this machine's LAN address)");
    }
    if let Some(name) = &ready.virtual_mic {
        println!();
        println!("  In Discord / OBS / Zoom, pick this input:   {name}");
        println!("  Nothing plays out of the speakers in this mode - that is the point.");
        // Only Linux creates the device, so only Linux has one to take away again.
        if cfg!(target_os = "linux") {
            println!("  It survives restarts of this program. Remove it with:");
            println!("      earshot-receiver --remove-virtual-mic");
        }
    }
    println!();
    println!("Waiting for the phone... (Ctrl-C to stop)");

    let status = engine.status();
    let mut last_line = Instant::now();
    let started = Instant::now();
    let mut hinted = false;

    while engine.is_running() {
        for notice in status.take_notices() {
            println!("{notice}");
        }
        // The worst failure this program can have is a silent one: the phone says "sending", the
        // receiver says "waiting", and neither says why. A blocked inbound port looks exactly like
        // an idle phone from here, so after a while say so out loud - once.
        if !hinted
            && started.elapsed() >= FIRST_PACKET_HINT
            && status.packets.load(Ordering::Relaxed) == 0
        {
            hinted = true;
            println!();
            print_nothing_arriving_hint(ready.port);
            println!();
        }
        if last_line.elapsed() >= Duration::from_secs(1) {
            last_line = Instant::now();
            if status.connected.load(Ordering::Relaxed) {
                let ring_drops = status.ring_drops.load(Ordering::Relaxed);
                // Only shown once they happen, so the ordinary line stays readable. A trimmed
                // count that keeps climbing is clock drift being held in check.
                let extra = [
                    (ring_drops, "ring-drops"),
                    (status.trimmed.load(Ordering::Relaxed), "trimmed"),
                ]
                .iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, label)| format!("  {label} {n}"))
                .collect::<String>();
                println!(
                    "{:>5} pkt/s  {:>4} kbps  buffered {:>5.1} ms  lost {}  late {}  dup {}  underruns {}{}",
                    status.pkt_per_sec.load(Ordering::Relaxed),
                    status.kbps.load(Ordering::Relaxed),
                    status.buffered_ms(),
                    status.lost.load(Ordering::Relaxed),
                    status.late.load(Ordering::Relaxed),
                    status.duplicates.load(Ordering::Relaxed),
                    status.underruns.load(Ordering::Relaxed),
                    extra,
                );
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    // Anything left to say, then whatever killed the loop.
    for notice in status.take_notices() {
        println!("{notice}");
    }
    match status.fatal() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Windows console is not reliably UTF-8. On the Turkish code page (857) or a
    /// Western-European one (1252), an em dash or an ellipsis renders as mojibake — and `--help`
    /// and this hint are the first things a confused user reads. Doc comments are exempt; anything
    /// that reaches a terminal is not.
    #[test]
    fn everything_printed_to_a_terminal_is_plain_ascii() {
        assert!(USAGE.is_ascii(), "USAGE is not ascii");
        for windows in [true, false] {
            let hint = nothing_arriving_hint(47811, windows);
            assert!(hint.is_ascii(), "hint (windows={windows}) is not ascii:\n{hint}");
        }
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
        }
    }
}
