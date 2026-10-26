//! `earshot-clean` — run one recording through several noise cleaners, so they can be compared.
//!
//! ## Why a file tool and not a switch in the receiver
//!
//! Judging a cleaner live means talking twice, and two takes of a keyboard are not the same noise.
//! A fair comparison needs one recording and several outputs, which is what this writes:
//! `earshot-record` captures the phone through the real Opus path, this cleans that one file every
//! way available, and the ear picks.
//!
//! It also answers the two questions a listening test cannot:
//!
//! - **How much delay does the model add**, which on a live microphone is the cost that is felt.
//! - **How fast does it run**, as a multiple of real time. Under 1× means it cannot keep up.
//!
//! 48 kHz mono only, like the models. Anything else is converted by ffmpeg first, and the error
//! says so.

use std::path::{Path, PathBuf};
use std::time::Instant;

use earshot::clean::{Choice, Cleaner, Strength, REQUIRED_RATE};
use earshot::wav;

const USAGE: &str = "\
earshot-clean — clean one recording every way, to compare them by ear

  earshot-clean <IN.wav> [options]

  -o, --out <DIR>      where the cleaned files go      [default: beside the input]
  -m, --model <NAME>   add one cleaner: dfn3, or a path to a model archive -- the low-latency
                       DeepFilterNet 3 being the one worth trying (repeatable; with none given,
                       the standard comparison runs)
      --atten <DB>     the most the noise may be turned down  [default: 100, meaning all of it]
  -h, --help           this text

The standard comparison is DeepFilterNet 3 with and without its post-filter. A copy of the
untouched audio is written beside them, because a comparison without the original is a comparison
against memory.
";

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

/// One entry in the comparison: what to call it, which model, how hard.
struct Candidate {
    slug: String,
    label: String,
    choice: Choice,
    strength: Strength,
}

fn run() -> Result<(), String> {
    let mut input: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut models: Vec<String> = Vec::new();
    let mut atten = 100.0f32;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                return Ok(());
            }
            "-o" | "--out" => {
                out_dir = Some(PathBuf::from(
                    args.next().ok_or("--out wants a directory")?,
                ));
            }
            "-m" | "--model" => models.push(args.next().ok_or("--model wants a name or a path")?),
            "--atten" => {
                let v = args.next().ok_or("--atten wants a number of dB")?;
                atten = v
                    .parse()
                    .map_err(|_| format!("--atten wants a number, got '{v}'"))?;
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'\n\n{USAGE}"))
            }
            other => input = Some(PathBuf::from(other)),
        }
    }
    let input = input.ok_or_else(|| format!("no recording to clean\n\n{USAGE}"))?;

    let source = wav::read(&input)?;
    if source.rate != REQUIRED_RATE {
        return Err(format!(
            "{} is {} Hz and the models want {REQUIRED_RATE}. Convert it first:\n  \
             ffmpeg -i {} -ar 48000 -ac 1 -c:a pcm_s16le converted.wav",
            input.display(),
            source.rate,
            input.display()
        ));
    }
    let audio = source.mono();
    let seconds = audio.len() as f32 / REQUIRED_RATE as f32;

    let out_dir = out_dir.unwrap_or_else(|| {
        input
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });
    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("cannot make {}: {e}", out_dir.display()))?;

    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "recording".to_string());

    let candidates = if models.is_empty() {
        vec![
            Candidate {
                slug: "dfn3".into(),
                label: "DeepFilterNet 3".into(),
                choice: Choice::Dfn3,
                strength: Strength {
                    atten_lim_db: atten,
                    post_filter: false,
                },
            },
            Candidate {
                slug: "dfn3-postfilter".into(),
                label: "DeepFilterNet 3, post-filter on".into(),
                choice: Choice::Dfn3,
                strength: Strength {
                    atten_lim_db: atten,
                    post_filter: true,
                },
            },
        ]
    } else {
        models
            .iter()
            .map(|name| {
                let choice = Choice::parse(name);
                Candidate {
                    slug: slug_for(name),
                    label: choice.label(),
                    choice,
                    strength: Strength {
                        atten_lim_db: atten,
                        post_filter: false,
                    },
                }
            })
            .collect()
    };

    // The untouched audio, written out in the same shape as the rest so that listening to the four
    // of them is one action and not two.
    let dry_path = out_dir.join(format!("{stem}--off.wav"));
    let mut dry = wav::WavWriter::create(&dry_path, REQUIRED_RATE)?;
    dry.write_f32(&audio)?;
    dry.finish()?;

    println!("{} — {seconds:.1}s at {REQUIRED_RATE} Hz", input.display());
    println!("{:<32} {:>8} {:>9} {:>8}", "cleaner", "delay", "speed", "level");
    println!(
        "{:<32} {:>8} {:>9} {:>8}",
        "off (untouched)", "0 ms", "—", "0.0 dB"
    );

    let dry_level = rms_db(&audio);
    let mut failures = Vec::new();

    for candidate in &candidates {
        let mut cleaner = match Cleaner::new(&candidate.choice, candidate.strength) {
            Ok(Some(c)) => c,
            Ok(None) => continue, // `off`, already written
            Err(e) => {
                failures.push(format!("{}: {e}", candidate.label));
                continue;
            }
        };

        let mut cleaned = Vec::with_capacity(audio.len());
        let started = Instant::now();
        // Fed in 20 ms frames, the size the receiver would hand it, so the measured speed is the
        // speed it will really run at rather than the speed of one big batch.
        for chunk in audio.chunks(REQUIRED_RATE as usize * 20 / 1000) {
            cleaner.process(chunk, &mut cleaned);
        }
        let took = started.elapsed().as_secs_f32();

        let path = out_dir.join(format!("{stem}--{}.wav", candidate.slug));
        let mut writer = wav::WavWriter::create(&path, REQUIRED_RATE)?;
        writer.write_f32(&cleaned)?;
        writer.finish()?;

        let speed = if took > 0.0 { seconds / took } else { f32::NAN };
        println!(
            "{:<32} {:>8} {:>9} {:>8}",
            candidate.label,
            format!("{:.0} ms", cleaner.delay_ms()),
            format!("{speed:.0}x"),
            format!("{:+.1} dB", rms_db(&cleaned) - dry_level)
        );
    }

    for failure in &failures {
        eprintln!("could not run {failure}");
    }
    println!("\nin {}", out_dir.display());
    println!("Listen to --off first, then the rest. The level column is how much quieter the whole");
    println!("file got, which is noise removed plus any voice taken with it — the ear decides which.");
    Ok(())
}

/// A file-name-safe version of whatever was asked for, so `-m /path/to/DeepFilterNet3_ll.tar.gz`
/// does not try to write a file with slashes in its name.
fn slug_for(name: &str) -> String {
    let base = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    base.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase()
}

/// Loudness in dB, as a single number. Only ever compared against another one of these.
fn rms_db(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return f32::NEG_INFINITY;
    }
    let mean_square = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    10.0 * (mean_square.max(1e-20)).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_usable_file_name() {
        assert_eq!(slug_for("/tmp/DeepFilterNet3_ll_onnx.tar.gz"), "deepfilternet3-ll-onnx-tar");
        assert_eq!(slug_for("dfn2"), "dfn2");
    }

    #[test]
    fn quieter_audio_measures_quieter() {
        let loud: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.01).sin()).collect();
        let quiet: Vec<f32> = loud.iter().map(|s| s * 0.1).collect();
        // A tenth of the amplitude is 20 dB down, which is the arithmetic this relies on.
        assert!((rms_db(&quiet) - rms_db(&loud) + 20.0).abs() < 0.5);
    }
}
