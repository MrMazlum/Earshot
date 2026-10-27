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
use crate::clean::{self, Cleaner};
use crate::opus;
use crate::proto::{self, Header, ParseError};
use crate::reorder::{Release, Reorder};
use crate::resample::{i16_to_f32, s16le_to_f32, Resampler};

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
/// How often the phone is told its audio is arriving. The phone gives up after three seconds of
/// silence, so this has room for two lost replies before it says so.
const HELLO_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: String,
    pub buffer_ms: u32,
    pub device: Option<String>,
    pub virtual_mic: bool,
    pub virtual_mic_name: String,
    pub verbose: bool,
    /// Which noise cleaner runs on the voice after it arrives, if any.
    pub clean: clean::Choice,
    /// How hard that cleaner works. Ignored when nothing is cleaning.
    pub clean_strength: clean::Strength,
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
            // On. The phone has no cleaner of its own any more -- this is where the noise is
            // taken out, so the useful default is the one that does it. `--clean off` is a local
            // veto and beats anything the phone asks for.
            clean: clean::Choice::Dfn3,
            clean_strength: clean::Strength::default(),
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
    /// Whether the voice is being cleaned at this moment -- a model that loaded, running on a
    /// stream at a rate it can take, with the phone not refusing. Measured, never assumed.
    pub cleaning: AtomicBool,
    pub rejected: AtomicU64,
    /// Frames that were lost on the network and rebuilt from the redundant copy in the next
    /// packet. Only Opus can do this; a climbing count means the channel is dropping packets and
    /// the codec is covering for it.
    pub recovered: AtomicU64,
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
/// These are *Linux* and *macOS* device names, matched at the start, where a name is short and the
/// prefix is the whole identity of the thing (`docker0`, `wg0`, `virbr1`, `utun3`, `awdl0`).
///
/// ⚠️ macOS's `bridge0`/`bridge100` and `ap1` are deliberately **not** listed, even though a
/// Thunderbolt Bridge holding `192.168.x.1/24` would otherwise outrank the real Wi-Fi. Those same
/// names are what Internet Sharing creates — and when the phone has joined the Mac's own hotspot,
/// that interface *is* the address it can reach. The name cannot tell the two apart, so neither is
/// filtered; the receiver lists all of them and the user picks, which is the same answer it gives a
/// machine on both Ethernet and Wi-Fi.
const VIRTUAL_PREFIXES: &[&str] = &[
    "virbr",
    "docker",
    "br-",
    "veth",
    "tun",
    "tap",
    "wg",
    "tailscale",
    "zt",
    "utun",  // macOS VPN tunnels - Tailscale, WARP and WireGuard all use these
    "vmnet", // VMware Fusion
    "awdl",  // Apple Wireless Direct Link (AirDrop). Never a phone's LAN
    "llw",   // the low-latency companion to awdl0
    "anpi",  // Apple Network Processor interfaces, internal to the machine
    "stf",   // macOS 6to4 tunnel stub
    "gif",   // macOS Teredo tunnel stub
];

/// The same idea for Windows, which needs its own list and its own matching rule.
///
/// `if_addrs` reports the adapter's **friendly name** on Windows — what Network Connections shows —
/// so the interesting word is in the middle, not at the start: `VMware Network Adapter VMnet8`,
/// `vEthernet (Default Switch)`, `ZeroTier One [8056c2e21c000001]`. A prefix test sees none of
/// those, which matters because a VMware or VirtualBox host-only adapter holds a perfectly ordinary
/// `192.168.x.1/24` — private, not a `/32`, and therefore ranked *first*, ahead of the real Wi-Fi.
/// The receiver would then lead with an address on a network the phone cannot reach.
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
        port: socket
            .local_addr()
            .map(|a| a.port())
            .unwrap_or(DEFAULT_PORT),
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

