// Cleaning the voice on this machine, after it arrives.
//
// ## Why there is a second cleaner at all
//
// The phone already has one: RNNoise, which it can run on the plain microphone. It was heard for
// the first time on 2026-10-26 and the verdict was that it is not enough -- "i can partially hear
// the keyboard sounds and the mouse clickings some time to time". That is not a fault in the
// build. RNNoise is a ~2017 model trained on STEADY noise: fans, hum, traffic. A keyboard is a
// transient, which is the one thing it was never good at, and no amount of tuning changes that.
//
// DeepFilterNet is trained for exactly the non-stationary case, and this is where it runs.
//
// ## Why on the PC and not on the phone
//
// Three reasons, in order of how much they matter:
//
//   1. This machine has the CPU. The model costs about a fifth of one core on an old laptop; the
//      same work on a phone is battery, heat, and a device that is usually lying face down.
//   2. The receiver is already Rust, and so is DeepFilterNet's real-time implementation -- it uses
//      `tract`, a pure-Rust ONNX runtime, so this adds no C++ and nothing to the Windows build but
//      more Rust. Getting RNNoise onto the phone cost four separate build faults.
//   3. It can be changed here without anybody reinstalling the phone app.
//
// The honest cost: this cleans AFTER Opus rather than before, so the codec has already had its way
// with the noise. Cleaning at the microphone would be better in principle and much more expensive
// in practice.
//
// ## The shape of the thing
//
// DeepFilterNet works in fixed hops -- 480 samples, 10 ms at 48 kHz. One Earshot frame is 20 ms,
// so it is exactly two hops, but nothing here assumes that: the stream is accumulated and drained
// in whole hops, so a concealed frame or an odd buffer length cannot knock the model out of step.
//
// 48 kHz only, like RNNoise. A stream that arrives at any other rate is passed through untouched
// rather than resampled into the model and back out, which would cost more than the noise does.

use std::path::{Path, PathBuf};

// The package is `deep_filter`; its library is `df`. Both names are right and only one of them
// compiles.
use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};

/// The one rate DeepFilterNet takes.
pub const REQUIRED_RATE: u32 = 48_000;

/// Which cleaner to run, as the command line says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Send the voice through untouched. The default: a receiver that processes audio nobody asked
    /// it to process is a receiver that cannot be trusted to be a wire.
    Off,
    /// DeepFilterNet 3, built in. The only model this program carries.
    Dfn3,
    /// Any other model archive, read from disk.
    ///
    /// The one worth knowing about is the low-latency DeepFilterNet 3: it adds **no** delay at all
    /// where the ordinary model adds 20 ms, and it cleans a little less. It is not built in
    /// because it is a 36 MB model against this one's 8 MB, which is most of a download for a
    /// trade only some people want.
    ///
    /// ⚠️ DeepFilterNet **2** is not offered. The pinned DeepFilterNet revision runs it through a
    /// `tract` that cannot: it aborts inside a reshape it has not implemented (measured
    /// 2026-10-26). A model archive that version cannot run makes this fail at startup rather
    /// than mid-call, which is the best that can be done about it.
    File(PathBuf),
}

impl Choice {
    /// Reads the `--clean` argument. Anything that is not a known name is taken as a path.
    pub fn parse(text: &str) -> Choice {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "no" => Choice::Off,
            "dfn3" | "deepfilternet3" | "3" | "on" | "yes" => Choice::Dfn3,
            _ => Choice::File(PathBuf::from(text)),
        }
    }

    pub fn label(&self) -> String {
        match self {
            Choice::Off => "off".to_string(),
            Choice::Dfn3 => "DeepFilterNet 3".to_string(),
            Choice::File(p) => format!("{}", p.display()),
        }
    }
}

/// How hard to clean, and how much of the residue to chase.
#[derive(Clone, Copy, Debug)]
pub struct Strength {
    /// The most the model may turn the noise down, in dB. 100 means "as much as you like", which
    /// is the default and what somebody who can hear their own keyboard wants. Lower leaves some
    /// of the room in, which sounds more natural on a quiet desk.
    pub atten_lim_db: f32,

    /// The post-filter chases the quiet residue left behind the voice. It sounds cleaner and very
    /// slightly more processed, so it is a choice rather than a default.
    pub post_filter: bool,
}

impl Default for Strength {
    fn default() -> Self {
        Strength {
            atten_lim_db: 100.0,
            post_filter: false,
        }
    }
}

/// A running cleaner, fed whatever arrives and drained in whole hops.
pub struct Cleaner {
    df: DfTract,
    hop: usize,
    /// Samples waiting for a full hop.
    pending: Vec<f32>,
    /// One hop of workspace, so the hot path allocates nothing.
    scratch: Vec<f32>,
}

