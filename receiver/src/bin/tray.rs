//! `earshot-tray` — the receiver as a tray icon instead of a terminal window.
//!
//! Same engine as `earshot-receiver`, same audio path; only the front-end differs. It defaults to
//! `--virtual-mic` because that is the whole point of not having a terminal: Earshot sits in the
//! input list of Discord, OBS and Zoom, and the icon tells you whether the phone is talking.
//!
//! On GNOME this needs the AppIndicator extension, which Ubuntu enables by default. Without it the
//! process runs happily and shows nothing — see the note printed at startup.

#[cfg(target_os = "linux")]
use earshot::autostart;
#[cfg(target_os = "linux")]
use earshot::engine::{self, Config, Engine, LanAddress};
#[cfg(target_os = "linux")]
use earshot::pairing::Code;

#[cfg(target_os = "linux")]
use ksni::menu::{CheckmarkItem, StandardItem};
#[cfg(target_os = "linux")]
use ksni::{Icon, MenuItem, ToolTip, Tray, TrayService};

#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::time::Duration;

/// Set by the Quit menu item; the main thread notices and shuts the engine down properly.
#[cfg(target_os = "linux")]
static QUIT: AtomicBool = AtomicBool::new(false);

/// How often the menu and icon are refreshed. The engine updates its gauges once a second, so
/// anything faster only costs D-Bus traffic.
#[cfg(target_os = "linux")]
const REFRESH: Duration = Duration::from_secs(1);

#[cfg(target_os = "linux")]
const USAGE: &str = "\
earshot-tray - Earshot in the system tray, no terminal needed

USAGE:
    earshot-tray [OPTIONS]

By default it creates the 'Earshot' virtual microphone and waits for the phone. Left-click the
tray icon for status, to start and stop, and to turn the login item on or off.

OPTIONS:
    -l, --listen <ADDR:PORT>   where to listen        [default: 0.0.0.0:47811]
    -b, --buffer-ms <MS>       jitter buffer depth    [default: 60]
    -d, --device <NAME>        output device, substring match
        --no-virtual-mic       play out of the speakers instead of into a virtual microphone
    -v, --verbose              also log to the terminal
        --install              copy this binary to ~/.local/bin and start it at login
        --uninstall            remove the login item
    -h, --help                 this text
";

#[cfg(target_os = "linux")]
struct Options {
    config: Config,
    install: bool,
    uninstall: bool,
}

#[cfg(target_os = "linux")]
fn parse_args() -> Result<Options, String> {
    let mut opts = Options {
        config: Config {
            // The reason to run the tray at all: be a microphone, not a speaker.
            virtual_mic: true,
            ..Config::default()
        },
        install: false,
        uninstall: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-l" | "--listen" => opts.config.listen = value("--listen")?,
            "-b" | "--buffer-ms" => {
                let v = value("--buffer-ms")?;
                opts.config.buffer_ms = v
                    .parse()
                    .map_err(|_| format!("--buffer-ms wants a number, got '{v}'"))?;
            }
            "-d" | "--device" => opts.config.device = Some(value("--device")?),
            "--no-virtual-mic" => opts.config.virtual_mic = false,
            "-v" | "--verbose" => opts.config.verbose = true,
            "--install" => opts.install = true,
            "--uninstall" => opts.uninstall = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option '{other}'\n\n{USAGE}")),
        }
    }
    if opts.config.listen.parse::<std::net::SocketAddr>().is_err()
        && !opts.config.listen.contains(':')
    {
        opts.config.listen = format!("0.0.0.0:{}", opts.config.listen);
    }
    Ok(opts)
}

/// What the menu shows. Copied out of the engine once a second so rendering never touches atomics
/// in a half-updated state.
#[cfg(target_os = "linux")]
#[derive(Default, Clone)]
struct Snapshot {
    connected: bool,
    peer: Option<String>,
    pkt_per_sec: u32,
    lost: u64,
    late: u64,
    underruns: u64,
    buffered_ms: f32,
    src_rate: u32,
    port: u16,
    out_device: String,
    virtual_mic: Option<String>,
}

#[cfg(target_os = "linux")]
struct EarshotTray {
    config: Config,
    engine: Option<Engine>,
    /// Why there is no engine, when there is no engine.
    error: Option<String>,
    lan: Vec<LanAddress>,
    autostart: bool,
    snap: Snapshot,
}