/// Whether to answer this packet with a [`proto::TYPE_HELLO`].
///
/// Named and separate for the same reason as [`accepts_from`]: it is a policy, it is the only
/// thing that decides how much traffic this receiver can be made to emit, and both of those are
/// worth testing directly. Protocol §The reply is the specification.
///
/// `new_peer` sends one immediately so the phone's badge turns green on the first packet rather
/// than up to a second later. Otherwise it is one per second, which is enough for a phone that
/// treats a three-second silence as "no answer" and cheap enough to be uninteresting to an
/// attacker: the reply is 16 bytes and never larger than the packet that provoked it.
fn hello_due(since_last: Option<Duration>, new_peer: bool) -> bool {
    if new_peer {
        return true;
    }
    match since_last {
        None => true,
        Some(elapsed) => elapsed >= HELLO_EVERY,
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
    // Decoded Opus, before it becomes floats. Unused by a raw-PCM stream.
    let mut decoded = Vec::with_capacity(opus::MAX_FRAME_SAMPLES);
    // The cleaned voice, before resampling. Separate from `pcm` because the cleaner gives back
    // fewer samples than it was given until its first hop fills.
    let mut cleaned = Vec::with_capacity(2048);

    // The noise cleaner, built once. A cleaner that will not start is said out loud and then done
    // without: arriving audio that is noisy beats arriving audio that is absent, and the one thing
    // this program must never do is fail to carry the voice.
    // The last answer given to the phone about cleaning, kept so the once-a-second report can
    // show the same thing the phone was told rather than a second opinion.
    let mut cleaning_state = proto::Cleaning::default();

    let mut cleaner: Option<Cleaner> = match Cleaner::new(&config.clean, config.clean_strength) {
        Ok(built) => {
            if let Some(ref c) = built {
                status.notice(format!(
                    "   cleaning the voice with {} (+{:.0} ms)",
                    config.clean.label(),
                    c.delay_ms()
                ));
            }
            built
        }
        Err(e) => {
            status.notice(format!("! no noise cleaning: {e}"));
            None
        }
    };

    // Which of the two audio formats this stream is in, and the decoder for it. Both are per
    // stream, not per packet: a sender does not change format mid-sentence, and the decoder has to
    // be thrown away if one somehow does, because its state describes the audio it has heard.
    let mut kind: Option<Kind> = None;
    let mut decoder: Option<opus::Decoder> = None;

    let mut bytes_in = 0u64;
    let mut packets_in = 0u64;
    let mut unsupported_type = 0u64;
    let mut last_report = Instant::now();
    let mut last_packet: Option<Instant> = None;
    let mut last_hello: Option<Instant> = None;
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
                let new_peer = current != Some(from);
                if new_peer {
                    *lock(&status.peer) = Some(from);
                    status.connected.store(true, Ordering::Relaxed);
                    status.notice(format!(">> phone connected: {from}"));
                }

                // Answer the phone. This is the only traffic that leaves the receiver, and it only
                // ever goes to the address whose packet was just accepted above — a malformed or
                // refused datagram has already `continue`d and is never replied to.
                //
                // The error is dropped on purpose: a reply that cannot be sent is a nicety the
                // phone will notice for itself, and it must not interrupt audio that is arriving
                // perfectly well.
                // The phone's wish travels on every packet it sends, including a keepalive, so it
                // is read here rather than in the audio path -- a phone that is only knocking
                // still gets a truthful answer about what would happen if it streamed. It is read
                // fresh from each packet and never remembered, which is what makes it impossible
                // for a lost packet to leave the two ends disagreeing about something audible.
                let clean_now = wants_clean(proto::clean_wish(header.flags));

                if hello_due(last_hello.map(|t: Instant| t.elapsed()), new_peer) {
                    let mut reply = [0u8; proto::HEADER_LEN];
                    // What is said here is measured, never assumed: `possible` is a model that
                    // actually loaded, and `active` is that model running on a stream at a rate it
                    // can take. A badge that claimed more than it knew is a mistake already made
                    // once in this protocol.
                    cleaning_state = proto::Cleaning {
                        possible: cleaner.is_some(),
                        active: cleaner.is_some()
                            && clean_now
                            && status.src_rate.load(Ordering::Relaxed) == clean::REQUIRED_RATE,
                    };
                    proto::hello(&header, out.buffered_ms(), cleaning_state).write(&mut reply);
                    let _ = socket.send_to(&reply, from);
                    last_hello = Some(Instant::now());
                }

                packets_in += 1;
                bytes_in += n as u64;
                status.packets.fetch_add(1, Ordering::Relaxed);
                last_packet = Some(Instant::now());

                match header.ptype {
                    proto::TYPE_PCM_DEBUG | proto::TYPE_OPUS => {
                        let arriving = Kind::of(header.ptype).expect("both arms are audio types");
                        if kind != Some(arriving) {
                            // Includes the very first audio packet of a session, which is where
                            // the format gets announced.
                            kind = Some(arriving);
                            decoder = None;
                            resampler = Resampler::new(0, out.sample_rate);
                            status.notice(format!("   the phone is sending {arriving}"));
                        }
                        reorder.push(header.ssrc, header.sequence, payload);
                    }
                    proto::TYPE_KEEPALIVE => {}
                    other => {
                        unsupported_type += 1;
                        if config.verbose {
                            status.notice(format!("packet type {other} not handled yet"));
                        }
                    }
                }

                drain_reorder(
                    &mut reorder,
                    kind.unwrap_or(Kind::Pcm),
                    &mut decoder,
                    &mut resampler,
                    &mut cleaner,
                    clean_now,
                    out,
                    status,
                    &mut decoded,
                    &mut pcm,
                    &mut cleaned,
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
            let idle = last_packet
                .map(|t| t.elapsed() > IDLE_AFTER)
                .unwrap_or(true);

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
            status
                .cleaning
                .store(cleaning_state.active && !idle, Ordering::Relaxed);
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

/// Which audio format a stream is in.
///
/// Not a per-packet decision: see where it is tracked in [`pump`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Raw signed 16-bit mono. About 770 kbps, and what every build before Opus sent.
    Pcm,
    /// Compressed. About 32-40 kbps for the same voice.
    Opus,
}

impl Kind {
    /// The format a payload type carries, or `None` for the types that are not audio.
    pub fn of(ptype: u8) -> Option<Kind> {
        match ptype {
            proto::TYPE_PCM_DEBUG => Some(Kind::Pcm),
            proto::TYPE_OPUS => Some(Kind::Opus),
            _ => None,
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Kind::Pcm => f.write_str("raw audio (uncompressed, about 770 kbps)"),
            Kind::Opus => f.write_str("Opus (compressed)"),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drain_reorder(
    reorder: &mut Reorder,
    kind: Kind,
    decoder: &mut Option<opus::Decoder>,
    resampler: &mut Resampler,
    cleaner: &mut Option<Cleaner>,
    clean_now: bool,
    out: &audio::Output,
    status: &Status,
    decoded: &mut Vec<i16>,
    pcm: &mut Vec<f32>,
    cleaned: &mut Vec<f32>,
    resampled: &mut Vec<f32>,
) {
    // The sequence the next release belongs to, needed to ask the reorder buffer for the packet
    // *after* a lost one. `pop` has already moved the playhead past it by the time a gap is seen.
    while let Some(release) = reorder.pop() {
        match release {
            Release::Frame(frame) if kind == Kind::Opus => {
                let decoder = match decoder {
                    Some(d) => d,
                    None => match opus::Decoder::new(opus::DECODE_RATE) {
                        Ok(d) => decoder.insert(d),
                        Err(e) => {
                            status.notice(format!("! {e}"));
                            status.rejected.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    },
                };
                decoded.clear();
                if decoder.decode(&frame, decoded).is_err() {
                    status.rejected.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                push_samples(
                    decoded,
                    opus::DECODE_RATE,
                    resampler,
                    cleaner,
                    clean_now,
                    out,
                    status,
                    pcm,
                    cleaned,
                    resampled,
                );
            }
            Release::Lost(seq) if kind == Kind::Opus => {
                let Some(decoder) = decoder.as_mut() else {
                    // Nothing has decoded yet, so there is no history to conceal from.
                    continue;
                };
                let frame_samples = opus::DECODE_RATE as usize * proto::FRAME_MS as usize / 1000;
                decoded.clear();
                // Forward error correction first: the packet after the gap may carry a coarse copy
                // of the frame that went missing, which beats anything invented from history. Only
                // if it does not does the codec make one up.
                let next = reorder.peek(seq.wrapping_add(1)).map(|p| p.to_vec());
                let recovered = next
                    .as_deref()
                    .and_then(|next| decoder.decode_fec(next, frame_samples, decoded).ok())
                    .is_some_and(|n| n > 0);
                if recovered {
                    status.recovered.fetch_add(1, Ordering::Relaxed);
                } else {
                    decoded.clear();
                    if decoder.conceal(frame_samples, decoded).is_err() {
                        continue;
                    }
                }
                push_samples(
                    decoded,
                    opus::DECODE_RATE,
                    resampler,
                    cleaner,
                    clean_now,
                    out,
                    status,
                    pcm,
                    cleaned,
                    resampled,
                );
            }
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
                let voice = clean_in_place(cleaner, clean_now, src_rate, pcm, cleaned);
                resampled.clear();
                resampler.process(voice, resampled);
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

/// Whole samples at a known rate → the sound card's ring, resampling if the rates differ.
///
/// The Opus path's share of what the raw-PCM path does inline. It is separate because the Opus
/// path reaches it from three places — a decoded frame, a recovered one and a concealed one — and
/// all three must sound alike.
#[allow(clippy::too_many_arguments)]
fn push_samples(
    samples: &[i16],
    src_rate: u32,
    resampler: &mut Resampler,
    cleaner: &mut Option<Cleaner>,
    clean_now: bool,
    out: &audio::Output,
    status: &Status,
    pcm: &mut Vec<f32>,
    cleaned: &mut Vec<f32>,
    resampled: &mut Vec<f32>,
) {
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
    i16_to_f32(samples, pcm);
    let voice = clean_in_place(cleaner, clean_now, src_rate, pcm, cleaned);
    resampled.clear();
    resampler.process(voice, resampled);
    let pushed = out.ring.push(resampled);
    status
        .ring_drops
        .fetch_add((resampled.len() - pushed) as u64, Ordering::Relaxed);
}

/// Whether the phone's wish means cleaning should run.
///
/// An unstated wish is an older app that does not know about this, and the local setting decides —
/// which is on. A stated *no* is obeyed exactly.
fn wants_clean(wish: proto::CleanWish) -> bool {
    !matches!(wish, proto::CleanWish::No)
}

/// Runs the cleaner, if there is one and if the stream is at the rate it needs.
///
/// Returns whichever buffer the caller should go on to resample: the cleaned one, or the original
/// when nothing cleaned it. That is why this hands back a slice rather than writing in place --
/// the two buffers have different lengths, because a cleaner works in whole hops and holds back
/// what does not fill one.
///
/// ⚠️ **Cleaning happens before resampling, not after.** The models want 48 kHz, and the sound
/// card's rate is whatever the sound card says. A stream at any other rate is left alone rather
/// than resampled into the model and back out, which would cost more than the noise does.
fn clean_in_place<'a>(
    cleaner: &mut Option<Cleaner>,
    clean_now: bool,
    src_rate: u32,
    pcm: &'a [f32],
    cleaned: &'a mut Vec<f32>,
) -> &'a [f32] {
    match cleaner {
        Some(c) if clean_now && src_rate == clean::REQUIRED_RATE => {
            cleaned.clear();
            c.process(pcm, cleaned);
            cleaned
        }
        _ => pcm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The phone's badge must go green on the first packet, not on the first whole second.
    #[test]
    fn a_new_phone_is_answered_at_once() {
        assert!(hello_due(Some(Duration::from_millis(1)), true));
        assert!(hello_due(None, false));
    }

    /// The cap is the whole of the reflection argument: without it, a flood of spoofed packets
    /// becomes a flood of replies aimed at whoever was spoofed.
    #[test]
    fn a_phone_already_answered_waits_a_second() {
        assert!(!hello_due(Some(Duration::from_millis(0)), false));
        assert!(!hello_due(Some(Duration::from_millis(999)), false));
        assert!(hello_due(Some(HELLO_EVERY), false));
        assert!(hello_due(Some(Duration::from_secs(5)), false));
    }

    /// 50 packets a second arrive; at most one is answered. Written as the arithmetic it is, so a
    /// later edit that answers every packet fails here rather than on someone's network.
    #[test]
    fn a_full_second_of_audio_earns_exactly_one_reply() {
        let mut replies = 0;
        let mut since_last: Option<Duration> = None;
        for packet in 0..50u32 {
            // One 20 ms frame per packet, one sender throughout.
            if hello_due(since_last, packet == 0) {
                replies += 1;
                since_last = Some(Duration::ZERO);
            } else if let Some(d) = since_last {
                since_last = Some(d + Duration::from_millis(20));
            }
        }
        assert_eq!(replies, 1);
    }

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

    /// macOS names its internal and tunnel interfaces in ways Linux never does. `awdl0`, `llw0` and
    /// `anpi0` are on every Mac, and a private address on any of them is not the Wi-Fi the phone is
    /// on — the `utun` devices already in the list are the VPN case.
    #[test]
    fn macos_internal_interfaces_are_not_offered() {
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        for name in ["awdl0", "llw0", "anpi0", "anpi1", "stf0", "gif0", "utun3"] {
            assert!(
                !is_lan_candidate(name, Ipv4Addr::new(192, 168, 1, 42), mask),
                "{name} should not be offered to the phone"
            );
        }
    }

    /// The other half of the macOS rule, and the reason `bridge`/`ap` are **not** in the filter:
    /// those are what Internet Sharing creates, and a phone on the Mac's own hotspot reaches the Mac
    /// *through* them. Filtering by name would break the hotspot case in order to fix the Thunderbolt
    /// one, so both are offered and the user chooses.
    #[test]
    fn a_macos_hotspot_or_shared_connection_is_still_offered() {
        let mask = Ipv4Addr::new(255, 255, 255, 0);
        for name in ["en0", "bridge100", "bridge0", "ap1"] {
            assert!(
                is_lan_candidate(name, Ipv4Addr::new(192, 168, 2, 1), mask),
                "{name} may be the network the phone is on and must be offered"
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
