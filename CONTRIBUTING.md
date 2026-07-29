# Contributing to Earshot

Earshot is a real-time audio project, and real-time audio breaks in ways ordinary software does
not: late is the same as wrong, and a glitch nobody can reproduce is still a glitch. The rules
below exist because of that, not because of taste.

## Non-negotiables

**The audio callback does nothing but move samples.** No allocation, no locks, no logging, no file
or network I/O, no JNI or platform-channel calls. Cross a thread boundary with a lock-free ring
buffer and nothing else. A `println!` in a callback is a dropout.

**Audio goes over UDP and is never retransmitted.** TCP is for control only. A packet that arrives
late is worse than one that never arrives — waiting for it puts you further behind for the rest of
the call.

**One 20 ms frame per datagram, never coalesced.**

**150 ms glass-to-glass is a hard cap.** Any change that adds buffering states its cost in
milliseconds in the pull request.

**No latency number without a method, a device and a date.** "It feels instant" is not a
measurement, and neither is a number with no way to reproduce it.

**Nothing leaves the local network.** No analytics, no crash upload, no remote config, no phone
home. The README makes this promise to users, so the code has to keep it.

**Dependencies get a licence and maintenance check before they land.** GPL-3.0 compatibility is
required. Prefer no dependency at all.

## Protocol changes

`protocol/` is the source of truth for what the two halves say to each other. Changing the wire
format means, in a single commit: a version bump, both ends updated, and the test vectors
regenerated.

`protocol/pairing-vectors.csv` works the same way. Both `receiver/src/pairing.rs` and
`app/lib/pairing.dart` are tested against it, which is what stops the two implementations drifting
apart — if you touch either, run both suites.

## Building and testing

```bash
cd receiver && cargo test --locked && cargo clippy --locked --all-targets -- -D warnings
cd app && flutter test && flutter analyze
```

`--locked` is not optional. The lockfile is deliberately kept at version 3 so the Rust that Ubuntu
ships can read it; without `--locked` a newer cargo quietly rewrites it and the build stops working
on older toolchains.

CI builds the receiver on Linux **and** Windows on every push. There is no cross-compiler in the
usual development setup, so the Windows job is the only thing that compiles that target — if it
goes red, it is broken, and nothing local would have caught it.

## Never commit

Secrets, tokens, keystores, `key.properties`, `.env` files, service-account JSON — and equally:
home IP addresses, MAC addresses, SSIDs, device serial numbers, or voice recordings. Test data uses
made-up addresses for this reason.

Deleting one of these in a later commit does not remove it; it stays in the history and in every
clone. If something does get committed, rotate it rather than merely deleting it.

See `docs/pre-open-source-checklist.md`.
