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
#[cfg(all(feature = "tray", any(target_os = "linux", target_os = "windows")))]
pub mod autostart;
pub mod cable;
pub mod engine;
/// The text every front-end shows when nothing is arriving. Shared so the Windows wording is
/// testable on Linux.
pub mod help;
pub mod pairing;
pub mod proto;
pub mod reorder;
pub mod resample;
pub mod ring;
/// What the tray shows, shared by both front-ends so the Windows text is testable on Linux.
#[cfg(feature = "tray")]
pub mod trayui;
pub mod virtualmic;
/// The Win32 notification area. Windows has no `ksni`, so this is hand-rolled.
#[cfg(all(feature = "tray", target_os = "windows"))]
pub mod wintray;
