//! Just enough wav to record a stream and to read it back.
//!
//! Earshot has no business carrying a general audio-file library: it decodes one format at one
//! rate, and the only files it ever touches are its own recordings. So this reads and writes
//! 16-bit PCM and refuses everything else by name, which is more useful than a dependency that
//! would accept a 24-bit float ADPCM file nobody here can produce.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// A 16-bit mono wav file, with the two sizes in its header patched in once the length is known.
///
/// Sizes go in as zero and are corrected by [`WavWriter::finish`], so a run that is cut short
/// still leaves a file most players will open.
pub struct WavWriter {
    file: File,
    data_bytes: u32,
}

impl WavWriter {
    pub fn create(path: impl AsRef<Path>, rate: u32) -> Result<WavWriter, String> {
        let path = path.as_ref();
        let mut file = File::create(path)
            .map_err(|e| format!("cannot write to {}: {e}", path.display()))?;
        let mut header = Vec::with_capacity(44);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&0u32.to_le_bytes());
        header.extend_from_slice(b"WAVEfmt ");
        header.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk length
        header.extend_from_slice(&1u16.to_le_bytes()); // PCM
        header.extend_from_slice(&1u16.to_le_bytes()); // mono
        header.extend_from_slice(&rate.to_le_bytes());
        header.extend_from_slice(&(rate * 2).to_le_bytes()); // bytes per second
        header.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
        header.extend_from_slice(&16u16.to_le_bytes()); // bits
        header.extend_from_slice(b"data");
        header.extend_from_slice(&0u32.to_le_bytes());
        file.write_all(&header)
            .map_err(|e| format!("cannot write the wav header: {e}"))?;
        Ok(WavWriter {
            file,
            data_bytes: 0,
        })
    }

    pub fn write(&mut self, samples: &[i16]) -> Result<(), String> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.file
            .write_all(&bytes)
            .map_err(|e| format!("cannot write audio: {e}"))?;
        self.data_bytes += bytes.len() as u32;
        Ok(())
    }

    /// Writes floats, clamped. A model hands back values that can sit slightly outside ±1 and
    /// wrapping those into the opposite sign is the loudest bug in audio.
    pub fn write_f32(&mut self, samples: &[f32]) -> Result<(), String> {
        let pcm: Vec<i16> = samples
            .iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect();
        self.write(&pcm)
    }

    pub fn write_silence(&mut self, samples: usize) -> Result<(), String> {
        self.write(&vec![0i16; samples])
    }

    pub fn finish(mut self) -> Result<(), String> {
        let patch = |file: &mut File, at: u64, value: u32| -> Result<(), String> {
            file.seek(SeekFrom::Start(at))
                .map_err(|e| format!("cannot seek in the wav: {e}"))?;
            file.write_all(&value.to_le_bytes())
                .map_err(|e| format!("cannot patch the wav header: {e}"))
        };
        patch(&mut self.file, 4, 36 + self.data_bytes)?;
        patch(&mut self.file, 40, self.data_bytes)?;
        self.file
            .flush()
            .map_err(|e| format!("cannot flush the wav: {e}"))
    }
}

/// What was read out of a wav file.
pub struct Wav {
    pub rate: u32,
    pub channels: u16,
    /// Interleaved if there is more than one channel, which [`Wav::mono`] undoes.
    pub samples: Vec<f32>,
}

impl Wav {
    /// Averages the channels down to one, because the models take mono and so does Earshot.
    pub fn mono(&self) -> Vec<f32> {
        if self.channels <= 1 {
            return self.samples.clone();
        }
        let n = self.channels as usize;
        self.samples
            .chunks(n)
            .map(|frame| frame.iter().sum::<f32>() / n as f32)
            .collect()
    }

    pub fn seconds(&self) -> f32 {
        let frames = self.samples.len() / self.channels.max(1) as usize;
        frames as f32 / self.rate as f32
    }
}

/// Reads a 16-bit PCM wav file.
///
/// Chunks are walked rather than assumed: a wav written by a phone or by ffmpeg often carries a
/// `LIST` chunk before the audio, and reading from a fixed offset 44 gets that as samples.
pub fn read(path: impl AsRef<Path>) -> Result<Wav, String> {
    let path = path.as_ref();
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{} is not a wav file", path.display()));
    }

    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at = |at: usize| {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    };

    let mut rate = 0u32;
    let mut channels = 1u16;
    let mut bits = 0u16;
    let mut format = 0u16;
    let mut data: Option<(usize, usize)> = None;

    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let length = u32_at(at + 4) as usize;
        let body = at + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            format = u16_at(body);
            channels = u16_at(body + 2).max(1);
            rate = u32_at(body + 4);
            bits = u16_at(body + 14);
        } else if id == b"data" {
            // A length that overruns the file is taken as "to the end": a recording killed
            // mid-write has a header claiming more than it holds, and that file is still worth
            // listening to.
            let end = body.saturating_add(length).min(bytes.len());
            data = Some((body, end));
        }
        // Chunks are padded to an even length.
        at = body + length + (length & 1);
    }

    let (start, end) = data.ok_or_else(|| format!("{} has no audio in it", path.display()))?;
    if format != 1 || bits != 16 {
        return Err(format!(
            "{} is not 16-bit PCM (format {format}, {bits} bits). Convert it first:\n  \
             ffmpeg -i {} -ar 48000 -ac 1 -c:a pcm_s16le out.wav",
            path.display(),
            path.display()
        ));
    }

    let samples = bytes[start..end]
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
        .collect();

    Ok(Wav {
        rate,
        channels,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is written comes back, through a header this module patched itself.
    #[test]
    fn a_written_file_reads_back() {
        let path = std::env::temp_dir().join("earshot-wav-roundtrip.wav");
        let mut writer = WavWriter::create(&path, 48_000).unwrap();
        writer.write(&[0, 1000, -1000, i16::MAX, i16::MIN]).unwrap();
        writer.finish().unwrap();

        let wav = read(&path).unwrap();
        assert_eq!(wav.rate, 48_000);
        assert_eq!(wav.channels, 1);
        assert_eq!(wav.samples.len(), 5);
        assert!(wav.samples[1] > 0.0 && wav.samples[2] < 0.0);
        let _ = std::fs::remove_file(path);
    }

    /// A chunk between the header and the audio must not be read as audio.
    #[test]
    fn a_chunk_before_the_audio_is_skipped() {
        let path = std::env::temp_dir().join("earshot-wav-listchunk.wav");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&96_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        // An odd-length chunk, so the padding byte is exercised too.
        bytes.extend_from_slice(b"LIST");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"abc\0");
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&1000i16.to_le_bytes());
        bytes.extend_from_slice(&2000i16.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let wav = read(&path).unwrap();
        assert_eq!(wav.samples.len(), 2, "the LIST chunk was read as audio");
        assert!(wav.samples[0] > 0.0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_format_nothing_here_can_read_says_so() {
        let path = std::env::temp_dir().join("earshot-wav-float.wav");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes()); // float
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&192_000u32.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&32u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.extend_from_slice(&0f32.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();

        // `Wav` holds the samples and is not `Debug`, so the error is taken by hand rather than
        // with `unwrap_err` -- and a failure here would otherwise print the whole file.
        let err = match read(&path) {
            Err(e) => e,
            Ok(_) => panic!("a float wav should not have been read"),
        };
        assert!(err.contains("16-bit PCM"), "{err}");
        assert!(err.contains("ffmpeg"), "it should say how to fix it: {err}");
        let _ = std::fs::remove_file(path);
    }
}
