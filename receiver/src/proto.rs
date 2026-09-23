//! The Earshot wire format.
//!
//! MUST stay byte-identical to the Android side (`app/android/.../Protocol.kt`).
//! Specified in `protocol/README.md` — change that first, then both ends, in the same commit,
//! with a version bump.

pub const MAGIC: [u8; 2] = [0x45, 0x53]; // 'E' 'S'
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 16;
pub const FRAME_MS: u32 = 20;

pub const TYPE_OPUS: u8 = 0;
pub const TYPE_DTX: u8 = 1;
pub const TYPE_KEEPALIVE: u8 = 2;
/// Raw s16le mono, and what ships today. Roughly 770 kbps; Opus replaces it.
pub const TYPE_PCM_DEBUG: u8 = 3;
/// The one packet that travels PC -> phone: header only, no payload.
///
/// It is what lets the phone tell "my audio is arriving" from "my audio is going nowhere", which
/// `send()` cannot: a datagram leaving down an interface with no route to the PC succeeds exactly
/// like one that gets there. `protocol/README.md` has the field meanings, which are borrowed for
/// this type and are not the usual ones.
pub const TYPE_HELLO: u8 = 4;

pub const FLAG_FEC: u8 = 0x01;
pub const FLAG_ENC: u8 = 0x02;
pub const FLAG_MARK: u8 = 0x04;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub version: u8,
    pub ptype: u8,
    pub flags: u8,
    pub sequence: u32,
    pub timestamp: u32,
    pub ssrc: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    TooShort,
    BadMagic,
    BadVersion(u8),
}

impl Header {
    pub fn write(&self, out: &mut [u8]) -> usize {
        assert!(out.len() >= HEADER_LEN);
        out[0] = MAGIC[0];
        out[1] = MAGIC[1];
        out[2] = ((self.version & 0x0F) << 4) | (self.ptype & 0x0F);
        out[3] = self.flags;
        out[4..8].copy_from_slice(&self.sequence.to_be_bytes());
        out[8..12].copy_from_slice(&self.timestamp.to_be_bytes());
        out[12..16].copy_from_slice(&self.ssrc.to_be_bytes());
        HEADER_LEN
    }

    /// Parses a datagram. Never panics: this reads packets from anyone on the LAN.
    pub fn parse(buf: &[u8]) -> Result<(Header, &[u8]), ParseError> {
        if buf.len() < HEADER_LEN {
            return Err(ParseError::TooShort);
        }
        if buf[0] != MAGIC[0] || buf[1] != MAGIC[1] {
            return Err(ParseError::BadMagic);
        }
        let version = buf[2] >> 4;
        if version != VERSION {
            return Err(ParseError::BadVersion(version));
        }
        let header = Header {
            version,
            ptype: buf[2] & 0x0F,
            flags: buf[3],
            sequence: u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]),
            timestamp: u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]),
            ssrc: u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]]),
        };
        Ok((header, &buf[HEADER_LEN..]))
    }
}

/// The reply to a datagram from the phone.
///
/// Both numbers it carries are echoes or gauges rather than the fields' usual meanings, and that
/// is why building one lives here next to the parser instead of in the receive loop: the phone
/// reads these three values, so there must be exactly one place that decides what they hold.
///
/// `buffered_ms` goes out in tenths of a millisecond, which keeps a useful figure inside a u32
/// without inventing a float encoding for one packet a second. A NaN or a negative reads as zero:
/// float-to-integer casts saturate in Rust, and a nonsense buffer depth must not become a huge one.
pub fn hello(answering: &Header, buffered_ms: f32) -> Header {
    Header {
        version: VERSION,
        ptype: TYPE_HELLO,
        flags: 0,
        sequence: answering.sequence,
        timestamp: (buffered_ms * 10.0) as u32,
        ssrc: answering.ssrc,
    }
}

/// Wrapping-aware sequence distance: `a - b`, positive when `a` is newer.
///
/// Sequence numbers wrap at 2^32. A naive `a > b` breaks once every ~2.7 years of continuous
/// streaming at 50 packets/s — which is exactly the kind of bug that is impossible to reproduce.
pub fn seq_diff(a: u32, b: u32) -> i64 {
    (a.wrapping_sub(b) as i32) as i64
}

