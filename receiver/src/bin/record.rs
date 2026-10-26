//! `earshot-record` — write what the phone sends to a wav file, and play nothing.
//!
//! ## Why this exists
//!
//! Two reasons, and both of them came out of real trouble.
//!
//! 1. **Judging a noise cleaner needs the same noise twice.** Listening to a cleaner live means
//!    comparing two takes of a keyboard, which is comparing nothing. One recording run through
//!    several cleaners is a fair test, and that is what this file captures.
//! 2. **A receiver that cannot play can still prove the stream.** On 2026-10-26 a counting UDP
//!    listener showed 1857 real Opus packets arriving from the phone while the user's own PC was
//!    silent, which is what proved the phone half was correct and his receiver was old. That
//!    listener counted bytes; this one keeps the audio, so the next such question is answered by
//!    listening instead of by arithmetic.
//!
//! It needs no sound card, which is the point: the development machine has none.
//!
//! ## What it is not
//!
//! Not the receiver. There is no jitter buffer and no reordering — a packet that arrives late is
//! written late, and a lost one leaves a hole of the right length rather than a concealed frame.
//! That is deliberate: this is a tap on the wire, and smoothing is the receiver's job.

use std::net::UdpSocket;
use std::time::{Duration, Instant};

use earshot::wav::WavWriter;
use earshot::{opus, proto};

const USAGE: &str = "\
earshot-record — capture the phone's audio to a wav file

  earshot-record <FILE.wav> [options]

  -l, --listen <ADDR:PORT>   where to listen        [default: 0.0.0.0:47811]
  -s, --seconds <N>          stop after N seconds of audio  [default: 30]
  -h, --help                 this text

It answers the phone the way the receiver does, so the phone shows it as a PC and goes live.
Nothing is played: the audio goes to the file at 48 kHz mono, which is what the receiver decodes.
";

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut path: Option<String> = None;
    let mut listen = "0.0.0.0:47811".to_string();
    let mut seconds = 30u64;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "-l" | "--listen" => {
                listen = args.next().ok_or("--listen wants an address")?;
            }
            "-s" | "--seconds" => {
                let v = args.next().ok_or("--seconds wants a number")?;
                seconds = v
                    .parse()
                    .map_err(|_| format!("--seconds wants a number, got '{v}'"))?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'\n\n{USAGE}"));
            }
            other => path = Some(other.to_string()),
        }
    }
    let path = path.ok_or_else(|| format!("no file to write to\n\n{USAGE}"))?;

    let socket = UdpSocket::bind(&listen).map_err(|e| {
        format!("cannot listen on {listen}: {e}\n(is a receiver or an old tray already running?)")
    })?;
    // Short enough that the deadline is honoured while nothing is arriving.
    socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(|e| format!("cannot set a read timeout: {e}"))?;

    let mut wav = WavWriter::create(&path, opus::DECODE_RATE)?;
    let mut decoder = opus::Decoder::new(opus::DECODE_RATE)?;
    let mut pcm: Vec<i16> = Vec::with_capacity(opus::MAX_FRAME_SAMPLES);
    let mut buf = [0u8; 2048];

    println!("listening on {listen} — open Earshot on the phone and point it here");
    println!("writing {path}, stopping after {seconds}s of audio");

    let mut samples_written = 0usize;
    let target = seconds as usize * opus::DECODE_RATE as usize;
    let mut packets = 0u64;
    let mut first_audio: Option<Instant> = None;
    // The sequence of the last audio packet, so a gap can be left at its real length instead of
    // being silently closed up.
    let mut last_seq: Option<u32> = None;

    while samples_written < target {
        let (len, from) = match socket.recv_from(&mut buf) {
            Ok(v) => v,
            Err(_) => {
                // A timeout before anything arrives is ordinary; afterwards it means the phone
                // stopped, and the file is however long it got.
                if first_audio.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
                    println!("nothing for 5s — stopping");
                    break;
                }
                continue;
            }
        };
        let (header, payload) = match proto::Header::parse(&buf[..len]) {
            Ok(v) => v,
            Err(_) => continue,
        };
        packets += 1;

        // Answer exactly as the receiver does, so the phone lists this machine and goes live.
        // The buffered figure is honest: nothing is buffered here, because nothing is played -- and
        // so is the cleaning answer, which is "cannot, and is not": this tool keeps the audio
        // exactly as it arrived, which is the whole point of recording with it.
        let mut reply = [0u8; proto::HEADER_LEN];
        proto::hello(&header, 0.0, proto::Cleaning::default()).write(&mut reply);
        let _ = socket.send_to(&reply, from);

        let frame_samples = opus::DECODE_RATE as usize * proto::FRAME_MS as usize / 1000;
        match header.ptype {
            proto::TYPE_OPUS => {
                if first_audio.is_none() {
                    first_audio = Some(Instant::now());
                    println!("audio from {from}");
                }
                // A gap is written as the silence it was, so the recording keeps real time.
                if let Some(previous) = last_seq {
                    let missed = proto::seq_diff(header.sequence, previous) - 1;
                    if missed > 0 && missed < 50 {
                        wav.write_silence(missed as usize * frame_samples)?;
                        samples_written += missed as usize * frame_samples;
                    }
                }
                last_seq = Some(header.sequence);
                pcm.clear();
                match decoder.decode(payload, &mut pcm) {
                    Ok(_) => {
                        wav.write(&pcm)?;
                        samples_written += pcm.len();
                    }
                    Err(e) => eprintln!("undecodable packet: {e}"),
                }
            }
            proto::TYPE_DTX => {
                // The phone saying "silence here". One frame of it.
                last_seq = Some(header.sequence);
                wav.write_silence(frame_samples)?;
                samples_written += frame_samples;
            }
            proto::TYPE_PCM_DEBUG => {
                if first_audio.is_none() {
                    first_audio = Some(Instant::now());
                    println!("raw PCM from {from} — this phone is not compressing");
                }
                let samples: Vec<i16> = payload
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                    .collect();
                // Written at whatever rate it implies rather than resampled: a raw stream is a
                // fallback and a 16 kHz recording is still a recording.
                wav.write(&samples)?;
                samples_written += samples.len();
            }
            _ => {}
        }
    }

    let seconds_written = samples_written as f32 / opus::DECODE_RATE as f32;
    wav.finish()?;
    println!(
        "wrote {path} — {seconds_written:.1}s, {packets} packets from the phone"
    );
    if samples_written == 0 {
        println!("no audio arrived. The phone answered nothing, or it is pointed somewhere else.");
    }
    Ok(())
}