#[cfg(target_os = "linux")]
impl EarshotTray {
    fn new(config: Config) -> Self {
        let mut tray = EarshotTray {
            config,
            engine: None,
            error: None,
            lan: engine::lan_addresses(),
            autostart: autostart::is_enabled(),
            snap: Snapshot::default(),
        };
        tray.start();
        tray
    }

    fn start(&mut self) {
        if self.engine.is_some() {
            return;
        }
        match Engine::start(self.config.clone()) {
            Ok(e) => {
                self.error = None;
                self.engine = Some(e);
            }
            Err(e) => {
                if self.config.verbose {
                    eprintln!("cannot start: {e}");
                }
                self.error = Some(e);
            }
        }
        self.refresh();
    }

    fn stop(&mut self) {
        if let Some(e) = self.engine.take() {
            e.stop();
        }
        self.snap = Snapshot::default();
    }

    fn restart(&mut self) {
        self.stop();
        self.start();
    }

    /// Pulls the engine's state into [`Snapshot`], and notices if the loop has died under us.
    fn refresh(&mut self) {
        let mut died = None;
        if let Some(engine) = &self.engine {
            let st = engine.status();
            let ready = engine.ready();
            // Drain regardless of verbosity: an undrained queue is a queue that fills up.
            for notice in st.take_notices() {
                if self.config.verbose {
                    eprintln!("{notice}");
                }
            }
            self.snap = Snapshot {
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
            };
            if !engine.is_running() {
                died = Some(
                    st.fatal()
                        .unwrap_or_else(|| "the receiver stopped unexpectedly".to_string()),
                );
            }
        }
        if let Some(e) = died {
            self.engine = None;
            self.error = Some(e);
            self.snap = Snapshot::default();
        }
    }

    fn status_line(&self) -> String {
        if let Some(e) = &self.error {
            // Menu labels are one line; the full text is in the tooltip.
            return format!("⚠ {}", e.lines().next().unwrap_or("failed to start"));
        }
        if self.engine.is_none() {
            return "Stopped".to_string();
        }
        match (&self.snap.connected, &self.snap.peer) {
            (true, Some(ip)) => format!("● Connected — {ip}"),
            (true, None) => "● Connected".to_string(),
            _ => "○ Waiting for the phone".to_string(),
        }
    }

    fn detail_line(&self) -> String {
        if self.engine.is_none() {
            return "Not running".to_string();
        }
        if !self.snap.connected {
            return match &self.snap.virtual_mic {
                Some(name) => format!("Ready — pick “{name}” as your input"),
                None => format!("Playing to {}", self.snap.out_device),
            };
        }
        let rate = if self.snap.src_rate > 0 {
            format!("{} kHz", self.snap.src_rate / 1000)
        } else {
            "—".to_string()
        };
        format!(
            "{rate} · {} lost · {:.0} ms buffered",
            self.snap.lost, self.snap.buffered_ms
        )
    }

    /// What to type into the phone. The pairing code when there is one — it is one field instead
    /// of two, and it keeps the network layout off the screen when the menu is open in a
    /// screenshot or a stream.
    fn address_line(&self) -> String {
        let Some(a) = self.lan.first() else {
            return "This PC: address unknown".to_string();
        };
        // With the engine stopped the port is not settled yet, so no code can be honest about it.
        if self.engine.is_none() {
            return format!("This PC: {}", a.ip);
        }
        match Code::new(a.ip, self.snap.port) {
            Some(code) => format!("Pairing code: {}", code.grouped()),
            None => format!("This PC: {} · port {}", a.ip, self.snap.port),
        }
    }

    /// Only worth saying when the machine is on more than one network — then the first guess may
    /// well be the wrong one.
    fn other_addresses(&self) -> Option<String> {
        if self.lan.len() < 2 {
            return None;
        }
        let others: Vec<String> = self.lan[1..].iter().map(|a| a.to_string()).collect();
        Some(format!("also on: {}", others.join(", ")))
    }
}

#[cfg(target_os = "linux")]
impl Tray for EarshotTray {
    fn id(&self) -> String {
        "earshot".into()
    }

    fn title(&self) -> String {
        "Earshot".into()
    }

    /// Symbolic names from Adwaita, which is always installed and is the theme's fallback. Symbolic
    /// icons get recoloured by the shell, so this stays legible on a light *or* dark panel — which
    /// a bitmap of our own would not.
    fn icon_name(&self) -> String {
        if self.engine.is_none() {
            "microphone-disabled-symbolic".into()
        } else if self.snap.connected {
            "microphone-sensitivity-high-symbolic".into()
        } else {
            "audio-input-microphone-symbolic".into()
        }
    }

