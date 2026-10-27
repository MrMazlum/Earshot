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

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
use earshot::engine::Config;

// Gated to the platforms that actually parse arguments, so a target with no back-end can still
// build (see the fallback `main`) without `USAGE` becoming dead code.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
const USAGE: &str = "\
earshot-tray - Earshot in the system tray, no terminal needed

USAGE:
    earshot-tray [OPTIONS]

By default it puts Earshot in the microphone list and waits for the phone. Click the tray icon
for this machine's address, to start and stop, and to turn the login item on or off.

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

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
struct Options {
    config: Config,
    install: bool,
    uninstall: bool,
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
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
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
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
// macOS
// ---------------------------------------------------------------------------------------------

/// The menu bar, over `tray-icon` and a hand-driven AppKit run loop.
///
/// Linux uses `ksni` (D-Bus) and Windows `wintray.rs` (Win32 by hand); macOS gets a crate, because
/// unlike the Win32 notification area there is a maintained one: `tray-icon` wraps `NSStatusItem`
/// and its menu. Every string still comes from [`earshot::trayui`], shared with the other
/// front-ends, so what this file adds is only the shell.
///
/// The one thing macOS adds is an **event loop**. `tray-icon` requires an AppKit run loop to be
/// running on the main thread, and the icon to be created on that same thread. Rather than drag in
/// `winit` for a program with no windows, this drives `NSApplication` itself through `objc2`: it
/// pumps the run loop half a second at a time — which is what delivers menu clicks to the handler —
/// and redraws once a second, the same shape as the Linux front-end's `while !QUIT { sleep; update }`.
#[cfg(target_os = "macos")]
mod macos {
    use earshot::engine::{self, Config, Engine, LanAddress};
    use earshot::trayui::{Snapshot, State, View};
    use earshot::{autostart, blackhole, help};

    use objc2::MainThreadMarker;
    use objc2_app_kit::{
        NSAlert, NSAlertStyle, NSApplication, NSApplicationActivationPolicy, NSPasteboard,
        NSPasteboardTypeString,
    };
    use objc2_foundation::{NSDate, NSRunLoop, NSString};

    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::Duration;

    /// Set by the Quit menu item; the loop here notices and shuts the engine down cleanly.
    pub static QUIT: AtomicBool = AtomicBool::new(false);

    /// How long each turn of the run loop is pumped before the menu is redrawn. The engine updates
    /// its gauges once a second, so anything shorter only burns CPU.
    const PUMP: Duration = Duration::from_millis(500);

    /// The icon, in pixels. 22 points is the menu-bar glyph; 44 is that at Retina density, which is
    /// what `tray-icon` scales from.
    const ICON_PX: u32 = 44;

    // Menu ids. Strings because that is what `muda` carries across the event channel; the handler
    // matches on them rather than holding a closure per item.
    const ID_VIRTUAL_MIC: &str = "virtual-mic";
    const ID_AUTOSTART: &str = "autostart";
    const ID_START_STOP: &str = "start-stop";
    const ID_GET_BLACKHOLE: &str = "get-blackhole";
    const ID_COPY: &str = "copy-address";
    const ID_QUICKSTART: &str = "quickstart";
    const ID_TROUBLE: &str = "trouble";
    const ID_QUIT: &str = "quit";

    /// The tray's state. Shared with the menu handler; both run on the main thread, inside the run
    /// loop, so the lock is never contended.
    struct Inner {
        config: Config,
        engine: Option<Engine>,
        /// Why there is no engine, when there is no engine.
        error: Option<String>,
        lan: Vec<LanAddress>,
        autostart: bool,
        snap: Snapshot,
    }

    fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    impl Inner {
        fn new(config: Config) -> Self {
            let mut inner = Inner {
                config,
                engine: None,
                error: None,
                lan: engine::lan_addresses(),
                autostart: autostart::is_enabled(),
                snap: Snapshot::default(),
            };
            inner.start();
            inner
        }

        fn running(&self) -> bool {
            self.engine.is_some()
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

        /// Pulls the engine's state across, and notices if the loop has died under us.
        fn refresh(&mut self) {
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

        fn view(&self) -> View<'_> {
            View {
                running: self.running(),
                error: self.error.as_deref(),
                snap: &self.snap,
                lan: &self.lan,
            }
        }
    }

    /// One menu item is one thing to do.
    fn click(state: &Mutex<Inner>, id: &str) {
        let mut inner = lock(state);
        match id {
            ID_VIRTUAL_MIC => {
                inner.config.virtual_mic = !inner.config.virtual_mic;
                inner.restart();
            }
            ID_AUTOSTART => {
                let extra = super::login_args(&inner.config);
                let result = if inner.autostart {
                    autostart::disable().map(|_| ())
                } else {
                    autostart::enable(extra).map(|_| ())
                };
                match result {
                    Ok(()) => inner.autostart = !inner.autostart,
                    Err(e) => inner.error = Some(e),
                }
            }
            ID_START_STOP => {
                if inner.running() {
                    inner.stop();
                } else {
                    inner.start();
                }
            }
            ID_GET_BLACKHOLE => {
                if let Err(e) = blackhole::open_download_page() {
                    inner.error = Some(e);
                }
            }
            ID_COPY => {
                if let Some((ip, _port)) = inner.view().address() {
                    copy_to_clipboard(&ip.to_string());
                }
            }
            ID_QUICKSTART => {
                // Name the device we are actually playing into; before the engine has started there
                // is none, so name the one Earshot would look for.
                let device = inner
                    .snap
                    .virtual_mic
                    .clone()
                    .unwrap_or_else(|| blackhole::KNOWN[0].device.to_string());
                message("Using Earshot", &blackhole::quickstart(&device));
            }
            ID_TROUBLE => {
                let port = if inner.snap.port > 0 {
                    inner.snap.port
                } else {
                    engine::DEFAULT_PORT
                };
                message(
                    "Nothing is arriving",
                    &help::troubleshooting(port, help::Os::Macos),
                );
            }
            ID_QUIT => QUIT.store(true, Ordering::Relaxed),
            _ => {}
        }
    }

    /// The items whose text and ticks change while the menu is alive. Kept so [`redraw`] can mutate
    /// them in place rather than rebuilding the whole menu every second.
    struct Items {
        status: MenuItem,
        detail: MenuItem,
        address: MenuItem,
        input: MenuItem,
        get_blackhole: MenuItem,
        virtual_mic: CheckMenuItem,
        autostart: CheckMenuItem,
        start_stop: MenuItem,
    }

    /// The menu, laid out like the Linux front-end's: status, what it is doing, where this machine
    /// is, which input to pick, the two toggles, start/stop, quit.
    ///
    /// There is deliberately **no "Remove the virtual microphone" item**, unlike Linux. macOS
    /// neither created BlackHole nor can remove it — `virtualmic::remove` is a no-op here — so an
    /// entry that stopped the engine and deleted nothing would be a lie, and a harmful one. Windows
    /// omits the item for the same reason.
    fn menu() -> (Menu, Items) {
        let status = MenuItem::with_id("status", "Stopped", false, None);
        let detail = MenuItem::with_id("detail", "Not running", false, None);
        // Clickable, so clicking it copies the address — the one thing a user with a phone in the
        // other hand actually wants, and the same trick the Windows tray plays. Both its text and
        // whether it can be clicked are set each second in `redraw`.
        let address = MenuItem::with_id(ID_COPY, "", false, None);
        let input = MenuItem::with_id("input", "", false, None);
        // Shown only when BlackHole is actually missing — see `redraw`. It is the tray's answer to
        // the one failure it can point at a fix for, the way the Windows tray's Help entry is.
        let get_blackhole = MenuItem::with_id(
            ID_GET_BLACKHOLE,
            "Get BlackHole (the virtual microphone)...",
            true,
            None,
        );
        // The two Help dialogs, carrying the same text the Windows tray puts in a message box
        // (`help.rs` and `blackhole.rs`). They earn their place more here than there: the tray has
        // no terminal to print any of it to, so without them a `.app` user sees an error in the
        // menu and has nowhere to go.
        let quickstart = MenuItem::with_id(ID_QUICKSTART, "How do I use this?", true, None);
        let trouble = MenuItem::with_id(ID_TROUBLE, "It is not working...", true, None);
        let virtual_mic =
            CheckMenuItem::with_id(ID_VIRTUAL_MIC, "Virtual microphone", true, true, None);
        let autostart = CheckMenuItem::with_id(ID_AUTOSTART, "Start at login", true, false, None);
        let start_stop = MenuItem::with_id(ID_START_STOP, "Start", true, None);
        let quit = MenuItem::with_id(ID_QUIT, "Quit", true, None);

        let sep1 = PredefinedMenuItem::separator();
        let sep2 = PredefinedMenuItem::separator();
        let sep3 = PredefinedMenuItem::separator();
        let sep4 = PredefinedMenuItem::separator();

        let menu = Menu::with_items(&[
            &status,
            &detail,
            &sep1,
            &address,
            &input,
            &get_blackhole,
            &sep2,
            &quickstart,
            &trouble,
            &sep3,
            &virtual_mic,
            &autostart,
            &sep4,
            &start_stop,
            &quit,
        ])
        .expect("building the menu");

        (
            menu,
            Items {
                status,
                detail,
                address,
                input,
                get_blackhole,
                virtual_mic,
                autostart,
                start_stop,
            },
        )
    }

    /// Copies the engine's state onto the menu, once a second.
    fn redraw(state: &Mutex<Inner>, items: &Items, tray: &TrayIcon) {
        let mut inner = lock(state);
        inner.refresh();
        let view = inner.view();
        let icon_state = view.state();

        items.status.set_text(view.status_line());
        items.detail.set_text(view.detail_line());
        // The address row is only clickable when there is an address to copy — a stopped engine has
        // not bound a port, so "copy" would copy the wrong thing.
        match view.address() {
            Some(_) => {
                items.address.set_text(format!("{} (copy)", view.address_line()));
                items.address.set_enabled(true);
            }
            None => {
                items.address.set_text(view.address_line());
                items.address.set_enabled(false);
            }
        }
        match view.input_line() {
            Some(text) => {
                items.input.set_text(text);
                items.input.set_enabled(true);
            }
            // Blanked rather than removed: the gap keeps the layout steady, and a greyed empty row
            // reads as "nothing to say" rather than as a missing feature.
            None => {
                items.input.set_text("");
                items.input.set_enabled(false);
            }
        }
        items.virtual_mic.set_checked(inner.config.virtual_mic);
        items.autostart.set_checked(inner.autostart);
        // The "Get BlackHole" entry is only actionable when the missing-BlackHole error is the
        // reason there is no engine. `muda` has no way to hide a row, so it greys out instead — the
        // same shape as the Windows tray's Help entry, which is always there and always enabled.
        items
            .get_blackhole
            .set_enabled(inner.error.as_deref().is_some_and(earshot::blackhole::is_missing));
        items
            .start_stop
            .set_text(if inner.running() { "Stop" } else { "Start" });

        let _ = tray.set_tooltip(Some(view.tooltip()));
        if let Ok(icon) = Icon::from_rgba(icon_rgba(icon_state), ICON_PX, ICON_PX) {
            let _ = tray.set_icon(Some(icon));
        }
    }

    /// The icon pixels, in the RGBA `tray-icon` wants.
    ///
    /// Coloured rather than a template image, so the four states are told apart the way they are on
    /// Windows and the drawing in [`earshot::trayui`] intends — grey, blue, green, red. A template
    /// image would adapt to light and dark automatically, but it would be one shape in one colour,
    /// and the state would then live only in the text.
    fn icon_rgba(state: State) -> Vec<u8> {
        earshot::trayui::icon_rgba(ICON_PX as usize, state)
    }

    /// A modal dialog — the macOS twin of the Windows tray's message box.
    ///
    /// The words are [`help`]'s and [`blackhole`]'s, the same strings Windows shows, so the two
    /// platforms cannot drift into telling people different things. `runModal` blocks until it is
    /// dismissed, which is the point: it is answering a question the user just asked.
    fn message(title: &str, body: &str) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let alert = NSAlert::new(mtm);
        alert.setAlertStyle(NSAlertStyle::Informational);
        alert.setMessageText(&NSString::from_str(title));
        alert.setInformativeText(&NSString::from_str(body));
        alert.runModal();
    }

