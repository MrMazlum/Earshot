//! What the tray shows, with no toolkit attached.
//!
//! Two front-ends draw this: `ksni` over D-Bus on Linux, and the Win32 notification area on
//! Windows. Neither of them decides what the words are, because the Windows one cannot be *run* on
//! the machine this is written on. Keeping every string here means `cargo test` on Linux checks the
//! text a Windows user will read, which is the only check available for it.
//!
//! The same reasoning covers [`icon_pixels`]: the tray icon is arithmetic rather than a resource
//! file, so what Windows draws in the notification area is decided by code that is tested here.

use crate::engine::{Engine, LanAddress};
use crate::pairing::Code;

/// The four states a tray icon has to be able to tell apart at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Deliberately not running: the user pressed Stop.
    Stopped,
    /// Tried to run and could not. The reason is in [`View::error`].
    Failed,
    /// Running, but the phone has not said anything yet.
    Waiting,
    /// Audio is arriving.
    Connected,
}

/// Everything the tray displays, copied out of the engine once a second so that rendering never
/// reads a half-updated set of atomics.
#[derive(Default, Clone)]
pub struct Snapshot {
    pub connected: bool,
    pub peer: Option<String>,
    pub pkt_per_sec: u32,
    pub lost: u64,
    pub late: u64,
    pub underruns: u64,
    pub buffered_ms: f32,
    pub src_rate: u32,
    pub port: u16,
    pub out_device: String,
    /// `Some(name)` when playing into a virtual microphone. On Windows this is the *recording* end
    /// of the cable — the name the user has to find in Discord's input list.
    pub virtual_mic: Option<String>,
}

impl Snapshot {
    /// Reads the engine's gauges. Does **not** drain the notice queue: that is destructive, and the
    /// front-end owns the decision about what to do with them.
    pub fn read(engine: &Engine) -> Snapshot {
        use std::sync::atomic::Ordering;
        let st = engine.status();
        let ready = engine.ready();
        Snapshot {
            connected: st.connected.load(Ordering::Relaxed),
            peer: st.peer().map(|p| p.ip().to_string()),
            pkt_per_sec: st.pkt_per_sec.load(Ordering::Relaxed),
            lost: st.lost.load(Ordering::Relaxed),
            late: st.late.load(Ordering::Relaxed),
            underruns: st.underruns.load(Ordering::Relaxed),
            buffered_ms: st.buffered_ms(),
            src_rate: st.src_rate.load(Ordering::Relaxed),
            port: ready.port,
            out_device: ready.out_device.clone(),
            virtual_mic: ready.virtual_mic.clone(),
        }
    }
}

/// A read-only view of the tray's state, and the one place that turns it into words.
pub struct View<'a> {
    /// Whether an engine exists at all.
    pub running: bool,
    /// Why there is no engine, when there is no engine.
    pub error: Option<&'a str>,
    pub snap: &'a Snapshot,
    pub lan: &'a [LanAddress],
}

