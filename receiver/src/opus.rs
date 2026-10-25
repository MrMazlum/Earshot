//! Opus, the compressed audio format the phone sends.
//!
//! Raw PCM is about 770 kbps; the same voice through Opus is about 32–40. That is the difference
//! between a mic that holds the Wi-Fi channel open continuously and one that barely touches it,
//! which matters on a busy home network far more than raw bandwidth figures suggest.
//!
//! A thin safe layer over libopus, which is built from source and linked statically by
//! `opusic-sys` — so the receiver is still a single file with nothing for the user to install.
//! Only what the receiver actually needs is wrapped: decoding, concealment, and (for tests and
//! the test sender) encoding.
//!
//! ## Two rules that come from libopus, not from us
//!
//! 1. **The decoder is stateful and must be fed in stream order.** Decoding a packet that arrived
//!    early, out of order, poisons the frames after it. This is why decoding happens where the
//!    reorder buffer *releases* frames, never where packets arrive.
//! 2. **A lost frame is told to the decoder, not hidden from it.** Handing it no data at all is
//!    what lets it invent a plausible frame from what it has just heard; dropping in silence
//!    instead is audible as a click. That is [`Decoder::conceal`].

use std::ptr;

use opusic_sys as sys;

/// Mono. Earshot sends one microphone, and a second channel would double the bandwidth it exists
/// to save.
const CHANNELS: i32 = 1;

/// Opus works internally at 48 kHz, so decoding at 48 kHz is the one rate that never resamples
/// inside libopus. The phone's capture rate does not have to match: that is the codec's problem,
/// and it solves it better than our linear resampler would.
pub const DECODE_RATE: u32 = 48_000;

/// The longest frame Opus can carry is 120 ms, which at 48 kHz is 5760 samples. Buffers are sized
/// for that rather than for the 20 ms we expect, so a stray long frame is decoded instead of
/// truncated.
pub const MAX_FRAME_SAMPLES: usize = 5760;

/// How many samples a packet will decode to, or `None` if it is not a packet libopus recognises.
///
/// Cheap: this reads the table of contents byte, it does not decode anything.
pub fn packet_samples(packet: &[u8], rate: u32) -> Option<usize> {
    if packet.is_empty() {
        return None;
    }
    let n = unsafe {
        sys::opus_packet_get_nb_samples(packet.as_ptr(), packet.len() as i32, rate as i32)
    };
    if n > 0 {
        Some(n as usize)
    } else {
        None
    }
}

/// An Opus decoder for one stream.
pub struct Decoder {
    raw: *mut sys::OpusDecoder,
    rate: u32,
}

// The pointer is owned exclusively by this struct and libopus keeps no global state, so moving a
// decoder between threads is sound. It is not `Sync`: two threads must not decode at once.
unsafe impl Send for Decoder {}

impl Decoder {
    pub fn new(rate: u32) -> Result<Self, String> {
        let mut error = 0;
        let raw = unsafe { sys::opus_decoder_create(rate as i32, CHANNELS, &mut error) };
        if raw.is_null() || error != sys::OPUS_OK {
            return Err(format!("could not start the Opus decoder ({error})"));
        }
        Ok(Self { raw, rate })
    }

    /// Decode one packet, appending samples to `out`.
    ///
    /// An undecodable packet is an error rather than a panic: it is a packet from the network, so
    /// a hostile or broken sender must not be able to take the receiver down with one datagram.
    pub fn decode(&mut self, packet: &[u8], out: &mut Vec<i16>) -> Result<usize, String> {
        self.run(Some(packet), MAX_FRAME_SAMPLES, false, out)
    }

    /// Invent a frame to cover one that never arrived — libopus's packet-loss concealment.
    ///
    /// `samples` is how long the missing frame was. The caller knows it from the frames around it;
    /// libopus cannot know it, because the packet that would have said so is the one that is gone.
    pub fn conceal(&mut self, samples: usize, out: &mut Vec<i16>) -> Result<usize, String> {
        self.run(None, samples, false, out)
    }

    /// Recover a lost frame from the *next* packet, when the sender put a redundant copy in it.
    ///
    /// This is Opus's in-band forward error correction: at the cost of a few kbps, each packet can
    /// carry a coarse copy of the one before it. Call this with the packet that arrived *after*
    /// the gap, before decoding that packet normally. Returns `Ok(0)` when there was no redundant
    /// copy to find, which is not an error — it is the ordinary case on a clean network.
    pub fn decode_fec(
        &mut self,
        next_packet: &[u8],
        samples: usize,
        out: &mut Vec<i16>,
    ) -> Result<usize, String> {
        self.run(Some(next_packet), samples, true, out)
    }

