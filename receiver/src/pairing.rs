//! Pairing codes: one nine-digit number instead of "type this IP address and this port number".
//!
//! The receiver prints a code, you type it into the phone, done. No IP address on screen, no
//! separate port box, one field to get wrong instead of two.
//!
//! # What this is, and what it is not
//!
//! A code **is** the address. It is a reversible encoding, not encryption, and the algorithm is
//! right here in a public repository — anyone who wants the address back can have it. That is
//! fine, because the address is a private one that means nothing outside your own network, and
//! anyone already on your network can find every device on it in about a second anyway.
//!
//! What it does buy is real, just modest:
//!
//! - **A screenshot or a stream overlay stops showing your network layout.** People post these.
//! - **Neighbouring machines look unrelated.** `192.168.1.41` and `192.168.1.42` differ in one
//!   digit as addresses; as codes they share nothing, so nobody guesses their way along a subnet.
//! - **Typos are usually caught before a connection is attempted.** Only about one in eight
//!   single-digit slips decodes to anything at all — the rest are rejected outright, which is a
//!   far better error than a silent thirty-second timeout.
//!
//! Do not ever describe it to a user as security. It is an address in a friendlier coat.
//!
//! # How the number is built
//!
//! Three steps, each one reversible:
//!
//! 1. **Index.** Private IPv4 space is small: 192.168/16, 172.16/12 and 10/8 come to 17,891,328
//!    addresses between them. Number them from zero. Multiply by 8 and add `port - 47811` to carry
//!    the port too. The result — the *payload* — is under 143,130,624.
//! 2. **Scatter.** Multiply by 387,420,489 modulo the prime 999,999,937. Bijective, so it throws
//!    nothing away, and it spreads 143 million payloads across the whole nine-digit range.
//! 3. **Diffuse.** Permute the nine digits and rotate each one. Step 2 alone is linear: consecutive
//!    addresses came out a fixed distance apart, which is visible if you ever see two codes side by
//!    side. This kills that.
//!
//! Decoding runs the three backwards and rejects anything that lands outside the payload range —
//! which is 86% of the nine-digit space, and is where the typo-catching comes from.
//!
//! An address outside those three private blocks has no code. That is deliberate: a public address
//! should not be handed round as a friendly-looking number, and the app keeps a manual field for
//! the strange cases.

use std::net::Ipv4Addr;

/// The port the receiver listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 47811;

/// How many ports a code can express: `DEFAULT_PORT` through `DEFAULT_PORT + 7`. Enough to run a
/// few receivers on one machine; anything further afield falls back to typing an address.
const PORT_SLOTS: u32 = 8;

/// Prime, and just under 10^9 so every value still fits in nine digits.
const MODULUS: u64 = 999_999_937;
/// Coprime to `MODULUS`, so multiplying by it permutes the whole range.
const MULTIPLIER: u64 = 387_420_489;
/// `MULTIPLIER * MULTIPLIER_INVERSE ≡ 1 (mod MODULUS)`. Precomputed — there is a test that checks
/// it, so a careless edit to either constant fails the build rather than the product.
const MULTIPLIER_INVERSE: u64 = 115_270_698;

/// Output digit *i* is taken from input digit `DIGIT_ORDER[i]`.
const DIGIT_ORDER: [usize; 9] = [4, 7, 1, 8, 0, 6, 3, 5, 2];
/// …and then advanced by this much, modulo ten.
const DIGIT_SHIFT: [u32; 9] = [3, 1, 4, 1, 5, 9, 2, 6, 5];

/// The private blocks a code can express, in the order they are numbered.
///
/// `(first address, how many, index of the first)`. The offsets are cumulative and must stay that
/// way; a test adds them up.
const BLOCKS: [(Ipv4Addr, u32, u32); 3] = [
    (Ipv4Addr::new(192, 168, 0, 0), 1 << 16, 0),
    (Ipv4Addr::new(172, 16, 0, 0), 1 << 20, 1 << 16),
    (Ipv4Addr::new(10, 0, 0, 0), 1 << 24, (1 << 16) + (1 << 20)),
];

/// Total addresses expressible, and so the size of the address half of a payload.
const ADDRESS_SPACE: u32 = (1 << 16) + (1 << 20) + (1 << 24);
/// Payloads run `0..PAYLOAD_SPACE`; everything above it is an invalid code.
const PAYLOAD_SPACE: u64 = ADDRESS_SPACE as u64 * PORT_SLOTS as u64;

/// A pairing code: nine decimal digits, leading zeros included.
///
/// Kept as a number rather than a string so it cannot be half-parsed. Use [`Code::to_string`] to
/// show it and [`Code::parse`] to read one back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code(u32);

impl Code {
    /// The code for an address and port, or `None` if it cannot be expressed — a public or
    /// link-local address, or a port more than 7 above the default.
    pub fn new(ip: Ipv4Addr, port: u16) -> Option<Code> {
        let address = address_index(ip)?;
        let slot = port.checked_sub(DEFAULT_PORT)? as u32;
        if slot >= PORT_SLOTS {
            return None;
        }
        let payload = address as u64 * PORT_SLOTS as u64 + slot as u64;
        Some(Code(diffuse((payload * MULTIPLIER % MODULUS) as u32)))
    }

