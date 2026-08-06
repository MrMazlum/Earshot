//! The receive loop with no user interface attached.
//!
//! `main.rs` prints it to a terminal, `bin/tray.rs` hangs a tray icon off it. Everything they both
//! need to know lives in [`Status`], which is plain atomics plus a few short-lived mutexes — read
//! it as often as you like from anywhere.
//!
//! The loop itself runs on its own thread because [`audio::Output`] owns a `cpal::Stream`, which is
//! `!Send`: it has to be created on, and stay on, the thread that uses it. That is also why setup
//! errors come back over a channel instead of a return value.

use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::audio;
use crate::proto::{self, Header, ParseError};
use crate::reorder::{Release, Reorder};
use crate::resample::{s16le_to_f32, Resampler};

pub const DEFAULT_PORT: u16 = 47811;
pub const DEFAULT_BUFFER_MS: u32 = 60;

/// One 20 ms frame is at most 1920 bytes of PCM + 16 header. This is comfortably larger, and
/// anything bigger than a frame is not ours anyway.
const MAX_DATAGRAM: usize = 4096;
/// Silence for this long and the phone is treated as gone. Long enough to ride out a Wi-Fi hiccup,
/// short enough that the tray icon does not lie about being connected.
const IDLE_AFTER: Duration = Duration::from_millis(1500);
/// How often the loop wakes up when nothing is arriving — also the worst-case stop latency.
const POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub buffer_ms: u32,
    pub device: Option<String>,
    pub virtual_mic: bool,
    pub virtual_mic_name: String,
    pub verbose: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: format!("0.0.0.0:{DEFAULT_PORT}"),
            buffer_ms: DEFAULT_BUFFER_MS,
            device: None,
            virtual_mic: false,
            virtual_mic_name: "Earshot".to_string(),
            verbose: false,
        }
    }
}

/// What the receiver settled on once it was running. Known only after a successful start.
#[derive(Clone, Debug)]
pub struct Ready {
    pub port: u16,
    pub out_device: String,
    pub out_rate: u32,
    pub out_channels: u16,
    /// `Some(name)` when playing into a virtual microphone rather than the speakers.
    pub virtual_mic: Option<String>,
    /// True when this run is what created the virtual microphone, rather than reusing one.
    pub virtual_mic_created: bool,
}

/// Live state, shared with whatever is displaying it.
///
/// Counters are cumulative for the lifetime of the engine; `pkt_per_sec`, `kbps` and
/// `buffered_ms_x10` are gauges refreshed once a second.
#[derive(Default)]
pub struct Status {
    pub running: AtomicBool,
    pub connected: AtomicBool,
    pub src_rate: AtomicU32,
    pub pkt_per_sec: AtomicU32,
    pub kbps: AtomicU32,
    buffered_ms_x10: AtomicU32,
    pub packets: AtomicU64,
    pub lost: AtomicU64,
    pub late: AtomicU64,
    pub duplicates: AtomicU64,
    pub underruns: AtomicU64,
    pub ring_drops: AtomicU64,
    pub rejected: AtomicU64,
    /// Samples dropped to stop clock drift turning into unbounded latency. A steadily rising count
    /// means the two clocks disagree; a zero means they do not.
    pub trimmed: AtomicU64,
    peer: Mutex<Option<SocketAddr>>,
    notices: Mutex<Vec<String>>,
    fatal: Mutex<Option<String>>,
}

/// A poisoned lock here means a *display* thread panicked while formatting. The audio must not stop
/// for that, so the data is taken back rather than propagating the panic.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Status {
    pub fn peer(&self) -> Option<SocketAddr> {
        *lock(&self.peer)
    }

    /// Milliseconds sitting in the ring — the buffering half of the latency budget, live.
    pub fn buffered_ms(&self) -> f32 {
        self.buffered_ms_x10.load(Ordering::Relaxed) as f32 / 10.0
    }

    /// The error that stopped the loop, if one did.
    pub fn fatal(&self) -> Option<String> {
        lock(&self.fatal).clone()
    }

    /// One-off things worth saying out loud: a protocol mismatch, a rate change, a disconnect.
    /// Draining is destructive so two displays should not both call it.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.notices))
    }

    fn notice(&self, msg: impl Into<String>) {
        let mut q = lock(&self.notices);
        // A UI that stopped draining must not grow this without bound.
        if q.len() < 64 {
            q.push(msg.into());
        }
    }
}

