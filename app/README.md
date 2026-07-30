# app — the Android client

Flutter UI, Kotlin audio capture. Build with `flutter build apk --debug`.

- `lib/main.dart` — the whole UI: where to send, which microphone to use, level meter, stats
- `android/.../MicService.kt` — the foreground service that records and sends. The capture thread
  lives here; it must not allocate, lock or log
- `android/.../Protocol.kt` — the wire format. Must stay byte-identical to `receiver/src/proto.rs`;
  `header_wire_bytes_are_frozen` on the Rust side is what catches a drift

The microphone picker is not a settings nicety — it is the experiment this app exists to run. Each
`AudioSource` runs a different amount of the phone's noise cancellation, and the good ones may force
16 kHz. Three of the five are locked until they can be shown to sound different on a real phone.

The wire format is specified in [`protocol/README.md`](../protocol/README.md); the rules a change
here has to obey are in [`CONTRIBUTING.md`](../CONTRIBUTING.md).