    fn run(
        &mut self,
        packet: Option<&[u8]>,
        samples: usize,
        fec: bool,
        out: &mut Vec<i16>,
    ) -> Result<usize, String> {
        let samples = samples.min(MAX_FRAME_SAMPLES);
        let base = out.len();
        out.resize(base + samples, 0);
        let (data, len) = match packet {
            Some(p) => (p.as_ptr(), p.len() as i32),
            None => (ptr::null(), 0),
        };
        let decoded = unsafe {
            sys::opus_decode(
                self.raw,
                data,
                len,
                out[base..].as_mut_ptr(),
                samples as i32,
                i32::from(fec),
            )
        };
        if decoded < 0 {
            out.truncate(base);
            return Err(format!("Opus refused a packet ({decoded})"));
        }
        out.truncate(base + decoded as usize);
        Ok(decoded as usize)
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe { sys::opus_decoder_destroy(self.raw) };
    }
}

/// An Opus encoder.
///
/// The receiver never encodes in normal use — the phone does. This exists so the test sender can
/// produce a real Opus stream, and so the tests can prove the decode path against packets that
/// libopus itself made rather than against bytes we invented.
pub struct Encoder {
    raw: *mut sys::OpusEncoder,
}

unsafe impl Send for Encoder {}

impl Encoder {
    /// `bitrate` in bits per second. 32000 is comfortable for a voice; 64000 is generous.
    pub fn new(rate: u32, bitrate: i32) -> Result<Self, String> {
        let mut error = 0;
        let raw = unsafe {
            sys::opus_encoder_create(
                rate as i32,
                CHANNELS,
                sys::OPUS_APPLICATION_AUDIO,
                &mut error,
            )
        };
        if raw.is_null() || error != sys::OPUS_OK {
            return Err(format!("could not start the Opus encoder ({error})"));
        }
        let encoder = Self { raw };
        encoder.ctl(sys::OPUS_SET_BITRATE_REQUEST, bitrate)?;
        // Redundant copies of the previous frame, which is what `decode_fec` reads. Worth a few
        // kbps on a shared channel.
        encoder.ctl(sys::OPUS_SET_INBAND_FEC_REQUEST, 1)?;
        encoder.ctl(sys::OPUS_SET_PACKET_LOSS_PERC_REQUEST, 5)?;
        Ok(encoder)
    }

    fn ctl(&self, request: i32, value: i32) -> Result<(), String> {
        let code = unsafe { sys::opus_encoder_ctl(self.raw, request, value) };
        if code != sys::OPUS_OK {
            return Err(format!("Opus encoder refused setting {request} ({code})"));
        }
        Ok(())
    }

