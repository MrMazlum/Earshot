//! Linear resampling, phone rate → sound-card rate.
//!
//! Needed because the good `AudioSource` values on Android may force 16 kHz
//! (`~/EarshotBrain/Concepts/audio-pipeline.md` §The AudioSource trap) while PC output is almost
//! always 48 kHz.
//!
//! Linear interpolation is not the nicest-sounding resampler, but it is cheap, allocation-free and
//! has no latency of its own. Real clock-drift correction — the slow, continuous kind described in
//! `~/EarshotBrain/Concepts/clock-drift-and-resampling.md` — is P5, not this.

/// Each call works on the virtual array `[prev, input[0] … input[len-1]]`, so index 0 is the last
/// sample of the *previous* block. Positions therefore run over `0 .. len`, and the carried
/// fractional position joins blocks without a click or a lost sample.
pub struct Resampler {
    src_rate: u32,
    dst_rate: u32,
    /// Read position in that virtual array.
    pos: f64,
    /// Last sample of the previous block.
    prev: f32,
}

/// Start one sample in, i.e. at `input[0]`: there is no earlier audio to interpolate from.
const START_POS: f64 = 1.0;

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            src_rate,
            dst_rate,
            pos: START_POS,
            prev: 0.0,
        }
    }

    pub fn rates(&self) -> (u32, u32) {
        (self.src_rate, self.dst_rate)
    }

    pub fn is_passthrough(&self) -> bool {
        self.src_rate == self.dst_rate
    }

    /// Forgets the join state. Call after a gap, so silence is not smeared into the next frame.
    pub fn reset(&mut self) {
        self.pos = START_POS;
        self.prev = 0.0;
    }

    /// Appends the resampled version of `input` to `out`. Never allocates beyond `out` growing.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if self.is_passthrough() {
            out.extend_from_slice(input);
            self.prev = input[input.len() - 1];
            return;
        }

        let step = self.src_rate as f64 / self.dst_rate as f64;
        let len = input.len();
        let mut t = self.pos;

        while t < len as f64 {
            let i = t.floor() as usize; // 0 .. len-1
            let frac = (t - i as f64) as f32;
            let a = if i == 0 { self.prev } else { input[i - 1] };
            let b = input[i];
            out.push(a + (b - a) * frac);
            t += step;
        }

        self.prev = input[len - 1];
        self.pos = t - len as f64;
    }
}

/// s16le bytes → f32 in [-1, 1). Trailing odd byte is ignored rather than panicking: this data
/// came off the network.
pub fn s16le_to_f32(bytes: &[u8], out: &mut Vec<f32>) {
    out.clear();
    for pair in bytes.chunks_exact(2) {
        let v = i16::from_le_bytes([pair[0], pair[1]]);
        out.push(v as f32 / 32768.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_is_exact() {
        let mut r = Resampler::new(48_000, 48_000);
        let mut out = Vec::new();
        r.process(&[0.1, 0.2, 0.3], &mut out);
        assert_eq!(out, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn tripling_16k_to_48k_gives_three_times_the_samples() {
        let mut r = Resampler::new(16_000, 48_000);
        let input = vec![0.5f32; 320]; // 20 ms at 16 kHz

        // 20 ms in, 20 ms out: 960 samples per block, ±1 from where the fractional read position
        // happens to land. `stays_in_sync_across_many_blocks` is what guards the long run.
        for block in 0..10 {
            let mut out = Vec::new();
            r.process(&input, &mut out);
            let n = out.len() as i64;
            assert!((n - 960).abs() <= 3, "block {block} produced {n}, want ~960");
        }
    }

    #[test]
    fn stays_in_sync_across_many_blocks() {
        // Drift here would be an audible, ever-growing delay. 5 seconds of 20 ms blocks.
        let mut r = Resampler::new(16_000, 48_000);
        let input = vec![0.0f32; 320];
        let mut total = 0usize;
        for _ in 0..250 {
            let mut out = Vec::new();
            r.process(&input, &mut out);
            total += out.len();
        }
        let expected = 250 * 960;
        assert!(
            (total as i64 - expected as i64).abs() <= 3,
            "produced {total}, expected ~{expected}"
        );
    }

    #[test]
    fn interpolates_between_samples() {
        let mut r = Resampler::new(1_000, 2_000);
        let mut out = Vec::new();
        r.process(&[0.0, 1.0], &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], 0.0);
        assert!((out[1] - 0.5).abs() < 1e-6, "halfway sample, got {}", out[1]);

        // The next block continues from 1.0 rather than jumping back — no click at the seam.
        out.clear();
        r.process(&[1.0, 0.0], &mut out);
        assert_eq!(out[0], 1.0);
    }

    #[test]
    fn decodes_little_endian_pcm() {
        let mut out = Vec::new();
        // 0x0000 = 0, 0x8000 = -1.0, 0x7FFF ≈ +1.0
        s16le_to_f32(&[0x00, 0x00, 0x00, 0x80, 0xFF, 0x7F], &mut out);
        assert_eq!(out[0], 0.0);
        assert_eq!(out[1], -1.0);
        assert!((out[2] - 1.0).abs() < 0.001);
    }

    #[test]
    fn ignores_a_truncated_trailing_sample() {
        let mut out = Vec::new();
        s16le_to_f32(&[0x00, 0x00, 0x7F], &mut out); // 3 bytes: 1 sample + junk
        assert_eq!(out.len(), 1);
    }
}