    /// Deliberately empty: with no pixmap, the shell must use `icon_name` and thus the theme.
    fn icon_pixmap(&self) -> Vec<Icon> {
        Vec::new()
    }

    fn tool_tip(&self) -> ToolTip {
        let description = match &self.error {
            Some(e) => e.clone(),
            None if self.snap.connected => format!(
                "{}\n{} pkt/s · {} late · {} underruns\n{}",
                self.detail_line(),
                self.snap.pkt_per_sec,
                self.snap.late,
                self.snap.underruns,
                self.address_line()
            ),
            None => format!("{}\n{}", self.detail_line(), self.address_line()),
        };
        let description = match self.other_addresses() {
            Some(more) => format!("{description}\n{more}"),
            None => description,
        };
        ToolTip {
            title: format!("Earshot — {}", self.status_line()),
            description,
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let running = self.engine.is_some();
        let can_remove_mic = self.config.virtual_mic;

        vec![
            StandardItem {
                label: self.status_line(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: self.detail_line(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: self.address_line(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Virtual microphone".into(),
                checked: self.config.virtual_mic,
                activate: Box::new(|this: &mut Self| {
                    this.config.virtual_mic = !this.config.virtual_mic;
                    this.restart();
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "Start at login".into(),
                checked: self.autostart,
                activate: Box::new(|this: &mut Self| {
                    let extra = if this.config.virtual_mic {
                        ""
                    } else {
                        "--no-virtual-mic"
                    };
                    let result = if this.autostart {
                        autostart::disable().map(|_| ())
                    } else {
                        autostart::enable(extra).map(|_| ())
                    };
                    match result {
                        Ok(()) => this.autostart = !this.autostart,
                        Err(e) => this.error = Some(e),
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: if running { "Stop" } else { "Start" }.into(),
                activate: Box::new(move |this: &mut Self| {
                    if this.engine.is_some() {
                        this.stop();
                    } else {
                        this.start();
                    }
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Remove the virtual microphone".into(),
                // Only meaningful when we are the ones who put it there.
                visible: can_remove_mic,
                activate: Box::new(|this: &mut Self| {
                    this.stop();
                    if let Err(e) = earshot::virtualmic::remove(&this.config.virtual_mic_name) {
                        this.error = Some(e);
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|_this: &mut Self| {
                    // The main thread owns shutdown, so the socket is released before we exit.
                    QUIT.store(true, Ordering::Relaxed);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Windows and macOS get the binary but not the icon: `ksni` is Linux-only, and a tray that
/// silently does nothing would be worse than one that says so.
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!(
        "earshot-tray is Linux-only for now.\n\n\
         On this platform use the receiver directly:\n\
         \n    earshot-receiver --virtual-mic\n"
    );
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    if opts.uninstall {
        match autostart::disable() {
            Ok(true) => println!("Earshot will no longer start at login."),
            Ok(false) => println!("Earshot was not set to start at login."),
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    if opts.install {
        let extra = if opts.config.virtual_mic {
            ""
        } else {
            "--no-virtual-mic"
        };
        match autostart::enable(extra) {
            Ok((bin, desktop)) => {
                println!("Installed.");
                println!("  binary      {}", bin.display());
                println!("  login item  {}", desktop.display());
                println!("\nEarshot will start with your next login. Starting it now too...");
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }

    let verbose = opts.config.verbose;
    let tray = EarshotTray::new(opts.config);
    if let Some(e) = &tray.error {
        eprintln!("warning: {e}");
    }

    println!("Earshot is in the system tray. Click the microphone icon.");
    println!("If you cannot see it, GNOME needs its AppIndicator extension (Ubuntu ships it on).");
    if let Some(a) = tray.lan.first() {
        match Code::new(a.ip, tray.snap.port) {
            Some(code) => println!("Pairing code for the phone app:   {}", code.grouped()),
            None => println!("In the phone app, type:   {}   port {}", a.ip, tray.snap.port),
        }
    }

    let service = TrayService::new(tray);
    let handle = service.handle();
    service.spawn();

    while !QUIT.load(Ordering::Relaxed) {
        std::thread::sleep(REFRESH);
        handle.update(|t: &mut EarshotTray| t.refresh());
    }

    // Give the engine its proper shutdown: this releases the UDP port straight away. The virtual
    // microphone is left loaded on purpose — applications remember the input you chose.
    handle.update(|t: &mut EarshotTray| t.stop());
    if verbose {
        eprintln!("quit");
    }
}