    /// Encode exactly one frame. `frame` must be a length Opus accepts: 2.5, 5, 10, 20, 40 or
    /// 60 ms at the encoder's rate.
    pub fn encode(&mut self, frame: &[i16], out: &mut Vec<u8>) -> Result<usize, String> {
        // 1275 bytes is the largest a single Opus frame can be.
        let base = out.len();
        out.resize(base + 1275, 0);
        let written = unsafe {
            sys::opus_encode(
                self.raw,
                frame.as_ptr(),
                frame.len() as i32,
                out[base..].as_mut_ptr(),
                1275,
            )
        };
        if written < 0 {
            out.truncate(base);
            return Err(format!("Opus refused a frame ({written})"));
        }
        out.truncate(base + written as usize);
        Ok(written as usize)
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe { sys::opus_encoder_destroy(self.raw) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 440 Hz tone, the signal every test here uses: loud enough to survive compression and
    /// simple enough that a mangled decode is obvious in the numbers.
    fn tone(samples: usize, rate: u32) -> Vec<i16> {
        (0..samples)
            .map(|i| {
                let t = i as f32 / rate as f32;
                ((t * 440.0 * std::f32::consts::TAU).sin() * 12000.0) as i16
            })
            .collect()
    }

    fn rms(samples: &[i16]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = samples.iter().map(|&s| (s as f64).powi(2)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    #[test]
    fn a_frame_survives_the_round_trip() {
        let frame = tone(960, DECODE_RATE);
        let mut encoder = Encoder::new(DECODE_RATE, 32_000).unwrap();
        let mut packet = Vec::new();
        encoder.encode(&frame, &mut packet).unwrap();

        // The point of the exercise: 1920 bytes of PCM became a fraction of that.
        assert!(
            packet.len() < 200,
            "20 ms should compress to well under 200 bytes, got {}",
            packet.len()
        );

        let mut decoder = Decoder::new(DECODE_RATE).unwrap();
        let mut out = Vec::new();
        let decoded = decoder.decode(&packet, &mut out).unwrap();
        assert_eq!(decoded, 960, "20 ms at 48 kHz is 960 samples");

        // Opus is lossy, so this cannot compare sample for sample. A tone that came back with
        // roughly the energy it went in with is what proves the path is real; the first frame is
        // allowed to be quiet because the codec has no history yet.
        assert!(
            rms(&out) > 0.3 * rms(&frame),
            "the tone came back far too quiet"
        );
    }

    /// The header says a payload length alone gives the rate for raw PCM. For Opus it does not —
    /// the packet says so itself, and that is what the engine reads.
    #[test]
    fn a_packet_declares_its_own_length() {
        let frame = tone(960, DECODE_RATE);
        let mut encoder = Encoder::new(DECODE_RATE, 32_000).unwrap();
        let mut packet = Vec::new();
        encoder.encode(&frame, &mut packet).unwrap();
        assert_eq!(packet_samples(&packet, DECODE_RATE), Some(960));
    }

    #[test]
    fn rubbish_is_refused_rather_than_fatal() {
        let mut decoder = Decoder::new(DECODE_RATE).unwrap();
        let mut out = Vec::new();
        assert!(decoder.decode(&[0xff, 0xff, 0xff, 0xff], &mut out).is_err());
        assert!(
            out.is_empty(),
            "a refused packet must leave no half-decoded audio behind"
        );
        assert_eq!(packet_samples(&[], DECODE_RATE), None);
    }

    /// Concealment has to produce *something* to play, and a frame's worth of it.
    #[test]
    fn concealment_fills_the_gap_it_is_asked_to_fill() {
        let frames: Vec<Vec<i16>> = (0..4).map(|_| tone(960, DECODE_RATE)).collect();
        let mut encoder = Encoder::new(DECODE_RATE, 32_000).unwrap();
        let mut decoder = Decoder::new(DECODE_RATE).unwrap();
        let mut out = Vec::new();

        for frame in &frames {
            let mut packet = Vec::new();
            encoder.encode(frame, &mut packet).unwrap();
            out.clear();
            decoder.decode(&packet, &mut out).unwrap();
        }

        let mut concealed = Vec::new();
        let n = decoder.conceal(960, &mut concealed).unwrap();
        assert_eq!(n, 960);
        assert_eq!(concealed.len(), 960);
        // After four frames of a steady tone, the invented frame should carry the tone on rather
        // than fall silent. That is the whole reason concealment beats a frame of zeros.
        assert!(rms(&concealed) > 100.0, "concealment produced near-silence");
    }

    /// Forward error correction: the packet *after* a gap can rebuild the one that was lost.
    #[test]
    fn a_lost_frame_can_be_rebuilt_from_the_next_packet() {
        let mut encoder = Encoder::new(DECODE_RATE, 32_000).unwrap();
        let mut decoder = Decoder::new(DECODE_RATE).unwrap();
        let mut out = Vec::new();

        // Warm both sides up, then lose one frame and recover it from the following packet.
        let mut packets = Vec::new();
        for _ in 0..4 {
            let mut packet = Vec::new();
            encoder
                .encode(&tone(960, DECODE_RATE), &mut packet)
                .unwrap();
            packets.push(packet);
        }
        for packet in &packets[..3] {
            out.clear();
            decoder.decode(packet, &mut out).unwrap();
        }

        let mut recovered = Vec::new();
        let n = decoder
            .decode_fec(&packets[3], 960, &mut recovered)
            .unwrap();
        // Whether a redundant copy is actually present depends on the encoder's own judgement, so
        // this asserts the call is well-formed and returns a frame's worth or nothing at all —
        // not that FEC fired on this particular tone.
        assert!(n == 0 || n == 960, "unexpected recovered length {n}");
        assert_eq!(recovered.len(), n);
    }
}
