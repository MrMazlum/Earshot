//! Earshot receiver — the PC half of the bridge.
//!
//! Phone → UDP → [`proto`] parse → [`reorder`] window → [`resample`] → [`ring`] → [`audio`] out.
//!
//! [`engine`] wires that chain together and runs it. The two front-ends — `earshot-receiver` in a
//! terminal, `earshot-tray` in the system tray — are both thin shells over it.
//!
//! The wire format is specified in `protocol/README.md`; the rules every change here has to obey
//! are in `CONTRIBUTING.md`.

pub mod audio;
#[cfg(all(feature = "tray", target_os = "linux"))]
pub mod autostart;
pub mod cable;
pub mod engine;
pub mod pairing;
pub mod proto;
pub mod reorder;
pub mod resample;
pub mod ring;
pub mod virtualmic;
