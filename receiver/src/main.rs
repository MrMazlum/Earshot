//! `earshot-receiver` — listens for the phone and plays what it hears.
//!
//! This is the terminal front-end. The receiving itself lives in [`earshot::engine`], which the
//! tray application (`earshot-tray`) drives too — keep behaviour changes in there, not here.
//!
//! Scope: one direction, raw PCM, no discovery, no encryption. `--virtual-mic` makes other
//! applications see the phone as an input device.

use earshot::audio;
use earshot::blackhole;
use earshot::cable;
use earshot::engine::{self, Config, Engine};
use earshot::help;

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

NOISE CLEANING:
        --clean <WHAT>         clean the voice on this machine, after it arrives
                               on    DeepFilterNet 3, built in. Adds 20 ms  [default]
                               off   nothing is touched. A local veto: it beats
                                     anything the phone asks for
                               <FILE>  any other model archive, such as the
                                       low-latency DeepFilterNet 3, which adds
                                       no delay at all and cleans a little less
        --atten <DB>           the most the noise may be turned down [default: 100]
                               lower leaves some of the room in: 20 or 30 sounds
                               more natural on a quiet desk
        --post-filter          chase the quiet residue behind the voice. Cleaner,
                               very slightly more processed

    On by default, because the phone has none: its own cleaner was removed once a
    keyboard was still audible through it. The phone asks for this one with a flag and
    can switch it off mid-session. 48 kHz streams only - a 16 kHz stream, which is what
    the phone's voice-call source often gives, is passed through untouched.

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
    macOS    borrows BlackHole, for the same reason. Earshot plays into it and you pick
             the same device - the two ends share one name, unlike Windows.

TIP:
    earshot-tray puts all of this in the system tray instead, with no terminal at all,
    and can start at login. It works on Linux, Windows and macOS, where on Windows it
    is shipped as Earshot.exe.
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
            "--clean" => {
                args.config.clean = earshot::clean::Choice::parse(&value("--clean")?);
            }
            "--atten" => {
                let v = value("--atten")?;
                args.config.clean_strength.atten_lim_db = v
                    .parse()
                    .map_err(|_| format!("--atten wants a number of dB, got '{v}'"))?;
            }
            "--post-filter" => args.config.clean_strength.post_filter = true,
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

fn print_nothing_arriving_hint(port: u16) {
    println!("{}", help::nothing_arriving_hint(port, help::Os::current()));
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
            // Only Linux has a device of its own to take away. On Windows the cable and on macOS
            // BlackHole belong to their own installers, and saying "none was loaded" would imply
            // Earshot had failed at something.
            Ok(false) if cfg!(target_os = "windows") => println!(
                "Nothing to remove. On Windows the virtual microphone is the cable's, not \
                 Earshot's - uninstall VB-Cable through Windows if you want it gone."
            ),
            Ok(false) if cfg!(target_os = "macos") => println!(
                "Nothing to remove. On macOS the virtual microphone is BlackHole, not \
                 Earshot's - uninstall BlackHole if you want it gone."
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
    // the engine's setup path runs on the receive thread where neither belongs. On success the
    // borrowed device now exists, so the engine's own lookup cannot fail. Each is a no-op off its
    // own platform, so calling both is harmless and keeps this branch from having to know which one
    // matters.
    if config.virtual_mic {
        cable::preflight()?;
        blackhole::preflight()?;
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
        // The address, plainly. The phone finds this machine by itself in the ordinary case — it
        // knocks on every address on its own subnet and whoever answers appears in a list — so
        // this line is for the times that cannot work: a different subnet, a VPN, an odd port.
        println!("  THIS PC           {}   port {}", best.ip, ready.port);
        println!("  The phone app should find it by itself. If it does not, type that in.");
        if addresses.len() > 1 {
            // Ethernet and Wi-Fi both up, say. Which one the phone is on is not ours to know.
            println!();
            println!("  This machine is on more than one network. The others:");
            for other in &addresses[1..] {
                println!("      {other}");
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
        } else if cfg!(target_os = "macos") {
            // BlackHole is one loopback device with one name, so the thing we play into and the
            // thing the user records from are the same - there is no back-to-front pair to warn
            // about, and saying so saves the Windows-shaped confusion.
            println!("  That is BlackHole's name, not Earshot's - macOS will not let a program");
            println!("  invent a microphone. Earshot plays into it and you record from the very");
            println!("  same device, so there is only one name to remember.");
        } else {
            // The name is VB-Audio's, not ours, and "why is it not called Earshot" is the first
            // thing every Windows user asks. Answer it before they have to.
            println!("  That is the cable's name, not Earshot's - Windows will not let a program");
            println!("  invent a microphone. Rename it in Sound settings if you would rather:");
            println!("      control mmsys.cpl,,1   ->   right-click it   ->   Properties");
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
                    (status.recovered.load(Ordering::Relaxed), "recovered"),
                    (status.trimmed.load(Ordering::Relaxed), "trimmed"),
                ]
                .iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, label)| format!("  {label} {n}"))
                .collect::<String>();
                // Said plainly and only while it is true, so the line never claims cleaning that
                // is not happening -- a 16 kHz stream, or a phone that asked for none.
                let cleaning = if status.cleaning.load(Ordering::Relaxed) {
                    "  cleaned"
                } else {
                    ""
                };
                println!(
                    "{:>5} pkt/s  {:>4} kbps  buffered {:>5.1} ms  lost {}  late {}  dup {}  underruns {}{}{}",
                    status.pkt_per_sec.load(Ordering::Relaxed),
                    status.kbps.load(Ordering::Relaxed),
                    status.buffered_ms(),
                    status.lost.load(Ordering::Relaxed),
                    status.late.load(Ordering::Relaxed),
                    status.duplicates.load(Ordering::Relaxed),
                    status.underruns.load(Ordering::Relaxed),
                    extra,
                    cleaning,
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

    /// `--help` is the other thing a confused Windows user reads, and a Turkish code page turns
    /// an em dash in it into mojibake. The troubleshooting text has its own test in `help`.
    #[test]
    fn the_usage_text_is_plain_ascii() {
        assert!(USAGE.is_ascii(), "USAGE is not ascii");
    }
}
