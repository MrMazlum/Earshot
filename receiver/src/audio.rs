//! Sound-card output via cpal (PipeWire/ALSA on Linux, WASAPI on Windows, CoreAudio on macOS).
//!
//! P1 plays into whatever the default output device is, so the owner can hear his own voice
//! (`~/EarshotBrain/MASTER_ROADMAP.md`, P1 exit gate). Routing into a *virtual microphone* so
//! Discord sees it is P3 — `~/EarshotBrain/07-PC-Integration.md`.
//!
//! Everything in the callback obeys `~/EarshotBrain/Rules/no-blocking-audio-thread.md`: no locks,
//! no allocation, no logging. It reads a lock-free ring and nothing else.

use crate::ring::SpscRing;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Largest block we will ever be asked for in one callback. Sized once, up front, because the
/// callback may not allocate.
const MAX_BLOCK_SAMPLES: usize = 16_384;

#[derive(Default)]
pub struct OutputStats {
    /// Times the ring ran dry mid-callback. Each one is an audible gap.
    pub underruns: AtomicU64,
    pub samples_played: AtomicU64,
    /// False while we are filling the buffer before starting to play.
    pub primed: AtomicBool,
}

pub struct Output {
    _stream: cpal::Stream,
    pub ring: Arc<SpscRing>,
    pub stats: Arc<OutputStats>,
    pub sample_rate: u32,
    pub channels: u16,
    pub device_name: String,
    /// How full the ring must get before playback starts. This *is* the jitter-buffer latency.
    pub prime_samples: usize,
}

impl Output {
    /// Milliseconds of audio currently sitting in the ring — the buffering part of the latency
    /// budget, live. See `~/EarshotBrain/06-Latency-Budget.md`.
    pub fn buffered_ms(&self) -> f32 {
        self.ring.len() as f32 * 1000.0 / self.sample_rate as f32
    }
}

/// The output devices, by name. Used to hunt for a virtual cable on Windows, where we cannot
/// create one and have to find somebody else's.
pub fn output_device_names() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    Ok(host
        .output_devices()
        .map_err(|e| format!("cannot list output devices: {e}"))?
        .filter_map(|d| d.name().ok())
        .collect())
}

pub fn list_devices() -> Result<(), cpal::DevicesError> {
    let host = cpal::default_host();
    let default = host
        .default_output_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();
    println!("Output devices ({}):", host.id().name());
    for device in host.output_devices()? {
        let name = device.name().unwrap_or_else(|_| "<unnamed>".into());
        let mark = if name == default { " (default)" } else { "" };
        let rate = device
            .default_output_config()
            .map(|c| format!("{} Hz, {} ch", c.sample_rate().0, c.channels()))
            .unwrap_or_else(|_| "unavailable".into());
        println!("  {name}{mark}  —  {rate}");
    }
    Ok(())
}

/// Opens the output device and starts the stream. `buffer_ms` is the pre-roll: bigger is safer on
/// a congested Wi-Fi, and costs exactly that many milliseconds of latency.
pub fn open(device_name: Option<&str>, buffer_ms: u32) -> Result<Output, String> {
    let host = cpal::default_host();
    let device = match device_name {
        Some(want) => host
            .output_devices()
            .map_err(|e| format!("cannot list output devices: {e}"))?
            .find(|d| {
                d.name()
                    .map(|n| n.to_lowercase().contains(&want.to_lowercase()))
                    .unwrap_or(false)
            })
            .ok_or_else(|| format!("no output device matching '{want}' (try --list-devices)"))?,
        None => host
            .default_output_device()
            .ok_or_else(|| "no default output device".to_string())?,
    };

    let name = device.name().unwrap_or_else(|_| "<unnamed>".into());
    let supported = device
        .default_output_config()
        .map_err(|e| format!("no usable output config on '{name}': {e}"))?;

    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let prime_samples = (sample_rate as usize * buffer_ms as usize) / 1000;
    // Room for the pre-roll plus a healthy burst, so a late clump of packets is absorbed rather
    // than dropped. Capacity is rounded to a power of two inside the ring.
    let ring = Arc::new(SpscRing::new((prime_samples * 6).max(sample_rate as usize)));
    let stats = Arc::new(OutputStats::default());

    let err_fn = |e| eprintln!("audio stream error: {e}");

    let stream = match format {
        cpal::SampleFormat::F32 => {
            let (ring_cb, stats_cb) = (ring.clone(), stats.clone());
            let mut scratch = vec![0.0f32; MAX_BLOCK_SAMPLES];
            device.build_output_stream(
                &config,
                move |data: &mut [f32], _| {
                    fill(data, channels, &ring_cb, &stats_cb, prime_samples, &mut scratch, |s| s)
                },
                err_fn,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let (ring_cb, stats_cb) = (ring.clone(), stats.clone());
            let mut scratch = vec![0.0f32; MAX_BLOCK_SAMPLES];
            device.build_output_stream(
                &config,
                move |data: &mut [i16], _| {
                    fill(data, channels, &ring_cb, &stats_cb, prime_samples, &mut scratch, |s| {
                        (s.clamp(-1.0, 1.0) * 32767.0) as i16
                    })
                },
                err_fn,
                None,
            )
        }
        other => return Err(format!("unsupported sample format {other:?} on '{name}'")),
    }
    .map_err(|e| format!("cannot open '{name}': {e}"))?;

    stream
        .play()
        .map_err(|e| format!("cannot start '{name}': {e}"))?;

    Ok(Output {
        _stream: stream,
        ring,
        stats,
        sample_rate,
        channels,
        device_name: name,
        prime_samples,
    })
}

/// The callback body, shared by every sample format.
///
/// Mono in, N channels out: the same sample goes to every channel.
fn fill<T: Copy>(
    data: &mut [T],
    channels: u16,
    ring: &SpscRing,
    stats: &OutputStats,
    prime_samples: usize,
    scratch: &mut [f32],
    convert: impl Fn(f32) -> T,
) {
    let silence = convert(0.0);
    let channels = channels.max(1) as usize;
    let frames = (data.len() / channels).min(scratch.len());

    // Hold playback until the buffer has filled, so the first seconds are not a stutter. The same
    // check re-arms after an underrun.
    if !stats.primed.load(Ordering::Acquire) {
        if ring.len() < prime_samples {
            data.fill(silence);
            return;
        }
        stats.primed.store(true, Ordering::Release);
    }

    let got = ring.pop(&mut scratch[..frames]);
    if got < frames {
        // Ran dry. Play what we have, then go back to filling.
        stats.underruns.fetch_add(1, Ordering::Relaxed);
        stats.primed.store(false, Ordering::Release);
    }

    for (frame, out) in data.chunks_mut(channels).enumerate() {
        let s = if frame < got {
            convert(scratch[frame])
        } else {
            silence
        };
        out.fill(s);
    }
    stats.samples_played.fetch_add(got as u64, Ordering::Relaxed);
}
