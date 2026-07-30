//! `earshot-testsend` — a fake phone.
//!
//! Sends the same packets the Android app sends, so the PC half can be developed and debugged
//! without a handset, and so loss and reordering can be produced *on purpose* instead of waited
//! for. `--loss 5 --jitter 30` is the closest thing to a bad Wi-Fi that a cable can offer.
//!
//! Run the receiver in one terminal and this in another:
//!     earshot-receiver
//!     earshot-testsend --to 127.0.0.1:47811 --loss 5 --jitter 30

use earshot::proto::{self, Header};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

const USAGE: &str = "\
earshot-testsend - pretends to be the phone, so the receiver can be tested alone

USAGE:
    earshot-testsend [OPTIONS]

OPTIONS:
    -t, --to <ADDR:PORT>   receiver address        [default: 127.0.0.1:47811]
    -r, --rate <HZ>        48000 or 16000          [default: 48000]
    -s, --seconds <N>      how long to send, 0 = forever  [default: 10]
        --loss <PERCENT>   drop this share of packets     [default: 0]
        --jitter <MS>      random extra delay, and occasional reordering [default: 0]
        --tone <HZ>        test tone frequency     [default: 440]
    -h, --help             this text
";

/// xorshift64*, so the fake network is reproducible and pulls in no dependency.
struct Rng(u64);
impl Rng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
    /// 0.0 .. 1.0
    fn unit(&mut self) -> f64 {
        self.next_u32() as f64 / u32::MAX as f64
    }
}

struct Args {
    to: String,
    rate: u32,
    seconds: u64,
    loss: f64,
    jitter_ms: u64,
    tone: f64,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        to: "127.0.0.1:47811".into(),
        rate: 48_000,
        seconds: 10,
        loss: 0.0,
        jitter_ms: 0,
        tone: 440.0,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        let num = |v: String, name: &str| -> Result<f64, String> {
            v.parse::<f64>()
                .map_err(|_| format!("{name} wants a number, got '{v}'"))
        };
        match arg.as_str() {
            "-t" | "--to" => a.to = value("--to")?,
            "-r" | "--rate" => a.rate = num(value("--rate")?, "--rate")? as u32,
            "-s" | "--seconds" => a.seconds = num(value("--seconds")?, "--seconds")? as u64,
            "--loss" => a.loss = num(value("--loss")?, "--loss")? / 100.0,
            "--jitter" => a.jitter_ms = num(value("--jitter")?, "--jitter")? as u64,
            "--tone" => a.tone = num(value("--tone")?, "--tone")?,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option '{other}'\n\n{USAGE}")),
        }
    }
    if a.rate != 48_000 && a.rate != 16_000 {
        return Err(format!("--rate must be 48000 or 16000, got {}", a.rate));
    }
    Ok(a)
}

fn main() {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("cannot open a socket: {e}"))?;
    socket
        .connect(&args.to)
        .map_err(|e| format!("cannot target {}: {e}", args.to))?;

    let frame_samples = (args.rate as usize * proto::FRAME_MS as usize) / 1000;
    let frame_interval = Duration::from_millis(proto::FRAME_MS as u64);
    let total_frames = if args.seconds == 0 {
        u64::MAX
    } else {
        args.seconds * 1000 / proto::FRAME_MS as u64
    };

    println!(
        "sending {} Hz tone to {}  ({} Hz mono, {} ms frames, loss {:.0}%, jitter {} ms)",
        args.tone,
        args.to,
        args.rate,
        proto::FRAME_MS,
        args.loss * 100.0,
        args.jitter_ms
    );

    // Seeded from the clock so each run is a distinct session, exactly as a real phone would be.
    // A fixed seed here made two consecutive runs look like one stream that had jumped backwards.
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x1234_5678_9ABC_DEF0)
        | 1;
    let mut rng = Rng(seed);
    let ssrc = 0xEA25_0000 | (rng.next_u32() & 0xFFFF);
    let mut packet = vec![0u8; proto::HEADER_LEN + frame_samples * 2];
    let mut phase = 0.0f64;
    let step = std::f64::consts::TAU * args.tone / args.rate as f64;

    // Packets waiting for their (jittered) departure time. Giving each packet an independent delay
    // is what produces genuine reordering; delaying the *loop* would just send a slower stream,
    // which starves the receiver and looks like a receiver bug.
    let mut pending: Vec<(Instant, Vec<u8>)> = Vec::new();
    let mut sent = 0u64;
    let mut dropped = 0u64;
    let start = Instant::now();
    let mut next_frame_at = start;

    for n in 0..total_frames {
        // A gentle envelope so the tone is obviously "a signal" and not a stuck buzzer.
        let env = 0.25 + 0.2 * (start.elapsed().as_secs_f64() * std::f64::consts::TAU / 3.0).sin();
        for i in 0..frame_samples {
            let v = (phase.sin() * env * i16::MAX as f64) as i16;
            phase += step;
            if phase > std::f64::consts::TAU {
                phase -= std::f64::consts::TAU;
            }
            let off = proto::HEADER_LEN + i * 2;
            packet[off..off + 2].copy_from_slice(&v.to_le_bytes());
        }

        let header = Header {
            version: proto::VERSION,
            ptype: proto::TYPE_PCM_DEBUG,
            flags: 0,
            sequence: n as u32,
            timestamp: (n as u32).wrapping_mul(frame_samples as u32),
            ssrc,
        };
        header.write(&mut packet);

        if args.loss > 0.0 && rng.unit() < args.loss {
            dropped += 1;
        } else {
            let delay = Duration::from_micros((rng.unit() * args.jitter_ms as f64 * 1000.0) as u64);
            pending.push((Instant::now() + delay, packet.clone()));
        }

        // The stream keeps its 20 ms cadence no matter what the jitter does.
        next_frame_at += frame_interval;
        flush_due(&socket, &mut pending, next_frame_at, &mut sent, &args.to)?;
        let now = Instant::now();
        if next_frame_at > now {
            std::thread::sleep(next_frame_at - now);
        }
    }

    flush_due(
        &socket,
        &mut pending,
        Instant::now() + Duration::from_secs(1),
        &mut sent,
        &args.to,
    )?;
    println!("done: {sent} packets sent, {dropped} deliberately dropped");
    Ok(())
}

/// UDP has no connection to refuse, so `ECONNREFUSED` here is really an ICMP port-unreachable
/// coming back: nobody is listening. Say that, rather than repeating the kernel's phrasing.
fn send_error(e: &std::io::Error, to: &str) -> String {
    if e.kind() == std::io::ErrorKind::ConnectionRefused {
        format!(
            "nothing is listening on {to}.\n\
             Start the receiver first, in another terminal:\n\
             \n    cargo run --release --bin earshot-receiver\n"
        )
    } else {
        format!("send failed: {e}")
    }
}

/// Sends every queued packet whose departure time has come, sleeping until each one is due, but
/// never past `until`. Packets leave in due order, so a jittered one can overtake its successor.
fn flush_due(
    socket: &UdpSocket,
    pending: &mut Vec<(Instant, Vec<u8>)>,
    until: Instant,
    sent: &mut u64,
    to: &str,
) -> Result<(), String> {
    while let Some(due) = pending.iter().map(|(d, _)| *d).min() {
        if due > until {
            break;
        }
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        }
        let idx = pending.iter().position(|(d, _)| *d == due).unwrap();
        let (_, packet) = pending.remove(idx);
        socket.send(&packet).map_err(|e| send_error(&e, to))?;
        *sent += 1;
    }
    Ok(())
}
