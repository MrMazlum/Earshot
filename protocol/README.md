# The Earshot wire protocol

Version **1**. Two implementations must agree byte for byte:

| | |
|---|---|
| `receiver/src/proto.rs` | Rust, the PC side |
| `app/android/app/src/main/kotlin/com/mazlum/earshot/Protocol.kt` | Kotlin, the phone side |

This file is the specification. If the code and this file disagree, that is a bug in one of them.

Changing the format means, in a **single commit**: this file, both implementations, a version bump,
and the test vectors regenerated. See [CONTRIBUTING.md](../CONTRIBUTING.md).

## Transport

Audio travels over **UDP**, one 20 ms frame per datagram, never coalesced and **never
retransmitted**. A packet that arrives after its slot has played is discarded, not played late —
waiting for it would put every later packet further behind for the rest of the call.

The phone sends; the PC listens on port **47811** by default. There is no handshake, no
registration and no reply: the first datagram to arrive is the stream.

## Datagram layout

16-byte header, big-endian (network order), then the payload.

```
 byte  0       1       2       3
      +-------+-------+-------+-------+
   0  | 'E'   | 'S'   |ver|typ| flags |
      +-------+-------+-------+-------+
   4  |         sequence (u32)        |
      +-------------------------------+
   8  |     timestamp (u32, samples)  |
      +-------------------------------+
  12  |           ssrc (u32)          |
      +-------------------------------+
  16  |  payload ...
```

| Field | Bytes | Meaning |
|---|---|---|
| magic | 0–1 | `0x45 0x53` — ASCII `ES`. Anything else is not ours and is dropped |
| version | 2, high nibble | `1`. A different value is refused, never guessed at |
| type | 2, low nibble | payload type, below |
| flags | 3 | bit field, below |
| sequence | 4–7 | increments by one per datagram, **wraps at 2^32** |
| timestamp | 8–11 | sample count at the source rate; advances by one frame per packet |
| ssrc | 12–15 | random per session. A change means the phone restarted, so the receiver resets rather than reading it as an enormous sequence jump |

### Payload types

| Value | Name | Payload |
|---|---|---|
| 0 | `OPUS` | one Opus frame. **Not implemented yet** — the receiver says so and drops it |
| 1 | `DTX` | comfort noise / silence marker. Not implemented yet |
| 2 | `KEEPALIVE` | empty. Counted as traffic, produces no audio |
| 3 | `PCM_DEBUG` | raw s16le mono. What ships today |

### Flags

| Bit | Name | Meaning |
|---|---|---|
| `0x01` | `FEC` | payload carries forward error correction. Not implemented yet |
| `0x02` | `ENC` | payload is encrypted. Not implemented yet |
| `0x04` | `MARK` | first packet after a silence |

### Raw PCM is self-describing

A `PCM_DEBUG` payload is 20 ms of signed 16-bit little-endian mono, so its **length alone gives the
sample rate** and no rate field is needed:

| Payload bytes | Samples | Rate |
|---|---|---|
| 1920 | 960 | 48 000 Hz |
| 640 | 320 | 16 000 Hz |

The receiver accepts any even length that works out to 8 000–48 000 Hz and rejects the rest.
16 kHz turns up on its own: some Android `AudioSource` values only offer the noise-cancelled voice
chain at that rate, so the receiver resamples rather than refusing the stream.

`PCM_DEBUG` is not a release format — it is roughly 770 kbps. Opus replaces it and brings that to
about 32 kbps.

## Rules a receiver must follow

- **Never trust a datagram.** It arrives from anyone on the LAN. Bounds-check every field, and
  treat a short, truncated, oversized or malformed packet as a counter to increment, never as an
  error to crash on
- **Compare sequence numbers with wrapping arithmetic.** A plain `a > b` breaks once every ~2.7
  years of continuous streaming at 50 packets/s, which is the least reproducible bug available.
  `proto::seq_diff` is the correct comparison
- **Hold late packets for a bounded window only**, then declare the gap and conceal it

## Pairing codes

Not part of the datagram: a pairing code is a friendlier way to type the receiver's **address**, and
never travels over the wire. The PC prints nine digits, the user types them into the phone, and the
phone turns them back into an address and a port before it sends anything.

It is a **reversible encoding, not encryption and not authentication.** The algorithm is public and
`receiver/src/pairing.rs` documents it in full. What it buys is that a screenshot or a stream
overlay stops showing your network layout, that neighbouring machines get unrelated-looking codes,
and that about seven in eight mistyped digits are rejected on the spot instead of timing out.

`pairing-vectors.csv` is the shared truth. **Both test suites read this file**, which is what stops
the Rust and Dart implementations drifting apart:

```
code,address,port
335618795,192.168.1.42,47811
```

Every address in it is made up. Touch either implementation and run both suites:

```bash
cd receiver && cargo test --locked
cd app && flutter test
```

## Not in the protocol yet

Discovery, encryption, the PC → phone direction, and any control channel at all. When a control
channel arrives it will be TCP; audio stays on UDP.
