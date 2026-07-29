# receiver — the PC side

```
UDP :47811 → proto::parse → Reorder → Resampler → SpscRing → cpal callback → sound card
```

Three binaries, one engine. `engine.rs` does the receiving; the rest are front-ends over it.

| | |
|---|---|
| `earshot-tray` | **the one to use.** A tray icon: status, start/stop, virtual mic, start-at-login. Needs no terminal |
| `earshot-receiver` | the same thing in a terminal, with a stats line. `--help` for options; `--list-devices` to pick an output |
| `earshot-testsend` | a fake phone, so the PC half can be developed and debugged alone |

```bash
cargo run --release --bin earshot-receiver -- --buffer-ms 60
cargo run --release --bin earshot-testsend -- --loss 5 --jitter 25
```

To make other applications see the phone as a microphone (Linux):

```bash
cargo run --release --bin earshot-receiver -- --virtual-mic
```

Then pick **Earshot** in Discord's input list. The device is created on first use and left loaded
afterwards, so applications do not forget which input you chose; `--remove-virtual-mic` deletes it.

### The tray

```bash
cargo run --release --bin earshot-tray            # try it
cargo run --release --bin earshot-tray -- --install   # keep it: copies to ~/.local/bin, starts at login
```

`--install` copies the binary out of `target/` on purpose — a login item pointing into a build tree
breaks silently the first time anyone runs `cargo clean`. `--uninstall` removes the login item.

The tray defaults to `--virtual-mic`; that is the point of it. The toggle in its menu applies to the
session only — the login item remembers whatever was set when you ran `--install`.

⚠️ **GNOME shows no tray icons without the AppIndicator extension.** Ubuntu enables it by default,
so this works out of the box there; on stock GNOME the process runs and shows nothing.

`ksni` was chosen over `tray-icon`/`libappindicator` because it speaks StatusNotifierItem over D-Bus
in pure Rust: no `libayatana-appindicator3-dev`, no GTK, nothing to `apt install` before building.

**Rust 1.75 is the minimum** — that is what `apt install cargo` gives on Ubuntu 24.04, and it is
what this builds and tests against. The lockfile is kept at version 3 for the same reason; a newer
cargo will silently rewrite it to version 4, which 1.75 then refuses to read. If you regenerate it
with a rustup toolchain, check `version = 3` is still the first entry before committing.

## Files

| | |
|---|---|
| `proto.rs` | the wire format. Byte-identical to `app/android/.../Protocol.kt`, and `header_wire_bytes_are_frozen` is the test that keeps it that way. `parse` never panics — it reads packets from anyone on the LAN |
| `reorder.rs` | puts packets back in order, declares gaps, refuses to play a packet that arrived after its turn, and resyncs if the sender restarts |
| `resample.rs` | phone rate → sound-card rate, e.g. a 16 kHz mic into a 44.1 kHz card |
| `ring.rs` | the lock-free handoff to the audio callback. Its fill level *is* the buffering latency |
| `audio.rs` | cpal output. Everything in the callback is allocation- and lock-free |
| `virtualmic.rs` | creates the PipeWire null sink + remapped source that Discord sees |
| `engine.rs` | the receive loop, headless. Owns the thread and publishes `Status`; both front-ends read it. **Behaviour changes belong here**, not in a front-end. Also picks the address to show the phone — see below |
| `autostart.rs` | the `~/.config/autostart` login item |
| `main.rs` | terminal front-end: the banner and the once-a-second stats line |
| `bin/tray.rs` | tray front-end: icon, menu, tooltip |

## Rules that are not negotiable here

- **Nothing blocks the audio callback.** No locks, no allocation, no logging, no I/O. To get data
  to it, push it through `SpscRing`
- **Late is worse than missing.** A packet that arrives after its slot is dropped, never played
- **Every buffer states its cost in milliseconds.** 150 ms mouth-to-ear is a hard cap
- **Never offer the phone an address it cannot reach.** "Which IP do I type?" is answered by
  enumerating interfaces, *not* by asking the kernel which route reaches 8.8.8.8. With a VPN up
  (Cloudflare WARP, Tailscale) the default route leaves through a `/32` tunnel that no phone on the
  Wi-Fi can talk to, and the receiver used to print exactly that. `is_lan_candidate` in `engine.rs`
  is the rule, and its tests name the cases

Design notes: `~/EarshotBrain/` — start with `06-Latency-Budget.md` and `05-Wire-Protocol.md`.

## Not here yet

Opus decoding (needs `libopus-dev`), the virtual microphone on **Windows and macOS** (Linux works),
the PC → phone direction, discovery, pairing and encryption.
