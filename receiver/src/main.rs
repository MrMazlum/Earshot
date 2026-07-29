//! `earshot-receiver` — listens for the phone and plays what it hears.
//!
//! This is the terminal front-end. The receiving itself lives in [`earshot::engine`], which the
//! tray application (`earshot-tray`) drives too — keep behaviour changes in there, not here.
//!
//! Scope: one direction, raw PCM, no discovery, no encryption. `--virtual-mic` makes other
//! applications see the phone as an input device (Linux only so far).
//! Roadmap and exit gates: `~/EarshotBrain/MASTER_ROADMAP.md`.

use earshot::audio;
use earshot::engine::{self, Config, Engine};
use earshot::pairing::Code;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Args {
    config: Config,
    list_devices: bool,
    remove_virtual_mic: bool,
}

const USAGE: &str = "\
earshot-receiver — turns your phone into this machine's microphone

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

    Linux    creates the device itself — nothing to install.
    Windows  needs VB-Cable (free, one-off): https://vb-audio.com/Cable/
             Earshot plays into 'CABLE Input'; you pick 'CABLE Output' as your mic.

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

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    if args.remove_virtual_mic {
        let name = &args.config.virtual_mic_name;
        match earshot::virtualmic::remove(name) {
            Ok(true) => println!("Removed the virtual microphone '{name}'."),
            Ok(false) => println!("No virtual microphone called '{name}' was loaded."),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if args.list_devices {
        if let Err(e) = audio::list_devices() {
            eprintln!("cannot list devices: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = run(args.config) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(config: Config) -> Result<(), String> {
    let buffer_ms = config.buffer_ms;
    let listen = config.listen.clone();
    let engine = Engine::start(config)?;
    let ready = engine.ready().clone();

    println!("Earshot receiver");
    match &ready.virtual_mic {
        Some(name) => println!(
            "  output   virtual microphone \"{}\" ({} Hz, {} ch){}",
            name,
            ready.out_rate,
            ready.out_channels,
            if ready.virtual_mic_created {
                " — created just now"
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
        println!("  ! No ordinary LAN address found — that is the default route, and if a VPN");
        println!("    is up the phone will not be able to reach it.");
    } else {
        println!("  listening on {listen} (could not detect this machine's LAN address)");
    }
    if let Some(name) = &ready.virtual_mic {
        println!();
        println!("  In Discord / OBS / Zoom, pick this input:   {name}");
        println!("  Nothing plays out of the speakers in this mode — that is the point.");
        // Only Linux creates the device, so only Linux has one to take away again.
        if cfg!(target_os = "linux") {
            println!("  It survives restarts of this program. Remove it with:");
            println!("      earshot-receiver --remove-virtual-mic");
        }
    }
    println!();
    println!("Waiting for the phone… (Ctrl-C to stop)");

    let status = engine.status();
    let mut last_line = Instant::now();

    while engine.is_running() {
        for notice in status.take_notices() {
            println!("{notice}");
        }
        if last_line.elapsed() >= Duration::from_secs(1) {
            last_line = Instant::now();
            if status.connected.load(Ordering::Relaxed) {
                let ring_drops = status.ring_drops.load(Ordering::Relaxed);
                println!(
                    "{:>5} pkt/s  {:>4} kbps  buffered {:>5.1} ms  lost {}  late {}  dup {}  underruns {}{}",
                    status.pkt_per_sec.load(Ordering::Relaxed),
                    status.kbps.load(Ordering::Relaxed),
                    status.buffered_ms(),
                    status.lost.load(Ordering::Relaxed),
                    status.late.load(Ordering::Relaxed),
                    status.duplicates.load(Ordering::Relaxed),
                    status.underruns.load(Ordering::Relaxed),
                    if ring_drops > 0 {
                        format!("  ring-drops {ring_drops}")
                    } else {
                        String::new()
                    },
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
