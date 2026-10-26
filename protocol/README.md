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
| 0 | `OPUS` | one Opus frame, mono. **What ships today** |
| 1 | `DTX` | comfort noise / silence marker. Not implemented yet |
| 2 | `KEEPALIVE` | empty. Counted as traffic, produces no audio |
| 3 | `PCM_DEBUG` | raw s16le mono. The fallback for a phone that cannot compress |
| 4 | `HELLO` | empty. **The only packet that travels PC → phone.** See [The reply](#the-reply) |

### Flags

The same byte means different things in the two directions, because only one packet type ever
travels PC → phone and it has no payload to put an answer in.

**Phone → PC**, on any packet:

| Bit | Name | Meaning |
|---|---|---|
| `0x01` | `FEC` | unused. Opus carries its redundant copies inside the packet, so recovery needs no flag |
| `0x02` | `ENC` | payload is encrypted. Not implemented yet |
| `0x04` | `MARK` | first packet after a silence |
| `0x08` | `CLEAN` | **clean this voice on the PC.** The phone's wish, not a report |
| `0x10` | `SAYS_CLEAN` | **this phone states its wish in bit `0x08`, whichever way it is set** |

**PC → phone**, in a `HELLO`:

| Bit | Name | Meaning |
|---|---|---|
| `0x08` | `CLEANING` | the receiver is cleaning the voice **right now** |
| `0x10` | `CAN_CLEAN` | the receiver has a working noise model and could clean if asked |

### Cleaning the voice, and why it takes two bits each way

Noise cleaning runs on the PC, which has the processor for a model far stronger than a phone can
carry. Whether it runs is the phone's choice, so the phone has to be able to say *no* as clearly as
it says *yes* — and a receiver has to be able to tell **a phone that said no** from **a phone too
old to say anything**, because both of those put a zero in bit `0x08`. That is what `SAYS_CLEAN`
is for:

| `SAYS_CLEAN` | `CLEAN` | What the receiver does |
|---|---|---|
| 0 | — | the phone is not asking either way. The receiver follows its own setting, which is **on** by default |
| 1 | 1 | clean |
| 1 | 0 | **do not clean**, whatever the receiver's own setting says |

A receiver with cleaning switched off locally never cleans and never sets `CLEANING`, whatever the
phone asks: a local *off* is a veto, not a preference.

**The wish rides on every audio packet rather than in a control packet of its own.** This is UDP: a
control packet can be the one that is lost, and then the two ends disagree about something the user
can hear until the next one happens along. A bit that is re-asserted fifty times a second cannot
drift, needs no handshake, and has nothing to go stale.

The two reply bits exist for the same reason the `HELLO` itself does. `CAN_CLEAN` is what lets the
phone offer the switch at all; `CLEANING` is what lets it show the truth rather than its own hope —
and a green badge that claimed more than it knew is a mistake this protocol has already made once.

### One stream, one format

A stream is either Opus or raw PCM for its whole life. The receiver reads the format from the first
audio packet of a session and keeps a decoder for it; a type that changes mid-stream throws that
decoder away and says so, because its state describes the audio it has already heard.

The phone decides at the start of a session and does not change its mind. If it cannot compress —
no bundled libopus for its processor, or a capture rate Opus does not take — it sends `PCM_DEBUG`
for the whole session instead. **Compression is an improvement, never a requirement.**

### An Opus payload says its own length

An `OPUS` payload is one Opus frame and nothing else: no container, no length prefix, no
identification header. The frame's first byte says how long it decodes to, which is what the
receiver reads — so unlike raw PCM, the datagram's length tells you nothing about the audio.

The receiver always decodes at **48 kHz**, whatever rate the phone captured at. Opus works at
48 kHz internally and resamples better than we would, so a 16 kHz capture is handed to it as 16 kHz
and comes back as 48 kHz with no resampling of ours in the path at all.

About **32–40 kbps** for a voice, against roughly 770 for the same voice as raw PCM.

### A lost Opus frame is rebuilt or invented, not silenced

Two things become possible once the audio is compressed, and the receiver does both:

- **Forward error correction.** Each packet can carry a coarse copy of the frame before it. When a
  frame is declared lost and the *next* one is already waiting, the receiver rebuilds the missing
  frame from that copy. The encoder decides whether to spend the bits; `OPUS_SET_PACKET_LOSS_PERC`
  on the phone is set to 5, which is the budget, not a measurement.
- **Concealment.** Failing that, the decoder is told a frame is missing and invents a plausible one
  from what it has just heard. The raw-PCM path can only push in a frame of silence, which is
  audible as a click.

The `FEC` flag is **not** used for this and remains unimplemented: recovery is read out of the
packet itself, so no flag has to be set for it to work.

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

`PCM_DEBUG` is the fallback, not the format: it is roughly 770 kbps, against 32–40 for the same
voice through Opus. Any build that can compress does.

## The reply

A `HELLO` is 16 bytes: the header alone, no payload. It is the only packet that ever travels from
the PC to the phone, and its three number fields are not the receiver's own — they are borrowed to
answer the phone with:

| Field | In a `HELLO` |
|---|---|
| sequence | the `sequence` of the packet being answered, echoed back |
| timestamp | how much audio the receiver is holding, in **tenths of a millisecond** (so `605` is 60.5 ms). Not a sample count — this is the one packet where that field means something else |
| ssrc | the phone's own `ssrc`, echoed back |

Its `flags` byte is the receiver's own, and carries `CLEANING` and `CAN_CLEAN` as above. A receiver
that does not know about cleaning sends zero there, which reads correctly as *cannot, and is not*.

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

The cleaning bits mismatch the same way, in both directions, with no negotiation:

| | |
|---|---|
| New phone, old PC | the flags are in a byte the old receiver reads and ignores. No reply bit comes back, so the phone reports *this PC cannot clean* and does not pretend otherwise |
| Old phone, new PC | `SAYS_CLEAN` is absent, so the receiver follows its own setting. Cleaning on by default means an old phone gets cleaned without knowing it asked |

## Rules a receiver must follow

- **Never trust a datagram.** It arrives from anyone on the LAN. Bounds-check every field, and
  treat a short, truncated, oversized or malformed packet as a counter to increment, never as an
  error to crash on
- **Compare sequence numbers with wrapping arithmetic.** A plain `a > b` breaks once every ~2.7
  years of continuous streaming at 50 packets/s, which is the least reproducible bug available.
  `proto::seq_diff` is the correct comparison
- **Hold late packets for a bounded window only**, then declare the gap and conceal it

## Finding the receiver

Not part of the datagram, and not a separate protocol: **the phone finds the PC by sending one
`KEEPALIVE` to every address on its own subnet and listening for the `HELLO` each receiver answers
with.** A keepalive is a valid packet carrying no audio, so a receiver answers it by existing — and
no address has to be read off one screen and typed into another.

254 datagrams of 16 bytes, once, when the user opens the screen. The reply is matched on the
**ssrc the phone put in the probe**, for the same reason the capture service matches replies on it:
anything on the network can send 16 bytes, and a reply that does not carry our own session number
is not evidence about us.

Deliberately **not** multicast, mDNS or broadcast. All three are filtered, rate limited or silently
dropped on enough home routers and phones to be unreliable, and a plain unicast knock is answered
by code that already shipped.

⚠️ **Discovery is not authentication.** Anything on the network can find a running receiver this
way, and the receiver accepts audio from whoever sends it. That is the same exposure the protocol
has always had — see the security notes in the repository README.

## Not in the protocol yet

Discovery, encryption, the PC → phone direction, and any control channel at all. When a control
channel arrives it will be TCP; audio stays on UDP.