pub struct Engine {
    status: Arc<Status>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    ready: Ready,
}

impl Engine {
    /// Binds, opens the output, and starts receiving. Returns once the loop is actually running,
    /// so a failure to bind or to open the sound card is reported here and not swallowed.
    pub fn start(config: Config) -> Result<Engine, String> {
        let status = Arc::new(Status::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Result<Ready, String>>();

        let thread = {
            let status = Arc::clone(&status);
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("earshot-receive".into())
                .spawn(move || receive_thread(config, status, stop, tx))
                .map_err(|e| format!("cannot start the receive thread: {e}"))?
        };

        // The sender is dropped if the thread panics during setup, which turns into RecvError.
        match rx.recv() {
            Ok(Ok(ready)) => Ok(Engine {
                status,
                stop,
                thread: Some(thread),
                ready,
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("the receive thread died during startup".to_string()),
        }
    }

    pub fn status(&self) -> &Arc<Status> {
        &self.status
    }

    pub fn ready(&self) -> &Ready {
        &self.ready
    }

    /// True until the loop exits — which, short of `stop`, means something went wrong.
    pub fn is_running(&self) -> bool {
        self.status.running.load(Ordering::Relaxed)
    }

    /// Asks the loop to finish and waits for it. Takes up to [`POLL`].
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// An address on this machine that the phone could plausibly reach.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanAddress {
    pub ip: Ipv4Addr,
    pub interface: String,
}

impl std::fmt::Display for LanAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.ip, self.interface)
    }
}

/// Interfaces that are never the Wi-Fi the phone is on: container bridges, VM bridges, and the
/// tunnel devices of VPNs that name themselves conventionally.
///
/// These are *Linux* device names, matched at the start, where a name is short and the prefix is
/// the whole identity of the thing (`docker0`, `wg0`, `virbr1`).
const VIRTUAL_PREFIXES: &[&str] = &[
    "virbr", "docker", "br-", "veth", "tun", "tap", "wg", "tailscale", "zt", "utun", "vmnet",
];

/// The same idea for Windows, which needs its own list and its own matching rule.
///
/// `if_addrs` reports the adapter's **friendly name** on Windows — what Network Connections shows —
/// so the interesting word is in the middle, not at the start: `VMware Network Adapter VMnet8`,
/// `vEthernet (Default Switch)`, `ZeroTier One [8056c2e21c000001]`. A prefix test sees none of
/// those, which matters because a VMware or VirtualBox host-only adapter holds a perfectly ordinary
/// `192.168.x.1/24` — private, not a `/32`, and therefore ranked *first*, ahead of the real Wi-Fi.
/// The receiver would then lead with a pairing code for a network the phone cannot reach.
///
/// Kept specific enough not to swallow a real adapter: `vethernet` does not match `Ethernet`.
const VIRTUAL_SUBSTRINGS: &[&str] = &[
    "vmware",
    "virtualbox",
    "vbox",
    "vmnet",
    "vethernet",
    "hyper-v",
    "zerotier",
    "hamachi",
    "radmin",
    "wireguard",
    "wintun",
    "openvpn",
    "tap-windows",
    "nordlynx",
    "loopback",
    "npcap",
];

fn looks_virtual(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    VIRTUAL_PREFIXES.iter().any(|p| name.starts_with(p))
        || VIRTUAL_SUBSTRINGS.iter().any(|s| name.contains(s))
}

/// Whether an address is worth offering as "type this into your phone".
///
/// The `/32` rule is what matters in practice. A VPN hands out a single address with no subnet
/// behind it — Cloudflare WARP gives `172.16.0.2/32`, Tailscale a `100.64/32` — and that address is
/// private, so a plain "is it RFC1918" test happily returns something the phone can never reach.
/// An interface with no local subnet has no LAN on it, whatever it is called.
fn is_lan_candidate(name: &str, ip: Ipv4Addr, netmask: Ipv4Addr) -> bool {
    !ip.is_loopback()
        && !ip.is_link_local()
        && ip.is_private()
        && netmask != Ipv4Addr::new(255, 255, 255, 255)
        && !looks_virtual(name)
}