    /// The address and port a code stands for, or `None` if it stands for nothing.
    pub fn resolve(self) -> Option<(Ipv4Addr, u16)> {
        let scattered = undiffuse(self.0) as u64;
        // `diffuse` works on all nine-digit strings, so undiffusing can land above the modulus.
        // Nothing legitimate ever does.
        if scattered >= MODULUS {
            return None;
        }
        let payload = scattered * MULTIPLIER_INVERSE % MODULUS;
        if payload >= PAYLOAD_SPACE {
            return None;
        }
        let ip = address_at((payload / PORT_SLOTS as u64) as u32)?;
        let port = DEFAULT_PORT + (payload % PORT_SLOTS as u64) as u16;
        Some((ip, port))
    }

    /// Reads a code the way a person would have typed it: spaces and dashes between the groups are
    /// ignored. There must be exactly nine digits, because dropping one is the mistake worth
    /// catching, and nothing else may be in there at all.
    ///
    /// The separators are an allowlist rather than "strip everything that is not a digit", and a
    /// dot is pointedly not on it: `192.168.1.42` is nine digits too, and quietly reading somebody's
    /// pasted IP address as a completely different machine's code is the worst answer available.
    pub fn parse(text: &str) -> Option<Code> {
        let text = text.trim();
        if !text
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '-' | '_'))
        {
            return None;
        }
        let digits: String = text.chars().filter(char::is_ascii_digit).collect();
        if digits.len() != 9 {
            return None;
        }
        digits.parse().ok().map(Code)
    }

    /// Grouped in threes, which is how anyone reads a nine-digit number aloud.
    pub fn grouped(self) -> String {
        let d = self.to_string();
        format!("{} {} {}", &d[0..3], &d[3..6], &d[6..9])
    }
}

impl std::fmt::Display for Code {
    /// Always nine characters. The leading zeros are part of the code, not decoration.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:09}", self.0)
    }
}

/// Where an address sits in the numbering, if it is in one of the private blocks at all.
fn address_index(ip: Ipv4Addr) -> Option<u32> {
    let value = u32::from(ip);
    BLOCKS.iter().find_map(|&(base, size, offset)| {
        let base = u32::from(base);
        (value >= base && value - base < size).then(|| offset + (value - base))
    })
}

/// The inverse of [`address_index`].
fn address_at(index: u32) -> Option<Ipv4Addr> {
    BLOCKS
        .iter()
        .rev()
        .find(|&&(_, _, offset)| index >= offset)
        .map(|&(base, _, offset)| Ipv4Addr::from(u32::from(base) + (index - offset)))
}

/// Shuffles the nine decimal digits. Its own inverse is [`undiffuse`].
fn diffuse(value: u32) -> u32 {
    let digits = digits_of(value);
    (0..9).fold(0, |acc, i| {
        acc * 10 + (digits[DIGIT_ORDER[i]] + DIGIT_SHIFT[i]) % 10
    })
}

fn undiffuse(value: u32) -> u32 {
    let shuffled = digits_of(value);
    let mut digits = [0u32; 9];
    for i in 0..9 {
        // +10 first: these are unsigned, and the shift can be larger than the digit.
        digits[DIGIT_ORDER[i]] = (shuffled[i] + 10 - DIGIT_SHIFT[i]) % 10;
    }
    digits.iter().fold(0, |acc, d| acc * 10 + d)
}