    /// Puts a string on the general pasteboard — what makes the address row's "(copy)" true.
    fn copy_to_clipboard(text: &str) {
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        let value = NSString::from_str(text);
        // SAFETY: `NSPasteboardTypeString` is a Foundation constant that lives for the whole
        // program, and `setString:forType:` only reads from it and from `value`, both of which
        // outlive the call.
        let _ = pasteboard.setString_forType(&value, unsafe { NSPasteboardTypeString });
    }

    /// Runs the menu bar until the user quits. Returns the process exit code.
    pub fn run(config: Config) -> i32 {
        let mtm = MainThreadMarker::new().expect("the tray must run on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        // No Dock icon and no application menu bar of our own: this is a status item, which is the
        // macOS shape of "tray application".
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();

        let state = Arc::new(Mutex::new(Inner::new(config)));
        let (menu, items) = menu();

        let icon = Icon::from_rgba(icon_rgba(State::Waiting), ICON_PX, ICON_PX)
            .expect("the icon buffer is size * size * 4, which cannot fail");
        let tray = match TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip("Earshot")
            .build()
        {
            Ok(tray) => tray,
            Err(e) => {
                eprintln!("cannot create the menu-bar icon: {e}");
                return 1;
            }
        };

        // No first-run login item here, unlike the Windows tray — see the note at the top of
        // `earshot::autostart` for why. The menu bar's own "Start at login" tick is how a macOS user
        // asks, and `--install` still works from a terminal.
        //
        // Menu clicks. `set_event_handler` fires on the main thread while the run loop is pumped, so
        // it and `redraw` never touch the state at the same time.
        {
            let state = Arc::clone(&state);
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                click(&state, event.id().as_ref());
            }));
        }