impl View<'_> {
    pub fn state(&self) -> State {
        match (self.error, self.running, self.snap.connected) {
            (Some(_), _, _) => State::Failed,
            (None, false, _) => State::Stopped,
            (None, true, false) => State::Waiting,
            (None, true, true) => State::Connected,
        }
    }

    /// One line, the first thing in the menu and the first line of the tooltip.
    pub fn status_line(&self) -> String {
        match self.state() {
            // Menu labels are one line; the whole error goes in the tooltip and the About box.
            State::Failed => format!(
                "\u{26a0} {}",
                self.error
                    .and_then(|e| e.lines().next())
                    .unwrap_or("failed to start")
            ),
            State::Stopped => "Stopped".to_string(),
            State::Waiting => "\u{25cb} Waiting for the phone".to_string(),
            State::Connected => match &self.snap.peer {
                Some(ip) => format!("\u{25cf} Connected \u{2014} {ip}"),
                None => "\u{25cf} Connected".to_string(),
            },
        }
    }

    /// The second line: what it is doing rather than whether it is doing it.
    pub fn detail_line(&self) -> String {
        if !self.running {
            return "Not running".to_string();
        }
        if !self.snap.connected {
            return match &self.snap.virtual_mic {
                Some(_) => "Ready \u{2014} start the app on your phone".to_string(),
                None => format!("Playing to {}", self.snap.out_device),
            };
        }
        let rate = if self.snap.src_rate > 0 {
            format!("{} kHz", self.snap.src_rate / 1000)
        } else {
            "\u{2014}".to_string()
        };
        format!(
            "{rate} \u{b7} {} lost \u{b7} {:.0} ms buffered",
            self.snap.lost, self.snap.buffered_ms
        )
    }

    /// The pairing code for the most likely address, when there is one.
    ///
    /// `None` while stopped as well as when the address cannot be encoded: with no engine the port
    /// is not settled, and a code that names the wrong port is worse than no code.
    pub fn pairing_code(&self) -> Option<Code> {
        if !self.running {
            return None;
        }
        Code::new(self.lan.first()?.ip, self.snap.port)
    }

    /// What to type into the phone. The code when there is one, the address when there is not.
    pub fn address_line(&self) -> String {
        let Some(a) = self.lan.first() else {
            return "This PC: address unknown".to_string();
        };
        match self.pairing_code() {
            Some(code) => format!("Pairing code: {}", code.grouped()),
            None if !self.running => format!("This PC: {}", a.ip),
            None => format!("This PC: {} \u{b7} port {}", a.ip, self.snap.port),
        }
    }

    /// The one line that answers "so what do I pick in Discord?".
    ///
    /// It exists because on Windows the answer is *not* "Earshot" — Windows will not let a program
    /// invent a microphone, so the input is the recording end of a borrowed cable and is called
    /// something else entirely. Leaving the user to work that out was the single most confusing
    /// thing about the Windows build.
    pub fn input_line(&self) -> Option<String> {
        let name = self.snap.virtual_mic.as_ref()?;
        Some(format!("Microphone to pick: {name}"))
    }

    /// Only worth saying when the machine is on more than one network — then the first guess may
    /// well be the wrong one.
    pub fn other_addresses(&self) -> Option<String> {
        if self.lan.len() < 2 {
            return None;
        }
        let others: Vec<String> = self.lan[1..].iter().map(|a| a.to_string()).collect();
        Some(format!("also on: {}", others.join(", ")))
    }

    /// Hover text. Everything the menu says, plus the counters that would make the menu long.
    pub fn tooltip(&self) -> String {
        let mut lines = vec![format!("Earshot \u{2014} {}", self.status_line())];
        match self.error {
            Some(e) => lines.push(e.to_string()),
            None => {
                lines.push(self.detail_line());
                if self.snap.connected {
                    lines.push(format!(
                        "{} pkt/s \u{b7} {} late \u{b7} {} underruns",
                        self.snap.pkt_per_sec, self.snap.late, self.snap.underruns
                    ));
                }
                lines.push(self.address_line());
                if let Some(input) = self.input_line() {
                    lines.push(input);
                }
            }
        }
        if let Some(more) = self.other_addresses() {
            lines.push(more);
        }
        lines.join("\n")
    }

    /// The text of the "what do I do now?" box — the tray's answer to a user who has just
    /// double-clicked the icon and wants the number to type.
    pub fn summary(&self) -> String {
        let mut lines = Vec::new();
        match self.pairing_code() {
            Some(code) => {
                lines.push(format!("Pairing code:   {}", code.grouped()));
                lines.push(String::new());
                lines.push("Type those nine digits into the Earshot app on your phone,".into());
                lines.push("then press Start there.".into());
            }
            None if !self.running => {
                lines.push("Earshot is not running.".into());
                if let Some(e) = self.error {
                    lines.push(String::new());
                    lines.push(e.to_string());
                }
            }
            None => {
                let a = self.lan.first();
                lines.push("No pairing code covers this machine's address.".into());
                lines.push(String::new());
                match a {
                    Some(a) => lines.push(format!(
                        "In the phone app, choose \"Type an address instead\" and enter:\n    \
                         {}   port {}",
                        a.ip, self.snap.port
                    )),
                    None => lines.push("This machine has no LAN address Earshot recognises.".into()),
                }
            }
        }
        if let Some(input) = self.input_line() {
            lines.push(String::new());
            lines.push(input);
        }
        if let Some(more) = self.other_addresses() {
            lines.push(String::new());
            lines.push(format!("This PC is {more}"));
        }
        lines.join("\n")
    }
}