/// Home routers hand out 192.168/16 far more often than the other two private ranges, so when a
/// machine has several this is the one to show first. It is a guess about which network the phone
/// is on, which is why [`lan_addresses`] returns all of them rather than only this one.
fn rank(ip: Ipv4Addr) -> u8 {
    match ip.octets() {
        [192, 168, ..] => 0,
        [10, ..] => 1,
        _ => 2,
    }
}

/// Every address the phone might be able to reach, best guess first.
pub fn lan_addresses() -> Vec<LanAddress> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut found: Vec<LanAddress> = interfaces
        .into_iter()
        .filter_map(|iface| match iface.addr {
            if_addrs::IfAddr::V4(v4) if is_lan_candidate(&iface.name, v4.ip, v4.netmask) => {
                Some(LanAddress {
                    ip: v4.ip,
                    interface: iface.name,
                })
            }
            _ => None,
        })
        .collect();
    found.sort_by_key(|a| (rank(a.ip), a.ip));
    found.dedup();
    found
}

/// This machine's address on the LAN, to be typed into the phone app.
///
/// ⚠️ Deliberately **not** "whatever the default route uses". When a VPN is up the default route
/// leaves through the tunnel, and this printed a tunnel address that no phone on the Wi-Fi could
/// reach — a bug that cost real debugging time. Falling back to that probe is still better than
/// printing nothing, but only as a last resort.
pub fn lan_ip() -> Option<IpAddr> {
    if let Some(best) = lan_addresses().first() {
        return Some(IpAddr::V4(best.ip));
    }
    default_route_ip()
}

/// `connect` on a UDP socket sends nothing — it only asks the kernel which interface it *would*
/// use. **No packet leaves the machine**, which is what keeps the "nothing leaves your network"
/// promise honest; verify it with `strace -e trace=network` or a packet capture if you like.
fn default_route_ip() -> Option<IpAddr> {
    let probe = UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect("8.8.8.8:80").ok()?;
    probe.local_addr().ok().map(|a| a.ip())
}

fn receive_thread(
    config: Config,
    status: Arc<Status>,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Result<Ready, String>>,
) {
    let started = match setup(&config) {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };
    let (socket, out, ready) = started;

    status.running.store(true, Ordering::Relaxed);
    if tx.send(Ok(ready)).is_err() {
        return; // whoever asked for this gave up already
    }

    let result = pump(&config, &socket, &out, &status, &stop);

    if let Err(e) = result {
        *lock(&status.fatal) = Some(e.clone());
        status.notice(format!("stopped: {e}"));
    }
    status.running.store(false, Ordering::Relaxed);
    status.connected.store(false, Ordering::Relaxed);
}

type Started = (UdpSocket, audio::Output, Ready);

fn setup(config: &Config) -> Result<Started, String> {
    // Bind first. It is the failure everyone actually hits — a second receiver already running —
    // and failing here leaves no half-built audio devices behind.
    let socket = UdpSocket::bind(&config.listen).map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse => format!(
            "{} is already in use - another Earshot receiver is probably running.\n\
             Stop it, or give this one its own port with --listen 0.0.0.0:47899",
            config.listen
        ),
        _ => format!("cannot bind {}: {e}", config.listen),
    })?;

    // Must happen before the output stream is opened. On Linux `ensure` sets PULSE_SINK, which the
    // PulseAudio plugin reads when the device is opened and never again.
    let virtual_mic = if config.virtual_mic {
        Some(crate::virtualmic::ensure(&config.virtual_mic_name)?)
    } else {
        crate::virtualmic::clear_routing();
        None
    };

    // Each platform knows which device its virtual mic needs us to open — "pulse" on Linux, the
    // cable's playback device on Windows. An explicit --device still wins, so an unusual setup can
    // always be steered by hand.
    let device = match (&virtual_mic, config.device.as_deref()) {
        (_, Some(explicit)) => Some(explicit.to_string()),
        (Some(v), None) => Some(v.device_hint.clone()),
        (None, None) => None,
    };

    let out = audio::open(device.as_deref(), config.buffer_ms)?;

    socket
        .set_read_timeout(Some(POLL))
        .map_err(|e| format!("cannot set socket timeout: {e}"))?;

    let ready = Ready {
        port: socket.local_addr().map(|a| a.port()).unwrap_or(DEFAULT_PORT),
        out_device: out.device_name.clone(),
        out_rate: out.sample_rate,
        out_channels: out.channels,
        virtual_mic: virtual_mic.as_ref().map(|v| v.display.clone()),
        virtual_mic_created: virtual_mic.as_ref().map(|v| v.created).unwrap_or(false),
    };
    Ok((socket, out, ready))
}