        println!("Earshot is in the menu bar. Click the microphone icon.");
        {
            let inner = lock(&state);
            let view = inner.view();
            println!("  {}", view.address_line());
            if let Some(input) = view.input_line() {
                println!("  {input}");
            }
        }

        while !QUIT.load(Ordering::Relaxed) {
            // Delivers whatever is waiting — menu clicks, at least — and returns after PUMP when
            // nothing is. Without this the icon would never respond to a click.
            let until = NSDate::dateWithTimeIntervalSinceNow(PUMP.as_secs_f64());
            NSRunLoop::currentRunLoop().runUntilDate(&until);
            redraw(&state, &items, &tray);
        }

        // The engine's proper shutdown releases the UDP port straight away.
        lock(&state).stop();
        0
    }
}

#[cfg(target_os = "macos")]
fn main() {
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };

    if opts.uninstall {
        match earshot::autostart::disable() {
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
        match earshot::autostart::enable(login_args(&opts.config)) {
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

    std::process::exit(macos::run(opts.config));
}

// ---------------------------------------------------------------------------------------------
// Anything else
// ---------------------------------------------------------------------------------------------

/// A platform with no back-end still builds, and says so rather than failing to link. Linux,
/// Windows and macOS are all supported, so this is only reached on something the manifest has never
/// claimed to build — and a clear message beats a compile error in a file nobody is editing.
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
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