/// The tray icon, drawn rather than shipped.
///
/// A `.ico` in the binary would need a resource compiler in the build, and a themed icon name (the
/// Linux route) has no Windows equivalent. Both are avoided by generating the pixels: it is a few
/// lines of arithmetic, it has no build-time dependency at all, and — the part that actually
/// matters here — it can be checked by a test on a machine that cannot run Windows.
///
/// Returns `size * size` pixels in **BGRA** order, premultiplied by nothing, top row first. That is
/// the layout a Win32 32-bit DIB section wants, so the caller can memcpy it straight in.
///
/// The shape is the app's: a microphone capsule on a stand. Colour carries the state, because in a
/// 16-pixel notification area a shape change is invisible and a colour change is not.
pub fn icon_pixels(size: usize, state: State) -> Vec<u8> {
    // Alpha is what the notification area composites against an unknown background, so the icon is
    // drawn as coverage first and coloured afterwards. 4x4 supersampling is enough to keep a 16 px
    // capsule from looking like a staircase.
    const SUB: usize = 4;
    let (r, g, b) = match state {
        // Grey: off, and not pretending otherwise.
        State::Stopped => (0x9au8, 0x9au8, 0xa0u8),
        // Red, for the one state that needs the user to do something.
        State::Failed => (0xe0, 0x53, 0x4f),
        // The app's blue: running, nothing wrong, nothing happening.
        State::Waiting => (0x4a, 0x8f, 0xe8),
        // Green only once audio is genuinely arriving, so the colour means something.
        State::Connected => (0x3f, 0xba, 0x6f),
    };

    let mut pixels = vec![0u8; size * size * 4];
    let s = size as f32;
    for y in 0..size {
        for x in 0..size {
            let mut hits = 0u32;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    // Sample at the centre of each sub-pixel, in a 0..1 square.
                    let u = (x as f32 + (sx as f32 + 0.5) / SUB as f32) / s;
                    let v = (y as f32 + (sy as f32 + 0.5) / SUB as f32) / s;
                    if in_microphone(u, v) {
                        hits += 1;
                    }
                }
            }
            let alpha = (hits * 255 / (SUB * SUB) as u32) as u8;
            let i = (y * size + x) * 4;
            pixels[i] = b;
            pixels[i + 1] = g;
            pixels[i + 2] = r;
            pixels[i + 3] = alpha;
        }
    }
    pixels
}