/// Sample rate implied by a raw-PCM payload: 20 ms of s16le mono is self-describing.
/// 1920 bytes → 48 000 Hz, 640 bytes → 16 000 Hz.
pub fn pcm_rate_from_payload(payload_len: usize) -> Option<u32> {
    // `& 1` rather than `% 2`: it says "odd" without tempting clippy into `is_multiple_of`, which
    // needs a much newer rustc than the one Ubuntu ships. See the MSRV note in README.
    if payload_len == 0 || payload_len & 1 != 0 {
        return None;
    }
    let samples = (payload_len / 2) as u32;
    let rate = samples * (1000 / FRAME_MS);
    if (8000..=48000).contains(&rate) {
        Some(rate)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_header() -> Header {
        Header {
            version: VERSION,
            ptype: TYPE_PCM_DEBUG,
            flags: FLAG_MARK,
            sequence: 0x0102_0304,
            timestamp: 0x0000_03C0,
            ssrc: 0xDEAD_BEEF,
        }
    }

    #[test]
    fn header_round_trip() {
        let h = sample_header();
        let mut buf = [0u8; HEADER_LEN + 4];
        h.write(&mut buf);
        buf[HEADER_LEN..].copy_from_slice(&[1, 2, 3, 4]);

        let (parsed, payload) = Header::parse(&buf).expect("parses");
        assert_eq!(parsed, h);
        assert_eq!(payload, &[1, 2, 3, 4]);
    }

    /// The exact bytes both implementations must agree on. If this test fails after an edit,
    /// the Android side is now speaking a different protocol.
    #[test]
    fn header_wire_bytes_are_frozen() {
        let mut buf = [0u8; HEADER_LEN];
        sample_header().write(&mut buf);
        assert_eq!(
            buf,
            [
                0x45, 0x53, // 'E' 'S'
                0x13, // version 1, type 3 (PCM debug)
                0x04, // flags: MARK
                0x01, 0x02, 0x03, 0x04, // sequence
                0x00, 0x00, 0x03, 0xC0, // timestamp = 960
                0xDE, 0xAD, 0xBE, 0xEF, // ssrc
            ]
        );
    }

    /// The phone matches a reply to its own session by the echoed ssrc, and would show a wrong
    /// buffer depth if the tenths were dropped. Both are the whole content of the packet.
    #[test]
    fn a_hello_echoes_the_phone_back_to_itself() {
        let asked = sample_header();
        let reply = hello(&asked, 60.5);

        assert_eq!(reply.ptype, TYPE_HELLO);
        assert_eq!(reply.version, VERSION);
        assert_eq!(reply.flags, 0);
        assert_eq!(reply.sequence, asked.sequence);
        assert_eq!(reply.ssrc, asked.ssrc);
        assert_eq!(reply.timestamp, 605);
    }

    /// A buffer depth arriving as NaN or negative is a bug somewhere upstream; it must not turn
    /// into an enormous number on the phone's screen.
    #[test]
    fn a_nonsense_buffer_depth_reads_as_zero() {
        let asked = sample_header();
        assert_eq!(hello(&asked, f32::NAN).timestamp, 0);
        assert_eq!(hello(&asked, -3.0).timestamp, 0);
        assert_eq!(hello(&asked, f32::INFINITY).timestamp, u32::MAX);
    }

    /// A reply is 16 bytes, and every packet that earns one is at least 16 bytes. That inequality
    /// is what stops the receiver being usable as a reflector, so it is a test and not a comment.
    #[test]
    fn a_hello_is_never_larger_than_what_provoked_it() {
        let mut buf = [0u8; HEADER_LEN];
        let written = hello(&sample_header(), 0.0).write(&mut buf);
        assert_eq!(written, HEADER_LEN);
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(Header::parse(&[]), Err(ParseError::TooShort));
        assert_eq!(Header::parse(&[0u8; 8]), Err(ParseError::TooShort));
        assert_eq!(Header::parse(&[0xFFu8; 32]), Err(ParseError::BadMagic));

        // Right magic, wrong version — must be refused, not misread.
        let mut buf = [0u8; HEADER_LEN];
        buf[0] = MAGIC[0];
        buf[1] = MAGIC[1];
        buf[2] = 0x90; // version 9
        assert_eq!(Header::parse(&buf), Err(ParseError::BadVersion(9)));
    }

    #[test]
    fn sequence_wrap_is_handled() {
        assert_eq!(seq_diff(5, 3), 2);
        assert_eq!(seq_diff(3, 5), -2);
        // Across the wrap: 1 is two packets newer than u32::MAX - 1.
        assert_eq!(seq_diff(1, u32::MAX - 1), 3);
        assert_eq!(seq_diff(u32::MAX - 1, 1), -3);
    }

    #[test]
    fn pcm_rate_is_derived_from_length() {
        assert_eq!(pcm_rate_from_payload(1920), Some(48_000));
        assert_eq!(pcm_rate_from_payload(640), Some(16_000));
        assert_eq!(pcm_rate_from_payload(0), None);
        assert_eq!(pcm_rate_from_payload(1921), None);
        assert_eq!(pcm_rate_from_payload(9999999), None);
    }
}
