# Changelog

Notable changes, newest first. Releases before 0.2.0 are on the
[releases page](https://github.com/MrMazlum/Earshot/releases); this file starts where it was
started.

## 0.2.0

**Noise cleaning moves from the phone to the PC.**

The phone carried RNNoise from 0.1.7. It was listened to properly for the first time and it was not
enough — a keyboard and mouse clicks still came through. That is not a bug in the build: RNNoise is
a 2017 model trained on *steady* noise, and a key press is a transient, which is the one thing it
was never good at.

The PC has the processor for a much stronger model, so that is where this happens now.

### Added

- **Noise cleaning in the receiver**, with DeepFilterNet 3 built in and **on by default**. Measured
  on a 48 kHz stream: 20 ms of added delay, about 1/30th of real time, roughly 8 dB off the noise.
- `--clean on|off|<model file>`, plus `--atten <dB>` and `--post-filter`. A model file may be any
  DeepFilterNet archive — the low-latency DeepFilterNet 3 adds *no* delay and cleans a little less.
  It is not bundled because it is 36 MB against the built-in model's 8.
- **A switch on the phone**, on by default, which works **mid-session**: the wish travels on every
  packet, so nothing restarts and nothing has to be acknowledged.
- The phone shows **what the PC reports**, not what its own switch wishes for. Against a receiver
  that cannot clean it says so.
- Two flag bits in each direction in the wire format, documented in
  [`protocol/README.md`](protocol/README.md). No new packet type and no protocol version bump.
- `earshot-record` and `earshot-clean`, development tools: capture what the phone sends to a wav
  without playing it, and run one recording through every cleaner with each one's delay, speed and
  level change. Neither ships in a release.

### Removed

- **RNNoise, from the phone** — the third audio choice, the JNI, the CMake fetch, the trained
  weights and the ARM header shim. The Android release is about 17 MB smaller as a result.
  *Voice call* still cleans, because that is Android's own capture chain and not RNNoise.
- **DeepFilterNet 2 as an option.** The pinned DeepFilterNet revision's `tract` aborts on a reshape
  it has not implemented, so that model cannot run at all. Dropping it took 8.6 MB off the receiver
  binary. A model archive that cannot run is now reported instead of aborting the process.

### Fixed

- `earshot-testsend --opus` had never sent more than one frame. The sample loop wrote raw PCM into
  the packet buffer at fixed offsets while the Opus path truncated that buffer to the compressed
  length, so the second frame indexed past the end and panicked. Broken since Opus landed.
- The microphone row on the phone was locked shut while a session ran, which made the one live
  control unreachable exactly when it was wanted.

### Notes

- Cleaning runs **before** resampling and only at 48 kHz. The models want that rate and a sound
  card's rate is whatever the sound card says; a stream at any other rate is passed through
  untouched rather than resampled into a model and back out.
- `--clean off` on the receiver is a veto and beats anything the phone asks for.
- The receiver's version had drifted to 0.1.7 while the app was at 0.1.8. Both are 0.2.0 from here.
- The tray menu has no cleaning toggle yet; the command-line flag and the phone's switch both work.