/// The microphone glyph, in a unit square. `true` where the icon is opaque.
///
/// Deliberately chunky: this is read at 16 px, where a faithful drawing of a microphone turns into
/// four grey smudges. A capsule, a gap, an arc and a stem survive the size.
fn in_microphone(u: f32, v: f32) -> bool {
    // Capsule: a vertical rounded bar in the upper half.
    let capsule = {
        let half_w = 0.115;
        let (top, bottom) = (0.14, 0.52);
        let dx = (u - 0.5).abs();
        if dx > half_w {
            false
        } else if v >= top && v <= bottom {
            true
        } else {
            // The rounded ends, as circles of the bar's own radius.
            let cy = if v < top { top } else { bottom };
            let dy = v - cy;
            dx * dx + dy * dy <= half_w * half_w
        }
    };

    // The cradle: an arc under the capsule, open at the top. Drawn as the band between two circles.
    let cradle = {
        let dx = u - 0.5;
        let dy = v - 0.5;
        let d2 = dx * dx + dy * dy;
        // Only the lower half of the ring, so it reads as a cradle rather than a full circle.
        v > 0.5 && (0.235 * 0.235..=0.30 * 0.30).contains(&d2)
    };

    // The stem, from the bottom of the cradle to the foot.
    let stem = (u - 0.5).abs() <= 0.055 && (0.79..=0.90).contains(&v);

    // The foot, so the glyph does not end in a point.
    let foot = (u - 0.5).abs() <= 0.20 && (0.87..=0.93).contains(&v);

    capsule || cradle || stem || foot
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn lan(ip: [u8; 4]) -> Vec<LanAddress> {
        vec![LanAddress {
            ip: Ipv4Addr::from(ip),
            interface: "wlan0".to_string(),
        }]
    }

    fn snap(port: u16) -> Snapshot {
        Snapshot {
            port,
            out_device: "Speakers".to_string(),
            ..Snapshot::default()
        }
    }

    fn view<'a>(running: bool, error: Option<&'a str>, s: &'a Snapshot, l: &'a [LanAddress]) -> View<'a> {
        View {
            running,
            error,
            snap: s,
            lan: l,
        }
    }

    #[test]
    fn the_four_states_are_distinguished() {
        let l = lan([192, 168, 1, 42]);
        let mut s = snap(47811);
        assert_eq!(view(false, None, &s, &l).state(), State::Stopped);
        assert_eq!(view(true, None, &s, &l).state(), State::Waiting);
        s.connected = true;
        assert_eq!(view(true, None, &s, &l).state(), State::Connected);
        // An error wins over everything: it is the only state the user has to act on.
        assert_eq!(view(true, Some("no cable"), &s, &l).state(), State::Failed);
    }

    /// The code is the whole reason the tray exists — a user who misses it has nothing to type.
    #[test]
    fn a_running_receiver_offers_a_pairing_code_everywhere_it_can() {
        let l = lan([192, 168, 1, 42]);
        let s = snap(47811);
        let v = view(true, None, &s, &l);
        let code = v.pairing_code().expect("a private address has a code");
        assert!(v.address_line().contains(&code.grouped()));
        assert!(v.tooltip().contains(&code.grouped()));
        assert!(v.summary().contains(&code.grouped()));
    }

    /// A stopped receiver has not bound a port yet, so any code it printed would name the wrong
    /// one. Better to say the address and nothing more.
    #[test]
    fn a_stopped_receiver_never_shows_a_code() {
        let l = lan([192, 168, 1, 42]);
        let s = snap(0);
        let v = view(false, None, &s, &l);
        assert!(v.pairing_code().is_none());
        assert_eq!(v.address_line(), "This PC: 192.168.1.42");
    }

    /// A public address has no code. Saying so, and giving the address instead, is the difference
    /// between a dead end and a workaround.
    #[test]
    fn an_address_with_no_code_falls_back_to_the_address() {
        let l = vec![LanAddress {
            ip: Ipv4Addr::new(93, 184, 216, 34),
            interface: "eth0".to_string(),
        }];
        let s = snap(47811);
        let v = view(true, None, &s, &l);
        assert!(v.pairing_code().is_none());
        assert!(v.summary().contains("93.184.216.34"));
        assert!(v.summary().contains("port 47811"));
    }

    /// The Windows complaint this whole line exists for: the input is not called Earshot, and
    /// nothing on screen used to say what it *was* called.
    #[test]
    fn the_windows_input_name_is_stated_rather_than_assumed() {
        let l = lan([192, 168, 1, 42]);
        let s = Snapshot {
            virtual_mic: Some("CABLE Output".to_string()),
            ..snap(47811)
        };
        let v = view(true, None, &s, &l);
        assert_eq!(
            v.input_line().as_deref(),
            Some("Microphone to pick: CABLE Output")
        );
        assert!(v.tooltip().contains("CABLE Output"));
        assert!(v.summary().contains("CABLE Output"));
    }

    /// Playing to the speakers is not a microphone, so there is nothing to pick.
    #[test]
    fn without_a_virtual_microphone_there_is_no_input_to_name() {
        let l = lan([10, 0, 0, 5]);
        let s = snap(47811);
        assert!(view(true, None, &s, &l).input_line().is_none());
    }

    #[test]
    fn a_failure_puts_its_reason_where_the_user_will_see_it() {
        let l = lan([192, 168, 1, 42]);
        let s = snap(47811);
        let v = view(false, Some("No virtual audio cable is installed.\nmore detail"), &s, &l);
        assert!(v.status_line().contains("No virtual audio cable"));
        // One line in the menu label, the whole thing in the tooltip.
        assert_eq!(v.status_line().lines().count(), 1);
        assert!(v.tooltip().contains("more detail"));
        assert!(v.summary().contains("more detail"));
    }

    #[test]
    fn a_second_network_is_mentioned_and_a_single_one_is_not() {
        let s = snap(47811);
        let one = lan([192, 168, 1, 42]);
        assert!(view(true, None, &s, &one).other_addresses().is_none());

        let two = vec![
            LanAddress { ip: Ipv4Addr::new(192, 168, 1, 42), interface: "wlan0".into() },
            LanAddress { ip: Ipv4Addr::new(10, 0, 0, 5), interface: "eth0".into() },
        ];
        let v = view(true, None, &s, &two);
        assert!(v.other_addresses().unwrap().contains("10.0.0.5"));
        assert!(v.tooltip().contains("10.0.0.5"));
    }

    /// `szTip` is 128 UTF-16 units and `szInfo` is 256. Neither is generous, and a tooltip that
    /// overruns is silently truncated by Windows rather than rejected, so the important lines have
    /// to come first.
    #[test]
    fn the_tooltip_leads_with_the_status_and_the_code() {
        let l = lan([192, 168, 1, 42]);
        let s = Snapshot {
            connected: true,
            peer: Some("192.168.1.99".to_string()),
            virtual_mic: Some("CABLE Output".to_string()),
            ..snap(47811)
        };
        let v = view(true, None, &s, &l);
        let tip = v.tooltip();
        let lines: Vec<&str> = tip.lines().collect();
        assert!(lines[0].starts_with("Earshot"));
        assert!(tip.find("Pairing code").unwrap() < 128);
    }

    #[test]
    fn the_icon_is_the_size_it_was_asked_for_and_fully_bgra() {
        for size in [16usize, 32, 64] {
            assert_eq!(icon_pixels(size, State::Waiting).len(), size * size * 4);
        }
    }

    /// A tray icon that is transparent everywhere is an invisible tray icon, and a tray icon that
    /// is opaque everywhere is a square. Both have shipped in other people's code.
    #[test]
    fn the_icon_is_neither_blank_nor_a_solid_block() {
        let px = icon_pixels(32, State::Connected);
        let opaque = px.chunks(4).filter(|p| p[3] > 200).count();
        assert!(opaque > 60, "almost nothing is drawn: {opaque} of 1024");
        assert!(opaque < 700, "that is a filled square: {opaque} of 1024");
    }

    /// The colour is the only thing distinguishing the states at 16 px, so it has to actually
    /// differ — and the drawn shape has to not.
    #[test]
    fn every_state_gets_its_own_colour_over_the_same_shape() {
        let states = [State::Stopped, State::Failed, State::Waiting, State::Connected];
        let mut colours = Vec::new();
        let mut shapes = Vec::new();
        for st in states {
            let px = icon_pixels(32, st);
            // The centre of the capsule is opaque in every state, so it carries the colour.
            let i = (8 * 32 + 16) * 4;
            assert_eq!(px[i + 3], 255, "the capsule should be solid for {st:?}");
            colours.push((px[i], px[i + 1], px[i + 2]));
            shapes.push(px.chunks(4).map(|p| p[3]).collect::<Vec<u8>>());
        }
        for i in 1..colours.len() {
            assert_ne!(colours[0], colours[i], "{:?} looks like Stopped", states[i]);
            assert_eq!(shapes[0], shapes[i], "{:?} is a different shape", states[i]);
        }
    }

    /// The icon is composited over a panel that may be black or white, so the edges have to be
    /// genuinely translucent rather than a hard cut.
    #[test]
    fn the_edges_are_antialiased() {
        let px = icon_pixels(32, State::Waiting);
        let partial = px.chunks(4).filter(|p| p[3] > 10 && p[3] < 245).count();
        assert!(partial > 20, "no soft edge anywhere: {partial} pixels");
    }
}