/// Whether a datagram from `from` is allowed in, given who is currently sending and whether that
/// sender is still mid-stream.
///
/// This is the whole of Earshot's sender policy, which is why it is one named function rather than
/// three lines buried in the receive loop. It is *not* authentication and cannot be: an attacker
/// who spoofs the phone's source address still gets in, and until the audio is encrypted there is
/// nothing to check anyone against. What it buys is that taking over a *live* microphone feed now
/// requires that spoof, where before it took one packet from any address on the network - which
/// both reassigned the peer and, carrying an unfamiliar SSRC, reset the reorder buffer.
///
/// `streaming` means "the last accepted packet arrived less than [`IDLE_AFTER`] ago" - the same
/// silence that already means the phone stopped. So a phone that reconnects on a new source port
/// is let back in within a second and a half rather than locked out of its own receiver.
fn accepts_from(current: Option<SocketAddr>, from: SocketAddr, streaming: bool) -> bool {
    match current {
        Some(peer) if peer != from => !streaming,
        _ => true,
    }
}

fn pump(
    config: &Config,
    socket: &UdpSocket,
    out: &audio::Output,
    status: &Status,
    stop: &AtomicBool,
) -> Result<(), String> {
    // Hold roughly `buffer_ms` of frames while waiting for a straggler — past that the gap is real.
    let reorder_depth = ((config.buffer_ms / proto::FRAME_MS) as usize).clamp(2, 16);
    let mut reorder = Reorder::new(reorder_depth);
    // Rate 0 is a "not yet known" marker: the first frame's length tells us the real rate.
    let mut resampler = Resampler::new(0, out.sample_rate);

    let mut buf = [0u8; MAX_DATAGRAM];
    let mut pcm = Vec::with_capacity(2048);
    let mut resampled = Vec::with_capacity(8192);

    let mut bytes_in = 0u64;
    let mut packets_in = 0u64;
    let mut unsupported_type = 0u64;
    let mut last_report = Instant::now();
    let mut last_packet: Option<Instant> = None;
    let mut warned_opus = false;
    let mut warned_version = false;
    let mut warned_intruder = false;
    let mut intruders = 0u64;

    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                let (header, payload) = match Header::parse(&buf[..n]) {
                    Ok(v) => v,
                    Err(e) => {
                        status.rejected.fetch_add(1, Ordering::Relaxed);
                        if config.verbose {
                            status.notice(format!("dropped {n} bytes from {from}: {e:?}"));
                        } else if matches!(e, ParseError::BadVersion(_)) && !warned_version {
                            warned_version = true;
                            status.notice(format!(
                                "! {from} speaks a different protocol version - update one side"
                            ));
                        }
                        continue;
                    }
                };

                // Nothing here authenticates a sender, and until the audio is encrypted nothing
                // can. What it can refuse to do is be taken over mid-sentence: once a phone is
                // streaming, datagrams from any other address are dropped until that stream has
                // actually stopped. Without this, one packet from anywhere on the LAN both
                // reassigns `peer` and - carrying a fresh SSRC - resets the reorder buffer, so an
                // attacker replaces a live microphone feed rather than having to wait for an idle
                // receiver. `IDLE_AFTER` is the same silence that already means "phone stopped", so
                // a phone that genuinely reconnects on a new port is back in well under a second.
                let current = status.peer();
                let streaming = last_packet.is_some_and(|t| t.elapsed() < IDLE_AFTER);
                if !accepts_from(current, from, streaming) {
                    status.rejected.fetch_add(1, Ordering::Relaxed);
                    intruders += 1;
                    // `current` is always `Some` on this branch - that is the only way to be
                    // refused. Matched rather than unwrapped anyway: this loop runs on packets an
                    // attacker chooses, and a panic here would be the denial of service that the
                    // rest of the receive path is written to avoid.
                    if let (false, Some(peer)) = (warned_intruder, current) {
                        warned_intruder = true;
                        status.notice(format!(
                            "! ignoring audio from {from} - {peer} is already sending"
                        ));
                    }
                    continue;
                }
                if current != Some(from) {
                    *lock(&status.peer) = Some(from);
                    status.connected.store(true, Ordering::Relaxed);
                    status.notice(format!(">> phone connected: {from}"));
                }
                packets_in += 1;
                bytes_in += n as u64;
                status.packets.fetch_add(1, Ordering::Relaxed);
                last_packet = Some(Instant::now());

                match header.ptype {
                    proto::TYPE_PCM_DEBUG => reorder.push(header.ssrc, header.sequence, payload),
                    proto::TYPE_KEEPALIVE => {}
                    proto::TYPE_OPUS => {
                        unsupported_type += 1;
                        if !warned_opus {
                            warned_opus = true;
                            status.notice(
                                "! the phone is sending Opus; this build only decodes raw PCM",
                            );
                        }
                    }
                    other => {
                        unsupported_type += 1;
                        if config.verbose {
                            status.notice(format!("packet type {other} not handled yet"));
                        }
                    }
                }

                drain_reorder(
                    &mut reorder,
                    &mut resampler,
                    out,
                    status,
                    &mut pcm,
                    &mut resampled,
                );
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(e) => return Err(format!("recv failed: {e}")),
        }

        if last_report.elapsed() >= Duration::from_secs(1) {
            let secs = last_report.elapsed().as_secs_f64();
            let idle = last_packet.map(|t| t.elapsed() > IDLE_AFTER).unwrap_or(true);

            status
                .pkt_per_sec
                .store((packets_in as f64 / secs).round() as u32, Ordering::Relaxed);
            status.kbps.store(
                (bytes_in as f64 * 8.0 / 1000.0 / secs).round() as u32,
                Ordering::Relaxed,
            );
            status
                .buffered_ms_x10
                .store((out.buffered_ms() * 10.0) as u32, Ordering::Relaxed);
            let s = &reorder.stats;
            status.lost.store(s.lost, Ordering::Relaxed);
            status.late.store(s.too_late, Ordering::Relaxed);
            status.duplicates.store(s.duplicates, Ordering::Relaxed);
            status.underruns.store(
                out.stats.underruns.load(Ordering::Relaxed),
                Ordering::Relaxed,
            );
            status
                .trimmed
                .store(out.stats.trimmed.load(Ordering::Relaxed), Ordering::Relaxed);

            if idle && status.connected.load(Ordering::Relaxed) {
                status.connected.store(false, Ordering::Relaxed);
                *lock(&status.peer) = None;
                // The next session gets its own warning: one stray sender an hour ago should not
                // silence the notice when it happens again to a different phone.
                warned_intruder = false;
                status.notice("... no packets for a moment (phone stopped, or Wi-Fi dropped)");
            }
            if intruders > 0 && config.verbose {
                status.notice(format!(
                    "  ({intruders} packets from an address other than the one sending)"
                ));
                intruders = 0;
            }
            if unsupported_type > 0 && config.verbose {
                status.notice(format!(
                    "  ({unsupported_type} packets of an unhandled type so far)"
                ));
            }

            packets_in = 0;
            bytes_in = 0;
            last_report = Instant::now();
        }
    }
    Ok(())
}

