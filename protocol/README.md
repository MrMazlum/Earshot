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

The phone sends; the PC listens on port **47811** by default. There is no handshake and no
registration: the first datagram to arrive is the stream.

The PC does send one thing back — a `HELLO`, about once a second, to whoever is currently sending.
It carries no audio and no part of the audio path depends on it. It exists because without it the
phone cannot tell "my packets are arriving" from "my packets are going nowhere": both look like a
successful `send()`. See [The reply](#the-reply).

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
| 4 | `HELLO` | empty. **The only packet that travels PC → phone.** See [The reply](#the-reply) |

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

## The reply

A `HELLO` is 16 bytes: the header alone, no payload. It is the only packet that ever travels from
the PC to the phone, and its three number fields are not the receiver's own — they are borrowed to
answer the phone with:

| Field | In a `HELLO` |
|---|---|
| sequence | the `sequence` of the packet being answered, echoed back |
| timestamp | how much audio the receiver is holding, in **tenths of a millisecond** (so `605` is 60.5 ms). Not a sample count — this is the one packet where that field means something else |
| ssrc | the phone's own `ssrc`, echoed back |

### Why it exists

A phone sending UDP learns nothing. `send()` succeeds whether the datagram reaches the PC, is
swallowed by a firewall, or leaves down a cellular interface that has no route to `192.168.x.x` at
all. That last one is not hypothetical: it is the failure that prompted this section, and from
inside the app it looked exactly like a working session.

So the phone shows "connected" only while `HELLO`s are arriving, and says so plainly when they stop.

### Rules

- The receiver replies **only to the address it is currently accepting audio from**, and only to a
  datagram that already parsed and passed the sender check. Never to a malformed packet, never to a
  rejected one
- **At most one `HELLO` per second**, plus one immediately when a new sender is accepted, so the
  phone's badge turns green at once rather than up to a second later
- The reply goes to the datagram's source address and port, and nowhere else. No address is
  remembered beyond the current peer
- The phone **ignores a `HELLO` whose `ssrc` is not its own**. A reply meant for another session on
  the same LAN is not evidence about this one
- Nothing in the audio path may wait on a `HELLO`. A receiver that never sends one still works; a
  phone that never sees one still streams

This is deliberately not a heartbeat protocol with state on both ends. It is one packet a second in
the quiet direction, and either side may ignore it entirely.

### Reflection

A `HELLO` is 16 bytes and every packet that earns one is at least 16 bytes, so this cannot amplify:
the reply is never larger than what provoked it, and it is capped at one per second regardless. An
attacker who spoofs a victim's address at an idle receiver gets one 16-byte datagram per second
sent to that victim, which is not a capability worth having.

### Old halves

`HELLO` is an addition inside version **1**, not a new version, and both directions of mismatch are
already handled:

| | |
|---|---|
| New phone, old PC | no reply ever comes. The phone says *sending, but the PC is not answering* and names the likely cause. Audio still works if the PC is in fact receiving |
| Old phone, new PC | the replies arrive at a socket the old app never reads, and the operating system discards them. Nothing changes |

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
