//! `earshot-tray` — the receiver as a tray icon instead of a terminal window.
//!
//! Same engine as `earshot-receiver`, same audio path; only the front-end differs. It defaults to
//! `--virtual-mic` because that is the whole point of not having a terminal: Earshot sits in the
//! input list of Discord, OBS and Zoom, and the icon tells you whether the phone is talking.
//!
//! Two back-ends, because the platforms have nothing in common here:
//!
//! - **Linux** — `ksni`, which speaks StatusNotifierItem over D-Bus. On GNOME this needs the
//!   AppIndicator extension, which Ubuntu enables by default; without it the process runs happily
//!   and shows nothing, so it says so at startup.
//! - **Windows** — [`earshot::wintray`], which is Win32 by hand. It is built as a GUI application,
//!   so double-clicking it opens no console at all.
//!
//! Neither back-end decides what any of it *says*: that is [`earshot::trayui`], which is ordinary
//! cross-platform code with tests.

// No console window on Windows. The whole complaint this answers is "there is a terminal sitting
// there and I might close it or miss what it printed" — leaving a console attached would keep that
// exactly as it was. Debug builds keep theirs, because a panic with nowhere to print is no fun.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(any(target_os = "linux", target_os = "windows"))]
use earshot::engine::Config;

const USAGE: &str = "\
earshot-tray - Earshot in the system tray, no terminal needed

USAGE:
    earshot-tray [OPTIONS]

By default it puts Earshot in the microphone list and waits for the phone. Click the tray icon
for the pairing code, to start and stop, and to turn the login item on or off.

OPTIONS:
    -l, --listen <ADDR:PORT>   where to listen        [default: 0.0.0.0:47811]
    -b, --buffer-ms <MS>       jitter buffer depth    [default: 60]
    -d, --device <NAME>        output device, substring match
        --no-virtual-mic       play out of the speakers instead of into a virtual microphone
    -v, --verbose              also log to the terminal
        --install              install a copy and start it at login
        --uninstall            remove the login item
    -h, --help                 this text
";

#[cfg(any(target_os = "linux", target_os = "windows"))]
struct Options {
    config: Config,
    install: bool,
    uninstall: bool,
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
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

/// The extra arguments a login item needs to reproduce this run's configuration.
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn login_args(config: &Config) -> &'static str {
    if config.virtual_mic {
        ""
    } else {
        "--no-virtual-mic"
    }
}

// ---------------------------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------------------------

/// On Windows there is no console to print to, so `--install` and `--uninstall` report through the
/// same message boxes as everything else. They are rare enough that a dialog is the right weight.
#[cfg(target_os = "windows")]
fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            earshot::wintray::notify("Earshot", &e, true);
            std::process::exit(2);
        }
    };

    if opts.uninstall {
        let (text, bad) = match earshot::autostart::disable() {
            Ok(true) => ("Earshot will no longer start at login.".to_string(), false),
            Ok(false) => ("Earshot was not set to start at login.".to_string(), false),
            Err(e) => (e, true),
        };
        earshot::wintray::notify("Start at login", &text, bad);
        std::process::exit(i32::from(bad));
    }

    if opts.install {
        match earshot::autostart::enable(login_args(&opts.config)) {
            Ok(i) => earshot::wintray::notify(
                "Start at login",
                &format!(
                    "Earshot will start with your next login.\n\n\
                     Installed to:\n{}\n\n\
                     Login item:\n{}\n\n\
                     Starting it now as well.",
                    i.binary.display(),
                    i.entry
                ),
                false,
            ),
            Err(e) => {
                earshot::wintray::notify("Could not install", &e, true);
                std::process::exit(1);
            }
        }
    }

    let verbose = opts.config.verbose;
    std::process::exit(earshot::wintray::run(opts.config, verbose));
}

// ---------------------------------------------------------------------------------------------
// Anything that is neither
// ---------------------------------------------------------------------------------------------

/// macOS gets the binary but not the icon: neither back-end exists there, and a tray that silently
/// does nothing would be worse than one that says so.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn main() {
    eprintln!(
        "earshot-tray has no back-end on this platform yet.\n\n\
         Use the receiver directly:\n\
         \n    earshot-receiver --virtual-mic\n"
    );
    std::process::exit(1);
}