/// Nine digits, most significant first, zero-padded.
fn digits_of(value: u32) -> [u32; 9] {
    let mut digits = [0u32; 9];
    let mut rest = value;
    for slot in digits.iter_mut().rev() {
        *slot = rest % 10;
        rest /= 10;
    }
    digits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// If either constant is edited without the other, everything still compiles and every code in
    /// the world decodes to the wrong machine. Catch it here instead.
    #[test]
    fn the_inverse_really_is_the_inverse() {
        assert_eq!(MULTIPLIER * MULTIPLIER_INVERSE % MODULUS, 1);
    }

    #[test]
    fn the_blocks_are_numbered_end_to_end_with_no_gap() {
        let mut expected = 0;
        for (_, size, offset) in BLOCKS {
            assert_eq!(offset, expected);
            expected += size;
        }
        assert_eq!(expected, ADDRESS_SPACE);
    }

    #[test]
    fn a_code_is_always_nine_digits() {
        // 192.168.0.0 is index 0, the value most likely to come out short.
        let code = Code::new(Ipv4Addr::new(192, 168, 0, 0), DEFAULT_PORT).unwrap();
        assert_eq!(code.to_string().len(), 9);
        assert_eq!(code.grouped().len(), 11);
    }

    #[test]
    fn every_private_address_survives_the_round_trip() {
        // Walking all 17.8 million is slow in a debug build; a stride that is coprime to each block
        // size visits every octet combination without repeating.
        for &(base, size, _) in &BLOCKS {
            let base = u32::from(base);
            let mut offset = 0u32;
            while offset < size {
                let ip = Ipv4Addr::from(base + offset);
                for port in DEFAULT_PORT..DEFAULT_PORT + PORT_SLOTS as u16 {
                    let code = Code::new(ip, port).expect("private address, in-range port");
                    assert_eq!(code.resolve(), Some((ip, port)), "{ip}:{port}");
                }
                offset += 9973; // prime, so it does not settle into a pattern
            }
        }
    }

    #[test]
    fn a_code_reads_back_however_it_was_typed() {
        let code = Code::new(Ipv4Addr::new(192, 168, 1, 42), DEFAULT_PORT).unwrap();
        let d = code.to_string();
        for written in [d.clone(), code.grouped(), format!("{}-{}", &d[0..4], &d[4..])] {
            assert_eq!(Code::parse(&written), Some(code), "{written}");
        }
    }

    #[test]
    fn the_wrong_number_of_digits_is_refused() {
        // A dropped or doubled digit is the mistake this catches; length is the only clue.
        assert_eq!(Code::parse("12345678"), None);
        assert_eq!(Code::parse("1234567890"), None);
        assert_eq!(Code::parse(""), None);
        // Nine digits, but an address. Reading it as a code would silently send the phone to some
        // unrelated machine.
        assert_eq!(Code::parse("192.168.1.42"), None);
        // Not a code, whatever the digit count.
        assert_eq!(Code::parse("abc123456"), None);
        assert_eq!(Code::parse("123456789 x"), None);
    }

    #[test]
    fn addresses_that_are_not_ours_to_hand_out_have_no_code() {
        for ip in [
            Ipv4Addr::new(8, 8, 8, 8),           // public
            Ipv4Addr::new(127, 0, 0, 1),         // loopback
            Ipv4Addr::new(169, 254, 3, 4),       // link-local
            Ipv4Addr::new(172, 32, 0, 1),        // just past 172.16/12
            Ipv4Addr::new(100, 64, 0, 1),        // carrier-grade NAT
        ] {
            assert_eq!(Code::new(ip, DEFAULT_PORT), None, "{ip}");
        }
    }

    #[test]
    fn only_the_ports_a_code_can_carry_get_one() {
        let ip = Ipv4Addr::new(10, 1, 2, 3);
        assert!(Code::new(ip, DEFAULT_PORT + PORT_SLOTS as u16 - 1).is_some());
        assert_eq!(Code::new(ip, DEFAULT_PORT + PORT_SLOTS as u16), None);
        assert_eq!(Code::new(ip, DEFAULT_PORT - 1), None);
        assert_eq!(Code::new(ip, 0), None);
    }

    /// The point of the third step. Without it, consecutive addresses sit a constant distance
    /// apart and the pattern is plain to see.
    #[test]
    fn consecutive_addresses_do_not_produce_related_codes() {
        let codes: Vec<u32> = (40..45)
            .map(|last| {
                Code::new(Ipv4Addr::new(192, 168, 1, last), DEFAULT_PORT)
                    .unwrap()
                    .0
            })
            .collect();
        let gaps: Vec<i64> = codes.windows(2).map(|w| w[1] as i64 - w[0] as i64).collect();
        assert!(
            gaps.windows(2).any(|g| g[0] != g[1]),
            "codes advance by a constant: {codes:?}"
        );
    }

    /// The one test that matters across the wire. `app/lib/pairing.dart` reads the same file, so if
    /// either side drifts, one of the two suites goes red instead of the phone quietly dialling a
    /// machine that is not there.
    #[test]
    fn both_implementations_agree_with_the_shared_vectors() {
        let path = "../protocol/pairing-vectors.csv";
        let text = std::fs::read_to_string(path).expect("vectors file, run from the crate root");
        let mut checked = 0;
        for line in text.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
            let mut fields = line.split(',');
            let (code, ip, port) = (
                fields.next().unwrap(),
                fields.next().unwrap().parse::<Ipv4Addr>().unwrap(),
                fields.next().unwrap().parse::<u16>().unwrap(),
            );
            assert_eq!(Code::new(ip, port).unwrap().to_string(), code, "{ip}:{port}");
            assert_eq!(
                Code::parse(code).unwrap().resolve(),
                Some((ip, port)),
                "{code}"
            );
            checked += 1;
        }
        assert!(checked >= 10, "only {checked} vectors — has the file been emptied?");
    }

    /// Not a guarantee, but the reason a mistyped code fails immediately instead of timing out.
    #[test]
    fn most_single_digit_typos_are_rejected_outright() {
        let code = Code::new(Ipv4Addr::new(192, 168, 1, 42), DEFAULT_PORT).unwrap();
        let original = code.to_string();
        let (mut tried, mut accepted) = (0, 0);
        for position in 0..9 {
            for digit in b'0'..=b'9' {
                let mut typo = original.clone().into_bytes();
                if typo[position] == digit {
                    continue;
                }
                typo[position] = digit;
                tried += 1;
                let parsed = Code::parse(std::str::from_utf8(&typo).unwrap()).unwrap();
                if parsed.resolve().is_some() {
                    accepted += 1;
                }
            }
        }
        assert!(
            accepted * 4 < tried,
            "{accepted} of {tried} typos still decoded — the range check is not doing its job"
        );
    }
}