fn drain_reorder(
    reorder: &mut Reorder,
    resampler: &mut Resampler,
    out: &audio::Output,
    status: &Status,
    pcm: &mut Vec<f32>,
    resampled: &mut Vec<f32>,
) {
    while let Some(release) = reorder.pop() {
        match release {
            Release::Frame(frame) => {
                let src_rate = match proto::pcm_rate_from_payload(frame.len()) {
                    Some(r) => r,
                    None => {
                        status.rejected.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                };
                if resampler.rates().0 != src_rate {
                    status.src_rate.store(src_rate, Ordering::Relaxed);
                    status.notice(format!(
                        "   stream is {} kHz mono{}",
                        src_rate / 1000,
                        if src_rate == out.sample_rate {
                            String::new()
                        } else {
                            format!(" -> resampling to {} kHz", out.sample_rate / 1000)
                        }
                    ));
                    *resampler = Resampler::new(src_rate, out.sample_rate);
                }
                s16le_to_f32(&frame, pcm);
                resampled.clear();
                resampler.process(pcm, resampled);
                let pushed = out.ring.push(resampled);
                status
                    .ring_drops
                    .fetch_add((resampled.len() - pushed) as u64, Ordering::Relaxed);
            }
            Release::Lost(_) => {
                // Placeholder concealment: a frame of silence. Real packet-loss concealment needs
                // a codec that can invent a plausible frame, so it arrives with Opus.
                let frame_samples = out.sample_rate as usize * proto::FRAME_MS as usize / 1000;
                resampled.clear();
                resampled.resize(frame_samples, 0.0);
                // Counted like any other push. Concealment silence that the ring had no room for
                // is still audio that did not play, and leaving it out made the stats line
                // understate a full ring exactly when it was most worth knowing about.
                let pushed = out.ring.push(resampled);
                status
                    .ring_drops
                    .fetch_add((resampled.len() - pushed) as u64, Ordering::Relaxed);
                resampler.reset();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_are_drained_not_accumulated() {
        let s = Status::default();
        s.notice("one");
        s.notice("two");
        assert_eq!(s.take_notices(), vec!["one", "two"]);
        assert!(s.take_notices().is_empty());
    }

    #[test]
    fn a_display_that_never_drains_cannot_grow_the_queue_without_bound() {
        let s = Status::default();
        for i in 0..500 {
            s.notice(format!("{i}"));
        }
        assert_eq!(s.take_notices().len(), 64);
    }

    #[test]
    fn buffered_ms_survives_the_trip_through_an_integer() {
        let s = Status::default();
        s.buffered_ms_x10.store(725, Ordering::Relaxed);
        assert!((s.buffered_ms() - 72.5).abs() < 0.01);
    }

    /// The bug this whole ranking exists to prevent: Cloudflare WARP's tunnel address is private
    /// *and* would win the default-route probe, but the phone cannot reach it.
    #[test]
    fn a_vpn_tunnel_address_is_not_offered_to_the_phone() {
        assert!(!is_lan_candidate(
            "CloudflareWARP",
            Ipv4Addr::new(172, 16, 0, 2),
            Ipv4Addr::new(255, 255, 255, 255),
        ));
        assert!(!is_lan_candidate(
            "tailscale0",
            Ipv4Addr::new(100, 64, 0, 1),
            Ipv4Addr::new(255, 255, 255, 255),
        ));
    }

    #[test]
    fn the_wifi_address_is_offered() {
        assert!(is_lan_candidate(
            "wlan0",
            Ipv4Addr::new(192, 168, 1, 42),
            Ipv4Addr::new(255, 255, 255, 0),
        ));
    }

    /// Windows names its adapters nothing like Linux does, and the ones below all carry a private,
    /// non-`/32` address that would otherwise outrank the Wi-Fi.
    #[test]
    fn windows_virtual_adapters_are_not_offered_either() {
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        for name in [
            "VMware Network Adapter VMnet1",
            "VMware Network Adapter VMnet8",
            "VirtualBox Host-Only Network",
            "vEthernet (Default Switch)",
            "vEthernet (WSL)",
            "ZeroTier One [8056c2e21c000001]",
            "Loopback Pseudo-Interface 1",
        ] {
            assert!(
                !is_lan_candidate(name, Ipv4Addr::new(192, 168, 56, 1), mask),
                "{name} should not be offered to the phone"
            );
        }
    }

    /// The other half of that rule: the real thing on Windows is called "Wi-Fi" or "Ethernet", and
    /// the substring list must not eat either of them.
    #[test]
    fn the_real_windows_adapters_survive_the_filter() {
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        for name in ["Wi-Fi", "Ethernet", "Ethernet 2", "Wi-Fi 2"] {
            assert!(
                is_lan_candidate(name, Ipv4Addr::new(192, 168, 1, 42), mask),
                "{name} is a real network and must be offered"
            );
        }
    }

    #[test]
    fn bridges_and_loopback_are_not() {
        assert!(!is_lan_candidate(
            "virbr0",
            Ipv4Addr::new(192, 168, 122, 1),
            Ipv4Addr::new(255, 255, 255, 0),
        ));
        assert!(!is_lan_candidate(
            "docker0",
            Ipv4Addr::new(172, 17, 0, 1),
            Ipv4Addr::new(255, 255, 0, 0),
        ));
        assert!(!is_lan_candidate(
            "lo",
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(255, 0, 0, 0),
        ));
    }

    #[test]
    fn a_public_address_is_never_offered() {
        assert!(!is_lan_candidate(
            "eth0",
            Ipv4Addr::new(8, 8, 8, 8),
            Ipv4Addr::new(255, 255, 255, 0),
        ));
    }

    #[test]
    fn home_router_ranges_come_first() {
        let mut ips = [
            Ipv4Addr::new(172, 20, 1, 1),
            Ipv4Addr::new(10, 0, 0, 5),
            Ipv4Addr::new(192, 168, 1, 42),
        ];
        ips.sort_by_key(|ip| rank(*ip));
        assert_eq!(ips[0], Ipv4Addr::new(192, 168, 1, 42));
    }

    /// The address of a phone, and of somebody else on the same network.
    fn addr(last: u8, port: u16) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::new(192, 168, 1, last), port))
    }

    /// The reason this policy exists. A live stream is not interruptible by a stranger's packet:
    /// before this, one datagram from anywhere on the LAN both became the peer and reset the
    /// reorder buffer under it, so a microphone feed could be replaced rather than merely joined.
    #[test]
    fn a_stranger_cannot_cut_into_a_live_stream() {
        let phone = addr(42, 50000);
        let stranger = addr(99, 50000);
        assert!(!accepts_from(Some(phone), stranger, true));
    }

    /// The same stranger is welcome once nothing is actually being sent - that is just the next
    /// device to connect, which is a thing the user does on purpose.
    #[test]
    fn a_new_sender_is_taken_once_the_last_one_has_stopped() {
        let phone = addr(42, 50000);
        let other = addr(99, 50000);
        assert!(accepts_from(Some(phone), other, false));
        assert!(accepts_from(None, other, false));
        // Nothing has ever arrived, so nothing is being defended yet.
        assert!(accepts_from(None, other, true));
    }

    /// The phone already streaming must never be locked out by its own policy.
    #[test]
    fn the_sender_that_holds_the_stream_keeps_it() {
        let phone = addr(42, 50000);
        assert!(accepts_from(Some(phone), phone, true));
        assert!(accepts_from(Some(phone), phone, false));
    }

    /// A different source *port* on the same host is a different sender: on a phone that is a
    /// fresh socket after a reconnect, and there is no way to tell it apart from an intruder. It
    /// waits out the idle window like anyone else, which is under two seconds.
    #[test]
    fn a_new_port_on_the_same_host_is_still_a_new_sender() {
        let phone = addr(42, 50000);
        let reconnected = addr(42, 50001);
        assert!(!accepts_from(Some(phone), reconnected, true));
        assert!(accepts_from(Some(phone), reconnected, false));
    }

    #[test]
    fn a_port_already_in_use_is_reported_as_such() {
        let hog = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = hog.local_addr().expect("addr").to_string();
        let err = match Engine::start(Config {
            listen: addr,
            ..Config::default()
        }) {
            Ok(_) => panic!("binding a taken port must fail"),
            Err(e) => e,
        };
        assert!(err.contains("already in use"), "unhelpful message: {err}");
    }
}