impl Cleaner {
    /// Builds a cleaner, or says in one sentence why it could not.
    ///
    /// Returns `Ok(None)` for [`Choice::Off`], so the caller has one code path rather than two.
    pub fn new(choice: &Choice, strength: Strength) -> Result<Option<Cleaner>, String> {
        let params = match choice {
            Choice::Off => return Ok(None),
            Choice::Dfn3 => DfParams::default(),
            Choice::File(path) => load_file(path)?,
        };

        let mut runtime = RuntimeParams::default_with_ch(1).with_atten_lim(strength.atten_lim_db);
        if strength.post_filter {
            runtime = runtime.with_post_filter(0.02);
        }

        // `tract` aborts rather than returning an error when a model uses an operator it has not
        // implemented, and a receiver must not die because of the file it was pointed at. The
        // panic is caught here, which is the one place a model is ever built.
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            DfTract::new(params, &runtime)
        }))
        .map_err(|_| {
            "this model cannot run here -- it uses something the bundled inference engine does \
             not implement. DeepFilterNet 2 is one such model."
                .to_string()
        })?;
        let df = built.map_err(|e| format!("the noise model would not start: {e}"))?;
        if df.sr as u32 != REQUIRED_RATE {
            return Err(format!(
                "this model wants {} Hz and Earshot decodes at {REQUIRED_RATE}",
                df.sr
            ));
        }
        let hop = df.hop_size;
        Ok(Some(Cleaner {
            df,
            hop,
            pending: Vec::with_capacity(hop * 4),
            scratch: vec![0.0; hop],
        }))
    }

    /// The delay the model adds, in milliseconds — worth printing, because it is the one cost the
    /// user can feel.
    pub fn delay_ms(&self) -> f32 {
        self.df.lookahead as f32 * self.hop as f32 * 1000.0 / self.df.sr as f32
    }

    /// Changes how hard it cleans, without rebuilding anything.
    pub fn set_strength(&mut self, strength: Strength) {
        self.df.set_atten_lim(strength.atten_lim_db);
        self.df
            .set_pf_beta(if strength.post_filter { 0.02 } else { 0.0 });
    }

    /// Cleans what it can and appends the result to `out`.
    ///
    /// Fewer samples come out than went in until the first hop fills, and from then on it is one
    /// for one. The caller must therefore treat the output length as the truth rather than
    /// assuming it got back what it handed over.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.pending.extend_from_slice(input);
        let mut taken = 0;
        while self.pending.len() - taken >= self.hop {
            let frame = &self.pending[taken..taken + self.hop];
            let noisy = match ArrayView2::from_shape((1, self.hop), frame) {
                Ok(v) => v,
                Err(_) => break,
            };
            let enh = match ArrayViewMut2::from_shape((1, self.hop), &mut self.scratch) {
                Ok(v) => v,
                Err(_) => break,
            };
            // A model that fails on one hop is not a reason to drop the voice: the untouched hop
            // goes out instead, which is noisier and still a voice.
            if self.df.process(noisy, enh).is_err() {
                out.extend_from_slice(frame);
            } else {
                out.extend_from_slice(&self.scratch);
            }
            taken += self.hop;
        }
        if taken > 0 {
            self.pending.drain(..taken);
        }
    }
}

fn load_file(path: &Path) -> Result<DfParams, String> {
    if !path.exists() {
        return Err(format!("there is no model file at {}", path.display()));
    }
    DfParams::new(path.to_path_buf())
        .map_err(|e| format!("{} is not a model archive: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_are_read_and_anything_else_is_a_path() {
        assert_eq!(Choice::parse("off"), Choice::Off);
        assert_eq!(Choice::parse("DFN3"), Choice::Dfn3);
        assert_eq!(Choice::parse(" on "), Choice::Dfn3);
        // DeepFilterNet 2 is not a name here, so it is read as a path and fails as one.
        assert_eq!(Choice::parse("dfn2"), Choice::File(PathBuf::from("dfn2")));
        assert_eq!(
            Choice::parse("/tmp/ll.tar.gz"),
            Choice::File(PathBuf::from("/tmp/ll.tar.gz"))
        );
    }

    #[test]
    fn off_builds_nothing() {
        assert!(Cleaner::new(&Choice::Off, Strength::default())
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_missing_model_file_says_so_rather_than_panicking() {
        // `Cleaner` holds a model and is not `Debug`, so the error is taken by hand rather than
        // with `unwrap_err`.
        let err = match Cleaner::new(
            &Choice::File(PathBuf::from("/nonexistent/model.tar.gz")),
            Strength::default(),
        ) {
            Err(e) => e,
            Ok(_) => panic!("a missing model file should not build a cleaner"),
        };
        assert!(err.contains("no model file"), "{err}");
    }

    /// The real one: a second of noise through the model comes back a second long, in whole hops,
    /// and quieter than it went in.
    #[test]
    fn a_stream_comes_back_the_length_it_went_in() {
        let mut cleaner = Cleaner::new(&Choice::Dfn3, Strength::default())
            .expect("DeepFilterNet 3 should build")
            .expect("not off");
        let hop = cleaner.hop;
        let noise: Vec<f32> = (0..hop * 100)
            .map(|i| ((i * 7919) % 2000) as f32 / 1000.0 - 1.0)
            .collect();
        let mut out = Vec::new();
        for chunk in noise.chunks(960) {
            cleaner.process(chunk, &mut out);
        }
        assert_eq!(out.len() % hop, 0, "output is not whole hops");
        assert!(
            out.len() >= noise.len() - hop,
            "got {} samples back from {}",
            out.len(),
            noise.len()
        );
        let before: f32 = noise.iter().map(|s| s * s).sum::<f32>() / noise.len() as f32;
        let after: f32 = out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32;
        assert!(
            after < before,
            "noise came back louder: {before} -> {after}"
        );
    }
}