// ---------------------------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use earshot::autostart;
    use earshot::engine::{self, Config, Engine, LanAddress};
    use earshot::trayui::{Snapshot, State, View};

    use ksni::menu::{CheckmarkItem, StandardItem};
    use ksni::{Icon, MenuItem, ToolTip, Tray, TrayService};

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// Set by the Quit menu item; the main thread notices and shuts the engine down properly.
    pub static QUIT: AtomicBool = AtomicBool::new(false);

    /// How often the menu and icon are refreshed. The engine updates its gauges once a second, so
    /// anything faster only costs D-Bus traffic.
    pub const REFRESH: Duration = Duration::from_secs(1);

    pub struct EarshotTray {
        pub config: Config,
        engine: Option<Engine>,
        /// Why there is no engine, when there is no engine.
        pub error: Option<String>,
        pub lan: Vec<LanAddress>,
        autostart: bool,
        pub snap: Snapshot,
    }

    impl EarshotTray {
        pub fn new(config: Config) -> Self {
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

        pub fn view(&self) -> View<'_> {
            View {
                running: self.engine.is_some(),
                error: self.error.as_deref(),
                snap: &self.snap,
                lan: &self.lan,
            }
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

        pub fn stop(&mut self) {
            if let Some(e) = self.engine.take() {
                e.stop();
            }
            self.snap = Snapshot::default();
        }

        fn restart(&mut self) {
            self.stop();
            self.start();
        }

        /// Pulls the engine's state across, and notices if the loop has died under us.
        pub fn refresh(&mut self) {
            let mut died = None;
            if let Some(engine) = &self.engine {
                // Drain regardless of verbosity: an undrained queue is a queue that fills up.
                for notice in engine.status().take_notices() {
                    if self.config.verbose {
                        eprintln!("{notice}");
                    }
                }
                self.snap = Snapshot::read(engine);
                if !engine.is_running() {
                    died = Some(
                        engine
                            .status()
                            .fatal()
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
    }

    impl Tray for EarshotTray {
        fn id(&self) -> String {
            "earshot".into()
        }

        fn title(&self) -> String {
            "Earshot".into()
        }

        /// Symbolic names from Adwaita, which is always installed and is the theme's fallback.
        /// Symbolic icons get recoloured by the shell, so this stays legible on a light *or* dark
        /// panel — which a bitmap of our own would not. Windows has no such thing and draws
        /// `trayui::icon_pixels` instead.
        fn icon_name(&self) -> String {
            match self.view().state() {
                State::Stopped | State::Failed => "microphone-disabled-symbolic".into(),
                State::Waiting => "audio-input-microphone-symbolic".into(),
                State::Connected => "microphone-sensitivity-high-symbolic".into(),
            }
        }

        /// Deliberately empty: with no pixmap, the shell must use `icon_name` and thus the theme.
        fn icon_pixmap(&self) -> Vec<Icon> {
            Vec::new()
        }

        fn tool_tip(&self) -> ToolTip {
            let view = self.view();
            let full = view.tooltip();
            // `View::tooltip` leads with the same line the title carries, so it is not repeated.
            let description = full.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
            ToolTip {
                title: format!("Earshot — {}", view.status_line()),
                description: description.to_string(),
                ..Default::default()
            }
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let view = self.view();
            let running = self.engine.is_some();
            let can_remove_mic = self.config.virtual_mic;

            let mut items: Vec<MenuItem<Self>> = vec![
                StandardItem {
                    label: view.status_line(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: view.detail_line(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: view.address_line(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            ];

            // What to pick in Discord. Obvious on Linux, where we named the device ourselves; the
            // line earns its place on Windows, and costs nothing here.
            if let Some(input) = view.input_line() {
                items.push(
                    StandardItem {
                        label: input,
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
            }

            items.extend([
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
                        let extra = super::login_args(&this.config);
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
            ]);

            items
        }
    }

    pub fn service(tray: EarshotTray) -> TrayService<EarshotTray> {
        TrayService::new(tray)
    }
}

#[cfg(target_os = "linux")]
fn main() {
    use earshot::autostart;
    use linux::{service, EarshotTray, QUIT, REFRESH};
    use std::sync::atomic::Ordering;

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
        match autostart::enable(login_args(&opts.config)) {
            Ok(i) => {
                println!("Installed.");
                println!("  binary      {}", i.binary.display());
                println!("  login item  {}", i.entry);
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
    println!("  {}", tray.view().address_line());
    if let Some(input) = tray.view().input_line() {
        println!("  {input}");
    }

    let service = service(tray);
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
