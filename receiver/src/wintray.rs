//! The Windows notification area, by hand.
//!
//! Linux gets a tray for free: `ksni` speaks StatusNotifierItem over D-Bus and the desktop draws
//! everything. Windows has no equivalent — the notification area is a Win32 API from 1996 that
//! wants a window, a message loop and an `HICON`. Every crate that wraps it brings a GUI event
//! loop (usually `winit`) along for the ride, which is a great deal of dependency for one icon in
//! a program that has no other windows. So this is the raw thing:
//!
//! - a window that is created and never shown, purely to receive messages;
//! - `Shell_NotifyIconW` to put an icon beside the clock and get clicks back;
//! - a popup menu built fresh on every click, so it always reflects the engine;
//! - a one-second timer to refresh the icon and the tooltip.
//!
//! **None of the words are decided here.** They come from [`crate::trayui`], [`crate::cable`] and
//! [`crate::help`], all of which are ordinary cross-platform Rust with tests that run on the Linux
//! machine this was written on. What is left in this file is plumbing, and plumbing is what a
//! compiler can check. That division is deliberate: this code has no other test.
//!
//! Reasons it looks the way it does:
//!
//! - **`TPM_RETURNCMD`.** `TrackPopupMenu` normally posts `WM_COMMAND` back into the window
//!   procedure *while the menu is still up*, which means re-entering the state that built the menu.
//!   Asking for the command as a return value instead makes the whole interaction linear, and the
//!   `RefCell` below can never be borrowed twice.
//! - **A real window, not `HWND_MESSAGE`.** Message-only windows do not receive broadcasts, and
//!   `TaskbarCreated` — sent to every top-level window when Explorer restarts — is a broadcast. Miss
//!   it and the icon vanishes for the rest of the session the first time Explorer crashes.
//! - **One thread.** The engine has its own; everything here runs on the thread that made the
//!   window, because that is the only thread allowed to touch it.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{GlobalFree, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    KillTimer, LoadCursorW, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage,
    HICON, ICONINFO, IDC_ARROW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MB_YESNO,
    MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, SW_SHOWNORMAL, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY,
    WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

use crate::cable;
use crate::engine::{self, Config, Engine, LanAddress};
use crate::help;
use crate::trayui::{icon_pixels, Snapshot, State, View};
use crate::{autostart, virtualmic};

/// Our own click notification. Anything from `WM_APP` up is the application's to define.
const WM_TRAYICON: u32 = WM_APP + 1;
/// Only ever one icon, so the id is a constant rather than a counter.
const ICON_ID: u32 = 1;
const TIMER_ID: usize = 1;
/// The engine refreshes its gauges once a second; going faster only redraws the same numbers.
const REFRESH_MS: u32 = 1000;
/// How long to wait before trying again after a failed start. Long enough not to hammer the audio
/// subsystem, short enough that installing VB-Cable in another window just... starts working.
const RETRY: Duration = Duration::from_secs(5);

// Menu command ids. Zero means "nothing was chosen", so the numbering starts at one.
const ID_STATUS: usize = 1;
const ID_DETAIL: usize = 2;
const ID_CODE: usize = 3;
const ID_INPUT: usize = 4;
const ID_RENAME: usize = 5;
const ID_QUICKSTART: usize = 6;
const ID_TROUBLE: usize = 7;
const ID_CABLE: usize = 8;
const ID_STARTSTOP: usize = 9;
const ID_AUTOSTART: usize = 10;
const ID_QUIT: usize = 11;

/// A NUL-terminated UTF-16 string, which is what every `...W` entry point wants.
///
/// Windows counts UTF-16 code units, not characters, and truncating between a surrogate pair
/// produces a string the shell may refuse to draw at all. Nothing here is long enough for that to
/// bite, but [`copy_into`] is where it would.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Fills a fixed-size `szSomething` field, truncating on a character boundary and always leaving
/// room for the terminating NUL.
fn copy_into(field: &mut [u16], text: &str) {
    field.fill(0);
    let limit = field.len() - 1;
    let mut n = 0;
    for unit in text.encode_utf16() {
        if n >= limit {
            break;
        }
        field[n] = unit;
        n += 1;
    }
    // A lone leading surrogate at the end is not a character; drop it rather than emit half of one.
    if n > 0 && (0xd800..0xdc00).contains(&field[n - 1]) {
        field[n - 1] = 0;
    }
}

/// Everything the tray owns. Lives in a thread-local because a window procedure is a plain
/// `extern "system" fn` with nowhere to put a `self`.
struct App {
    config: Config,
    engine: Option<Engine>,
    error: Option<String>,
    lan: Vec<LanAddress>,
    autostart: bool,
    snap: Snapshot,
    hwnd: HWND,
    /// One icon per [`State`], created once. Creating them per refresh would leak a GDI handle a
    /// second, which takes about three hours to exhaust the desktop heap.
    icons: [HICON; 4],
    shown_state: Option<State>,
    shown_tip: String,
    /// Set once the pairing code has been announced, so the balloon appears on the run that earned
    /// it and not once a second forever.
    announced: bool,
    retry_at: Option<Instant>,
    verbose: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

impl App {
    fn view(&self) -> View<'_> {
        View {
            running: self.engine.is_some(),
            error: self.error.as_deref(),
            snap: &self.snap,
            lan: &self.lan,
        }
    }

    fn icon_for(&self, state: State) -> HICON {
        self.icons[match state {
            State::Stopped => 0,
            State::Failed => 1,
            State::Waiting => 2,
            State::Connected => 3,
        }]
    }

    fn start(&mut self) {
        if self.engine.is_some() {
            return;
        }
        match Engine::start(self.config.clone()) {
            Ok(e) => {
                self.error = None;
                self.retry_at = None;
                self.engine = Some(e);
                self.refresh();
                self.announce();
            }
            Err(e) => {
                self.error = Some(e);
                // Keep trying. The overwhelmingly common failure is "VB-Cable is not installed
                // yet", and the user is very likely installing it in another window right now.
                self.retry_at = Some(Instant::now() + RETRY);
            }
        }
    }

    fn stop(&mut self) {
        if let Some(e) = self.engine.take() {
            e.stop();
        }
        self.snap = Snapshot::default();
        self.announced = false;
        self.retry_at = None;
    }

    /// Pulls the engine's gauges across, and notices if the loop has died under us.
    fn refresh(&mut self) {
        let mut died = None;
        if let Some(engine) = &self.engine {
            // Drain regardless of verbosity: an undrained queue is a queue that fills up.
            for notice in engine.status().take_notices() {
                if self.verbose {
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
            self.snap = Snapshot::default();
            self.error = Some(e);
            self.announced = false;
            self.retry_at = Some(Instant::now() + RETRY);
        }
        if self.engine.is_some() && !self.announced {
            self.announce();
        }
        if let Some(at) = self.retry_at {
            if Instant::now() >= at {
                self.retry_at = None;
                self.start();
            }
        }
    }

    /// The balloon that makes the pairing code impossible to miss.
    ///
    /// This is the whole reason a Windows user needed a console window before: the code scrolled
    /// past in a terminal they were told to leave open, and if they looked away they had nothing to
    /// type. A notification sits there until dismissed.
    fn announce(&mut self) {
        if self.announced {
            return;
        }
        let Some(code) = self.view().pairing_code() else {
            return;
        };
        self.announced = true;
        let body = match self.view().input_line() {
            Some(input) => format!(
                "Type {} into the Earshot app on your phone.\n{input}",
                code.grouped()
            ),
            None => format!("Type {} into the Earshot app on your phone.", code.grouped()),
        };
        self.balloon("Earshot is ready", &body);
    }

    fn balloon(&self, title: &str, body: &str) {
        let mut nid = self.icon_data();
        nid.uFlags = NIF_INFO;
        copy_into(&mut nid.szInfoTitle, title);
        copy_into(&mut nid.szInfo, body);
        nid.dwInfoFlags = NIIF_INFO;
        // SAFETY: `nid` is fully initialised, `cbSize` matches, and the window is alive.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    }

    fn icon_data(&self) -> NOTIFYICONDATAW {
        // SAFETY: `NOTIFYICONDATAW` is a plain C struct of integers, arrays and handles; an
        // all-zero value is the documented starting point and every field used below is then set.
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = ICON_ID;
        nid
    }

    /// Pushes the current state into the notification area, if it has changed.
    ///
    /// The comparison is not premature optimisation: `NIM_MODIFY` with an unchanged icon still
    /// makes the shell redraw, and on some machines that visibly flickers once a second.
    fn redraw(&mut self) {
        let state = self.view().state();
        let tip = self.view().tooltip();
        if self.shown_state == Some(state) && self.shown_tip == tip {
            return;
        }
        self.shown_state = Some(state);
        self.shown_tip = tip.clone();

        let mut nid = self.icon_data();
        nid.uFlags = NIF_ICON | NIF_TIP;
        nid.hIcon = self.icon_for(state);
        copy_into(&mut nid.szTip, &tip);
        // SAFETY: as `balloon`.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    }

    /// Re-adds the icon after Explorer has restarted and taken every tray icon with it.
    fn add_icon(&mut self) {
        let state = self.view().state();
        let mut nid = self.icon_data();
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAYICON;
        nid.hIcon = self.icon_for(state);
        copy_into(&mut nid.szTip, &self.view().tooltip());
        // SAFETY: as `balloon`.
        unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
        self.shown_state = Some(state);
        self.shown_tip = self.view().tooltip();
    }

    fn remove_icon(&self) {
        let nid = self.icon_data();
        // SAFETY: as `balloon`.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &nid) };
    }

    /// The menu, rebuilt on every click so it can never show a stale state.
    ///
    /// Returns the chosen command, or 0. Building and tracking are one function because the `HMENU`
    /// must be destroyed on every path out, including the one where nothing was clicked.
    fn popup(&self) -> usize {
        let view = self.view();
        // SAFETY: every call below is on a menu handle we just created and own, with
        // NUL-terminated strings that outlive the calls.
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return 0;
            }

            let item = |flags: u32, id: usize, label: &str| {
                AppendMenuW(menu, flags, id, wide(label).as_ptr());
            };
            let sep = || AppendMenuW(menu, MF_SEPARATOR, 0, ptr::null());

            // Clickable: it is the obvious thing to click when something is wrong, and it opens
            // the box with the whole error in it rather than the one line that fits here.
            item(MF_STRING, ID_STATUS, &view.status_line());
            item(MF_STRING | MF_GRAYED, ID_DETAIL, &view.detail_line());
            sep();

            // Clickable, because clicking it copies the code — the one thing a user with a phone
            // in the other hand actually wants.
            match view.pairing_code() {
                Some(_) => item(MF_STRING, ID_CODE, &format!("{} (copy)", view.address_line())),
                None => item(MF_STRING | MF_GRAYED, ID_CODE, &view.address_line()),
            }
            if let Some(input) = view.input_line() {
                item(MF_STRING | MF_GRAYED, ID_INPUT, &input);
                item(MF_STRING, ID_RENAME, "Rename this input to Earshot...");
            }
            sep();

            if self.error.as_deref().map(cable::is_missing).unwrap_or(false) {
                item(MF_STRING, ID_CABLE, "Set up VB-Cable...");
            }
            item(MF_STRING, ID_QUICKSTART, "How do I use this?");
            item(MF_STRING, ID_TROUBLE, "It is not working...");
            sep();

            item(
                MF_STRING,
                ID_STARTSTOP,
                if self.engine.is_some() { "Stop" } else { "Start" },
            );
            item(
                MF_STRING | if self.autostart { MF_CHECKED } else { 0 },
                ID_AUTOSTART,
                "Start at login",
            );
            sep();
            item(MF_STRING, ID_QUIT, "Quit Earshot");

            let mut pos = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pos);
            // The menu will not dismiss when clicked away from unless its owner is foreground.
            SetForegroundWindow(self.hwnd);
            let chosen = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
                pos.x,
                pos.y,
                0,
                self.hwnd,
                ptr::null(),
            ) as usize;
            // The documented workaround for a menu that stays up after being dismissed.
            PostMessageW(self.hwnd, 0, 0, 0);
            DestroyMenu(menu);
            chosen
        }
    }

    fn message(&self, title: &str, body: &str, style: u32) -> i32 {
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe {
            MessageBoxW(
                self.hwnd,
                wide(body).as_ptr(),
                wide(title).as_ptr(),
                style,
            )
        }
    }

    fn copy_code(&self) {
        let Some(code) = self.view().pairing_code() else {
            return;
        };
        if copy_to_clipboard(self.hwnd, &code.to_string()) {
            self.balloon(
                "Pairing code copied",
                &format!("{} is on the clipboard.", code.grouped()),
            );
        }
    }

    /// Opens the Sound control panel on the Recording tab, having first said what to do there.
    ///
    /// `mmsys.cpl` takes the tab as its second positional argument, which is why the comma is
    /// doubled: 0 is Playback, 1 is Recording. Settings has never grown an equivalent, so this
    /// decades-old control panel is still the only place a capture device can be renamed.
    fn rename_input(&self) {
        let Some(name) = self.snap.virtual_mic.clone() else {
            return;
        };
        self.message(
            "Rename your microphone",
            &cable::rename_steps(&name),
            MB_OK | MB_ICONINFORMATION,
        );
        shell_open("control.exe", "mmsys.cpl,,1");
    }

    fn handle(&mut self, command: usize) {
        match command {
            ID_STATUS => self.show_summary(),
            ID_CODE => self.copy_code(),
            ID_RENAME => self.rename_input(),
            ID_QUICKSTART => {
                let capture = self
                    .snap
                    .virtual_mic
                    .clone()
                    .unwrap_or_else(|| "CABLE Output".to_string());
                self.message(
                    "Using Earshot",
                    &cable::quickstart(&capture),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            ID_TROUBLE => {
                let port = if self.snap.port > 0 {
                    self.snap.port
                } else {
                    engine::DEFAULT_PORT
                };
                self.message(
                    "Nothing is arriving",
                    &help::troubleshooting(port, true),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
            ID_CABLE => {
                let answer = self.message(
                    "Earshot needs a virtual audio cable",
                    &format!(
                        "{}\n\nOpen the download page now?",
                        cable::install_steps()
                    ),
                    MB_YESNO | MB_ICONINFORMATION,
                );
                if answer == IDYES {
                    if let Err(e) = cable::open_download_page() {
                        self.message("Could not open a browser", &e, MB_OK | MB_ICONERROR);
                    }
                    // The retry loop takes it from here: as soon as the cable appears, Earshot
                    // starts by itself, with no further clicking.
                    self.retry_at = Some(Instant::now() + RETRY);
                }
            }
            ID_STARTSTOP => {
                if self.engine.is_some() {
                    self.stop();
                } else {
                    self.error = None;
                    self.start();
                }
            }
            ID_AUTOSTART => {
                let extra = if self.config.virtual_mic {
                    ""
                } else {
                    "--no-virtual-mic"
                };
                let result = if self.autostart {
                    autostart::disable().map(|_| None)
                } else {
                    autostart::enable(extra).map(Some)
                };
                match result {
                    Ok(installed) => {
                        self.autostart = !self.autostart;
                        if let Some(i) = installed {
                            self.balloon(
                                "Earshot will start at login",
                                &format!("Installed to {}", i.binary.display()),
                            );
                        }
                    }
                    Err(e) => {
                        self.message("Could not change the login item", &e, MB_OK | MB_ICONERROR);
                    }
                }
            }
            ID_QUIT => {
                // SAFETY: the window is alive and owned by this thread.
                unsafe { PostQuitMessage(0) };
            }
            _ => {}
        }
    }

    /// Double-clicking the icon is the reflex for "show me the thing", so it shows the thing.
    fn show_summary(&self) {
        let title = match self.view().state() {
            State::Failed => "Earshot cannot start",
            _ => "Earshot",
        };
        let style = match self.view().state() {
            State::Failed => MB_OK | MB_ICONERROR,
            _ => MB_OK | MB_ICONINFORMATION,
        };
        self.message(title, &self.view().summary(), style);
    }
}

/// Puts text on the clipboard, which is the difference between reading nine digits off a screen and
/// having them.
///
/// Ownership of the memory passes to the system on a successful `SetClipboardData`, so it must
/// **not** be freed after that point — and must be freed on every path before it.
fn copy_to_clipboard(hwnd: HWND, text: &str) -> bool {
    let units = wide(text);
    let bytes = units.len() * std::mem::size_of::<u16>();
    // SAFETY: each call is checked before its result is used; the buffer is exactly `bytes` long
    // and is copied into under a lock that is released before the handle is handed over.
    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            return false;
        }
        let dest = GlobalLock(handle) as *mut u16;
        if dest.is_null() {
            GlobalFree(handle);
            return false;
        }
        ptr::copy_nonoverlapping(units.as_ptr(), dest, units.len());
        GlobalUnlock(handle);

        if OpenClipboard(hwnd) == 0 {
            GlobalFree(handle);
            return false;
        }
        EmptyClipboard();
        // CF_UNICODETEXT. Spelled out rather than imported: the constant lives in a different
        // module in every windows-sys generation, and 13 has been its value since Windows NT 3.1.
        const CF_UNICODETEXT: u32 = 13;
        let ok = !SetClipboardData(CF_UNICODETEXT, handle as _).is_null();
        CloseClipboard();
        if !ok {
            // Still ours, because the system did not take it.
            GlobalFree(handle);
        }
        ok
    }
}

/// `ShellExecuteW`, for the two places that need to open something Windows owns.
fn shell_open(file: &str, params: &str) {
    // SAFETY: all four strings are NUL-terminated and outlive the call.
    unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            ptr::null_mut(),
            wide("open").as_ptr(),
            wide(file).as_ptr(),
            wide(params).as_ptr(),
            ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Turns [`icon_pixels`] into something the shell can draw.
///
/// A `.ico` in the executable would want a resource compiler in the build; a themed icon name is a
/// Linux idea with no Windows equivalent. Generating a 32-bit DIB section and wrapping it in an
/// icon needs neither, and the pixels come from code that is tested on Linux.
fn make_icon(size: i32, state: State) -> HICON {
    let pixels = icon_pixels(size as usize, state);
    // SAFETY: `BITMAPINFO` is a plain C struct; zeroing it is the documented way to start, and
    // every field the call reads is set below.
    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = size;
    // Negative height means top-down, which is the order `icon_pixels` produces. Bottom-up would
    // put the microphone on its head.
    info.bmiHeader.biHeight = -size;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;

    // SAFETY: `bits` receives a buffer of exactly `size * size * 4` bytes, which is the length of
    // `pixels`; both bitmaps are deleted before returning, and the icon owns its own copies.
    unsafe {
        let mut bits: *mut c_void = ptr::null_mut();
        let colour = CreateDIBSection(
            ptr::null_mut(),
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            ptr::null_mut(),
            0,
        );
        if colour.is_null() || bits.is_null() {
            return ptr::null_mut();
        }
        ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u8, pixels.len());

        // A 32-bit icon carries its own alpha, but `CreateIconIndirect` still insists on a mask
        // bitmap of the right size. All-zero means "opaque everywhere" and lets the alpha decide.
        // Monochrome scanlines are padded to a 16-bit boundary, hence the rounding.
        let stride = (size as usize).div_ceil(16) * 2;
        let mask_bits = vec![0u8; stride * size as usize];
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr() as *const c_void);
        if mask.is_null() {
            DeleteObject(colour as _);
            return ptr::null_mut();
        }

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: colour,
        };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(colour as _);
        DeleteObject(mask as _);
        icon
    }
}

/// The registered `TaskbarCreated` message, looked up once. Explorer broadcasts it after a restart,
/// and a tray application that ignores it loses its icon until the user logs out.
///
/// An atomic rather than a `static mut` so that reading it in the window procedure needs no
/// `unsafe` and cannot trip the `static_mut_refs` lint on a newer compiler than the one here.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // Re-entering the borrow is what `TPM_RETURNCMD` exists to prevent; every arm below either
    // finishes its borrow before doing anything that pumps messages, or takes a fresh one.
    match msg {
        WM_TRAYICON => {
            let click = lp as u32;
            // Deliberately no `WM_LBUTTONDBLCLK` arm. A double-click is a down-up-dblclk-up
            // sequence, so the first up would already have opened the menu and the double-click
            // would land behind it. The menu's own top line shows the details instead.
            if click == WM_LBUTTONUP || click == WM_RBUTTONUP {
                // Build and track the menu without holding the borrow across the click handler.
                let chosen = APP.with(|a| a.borrow().as_ref().map(|app| app.popup()).unwrap_or(0));
                if chosen != 0 {
                    APP.with(|a| {
                        if let Some(app) = a.borrow_mut().as_mut() {
                            app.handle(chosen);
                        }
                    });
                }
            }
            0
        }
        WM_TIMER if wp == TIMER_ID => {
            APP.with(|a| {
                if let Some(app) = a.borrow_mut().as_mut() {
                    app.refresh();
                    app.redraw();
                }
            });
            0
        }
        WM_COMMAND => {
            // Nothing posts these — the menu returns its command instead — but a stray one must
            // not be mistaken for a click.
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            APP.with(|a| {
                if let Some(app) = a.borrow_mut().as_mut() {
                    app.add_icon();
                }
            });
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Says something to the user when there is no tray to say it through.
///
/// The tray is built with the `windows` subsystem, so it has **no console**: `println!` and
/// `eprintln!` go nowhere at all, and a program that reports a problem that way looks to the user
/// like a program that does nothing when double-clicked. Everything before the message loop starts,
/// and `--install` / `--uninstall`, come through here instead.
pub fn notify(title: &str, body: &str, bad: bool) {
    let style = MB_OK | if bad { MB_ICONERROR } else { MB_ICONINFORMATION };
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(body).as_ptr(),
            wide(title).as_ptr(),
            style,
        );
    }
}

fn fatal_box(body: &str) {
    notify("Earshot", body, true);
}

/// Runs the tray until the user quits it. Returns the process exit code.
pub fn run(config: Config, verbose: bool) -> i32 {
    let class = wide("EarshotTray");
    let title = wide("Earshot");

    // SAFETY: the class name and title outlive the window; every handle is checked before use.
    let hwnd = unsafe {
        let instance = GetModuleHandleW(ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        wc.hCursor = LoadCursorW(ptr::null_mut(), IDC_ARROW);
        if RegisterClassW(&wc) == 0 {
            fatal_box("Windows would not register Earshot's window class.");
            return 1;
        }

        TASKBAR_CREATED.store(
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            Ordering::Relaxed,
        );

        // Created and never shown. It exists to receive messages; `Shell_NotifyIcon` needs a window
        // to send clicks to, and `TrackPopupMenu` needs one to own the menu.
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            0,
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            instance,
            ptr::null(),
        );
        if hwnd.is_null() {
            fatal_box("Windows would not create Earshot's message window.");
            return 1;
        }
        hwnd
    };

    let icons = [
        make_icon(32, State::Stopped),
        make_icon(32, State::Failed),
        make_icon(32, State::Waiting),
        make_icon(32, State::Connected),
    ];

    let mut app = App {
        config,
        engine: None,
        error: None,
        lan: engine::lan_addresses(),
        autostart: autostart::is_enabled(),
        snap: Snapshot::default(),
        hwnd,
        icons,
        shown_state: None,
        shown_tip: String::new(),
        announced: false,
        retry_at: None,
        verbose,
    };
    // The icon goes up *before* the engine starts: `announce` fires a balloon, and a balloon with
    // no icon to hang off is silently discarded -- which would have thrown away the first, and
    // most useful, notification of all.
    app.add_icon();
    app.start();
    app.redraw();
    // The first failure deserves saying out loud rather than waiting to be hovered over.
    if let Some(e) = app.error.clone() {
        if cable::is_missing(&e) {
            app.balloon(
                "Earshot needs VB-Cable",
                "Windows cannot make a microphone without one. Click here for the \
                 three-minute setup.",
            );
        } else {
            app.balloon("Earshot could not start", e.lines().next().unwrap_or(&e));
        }
    }
    APP.with(|a| *a.borrow_mut() = Some(app));

    // SAFETY: `hwnd` is alive, and the message loop runs on the thread that created it.
    let code = unsafe {
        SetTimer(hwnd, TIMER_ID, REFRESH_MS, None);

        let mut msg: MSG = std::mem::zeroed();
        loop {
            let got = GetMessageW(&mut msg, ptr::null_mut(), 0, 0);
            // -1 is an error, 0 is WM_QUIT. Neither is a message to dispatch.
            if got <= 0 {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        KillTimer(hwnd, TIMER_ID);
        0
    };

    // Take the app out before tearing the window down, so nothing can be dispatched into a
    // half-destroyed state.
    let app = APP.with(|a| a.borrow_mut().take());
    if let Some(mut app) = app {
        app.remove_icon();
        app.stop();
        // The virtual microphone is left alone on purpose: on Windows it is VB-Cable's device and
        // was never ours, and on any platform applications remember the input you chose.
        virtualmic::clear_routing();
    }
    // SAFETY: created above, destroyed once, on the owning thread.
    unsafe {
        DestroyWindow(hwnd);
        for icon in icons {
            if !icon.is_null() {
                DestroyIcon(icon);
            }
        }
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_string_is_terminated() {
        assert_eq!(wide("hi"), vec![b'h' as u16, b'i' as u16, 0]);
        assert_eq!(wide(""), vec![0]);
    }

    #[test]
    fn a_field_is_filled_and_terminated() {
        let mut field = [0xffffu16; 8];
        copy_into(&mut field, "abc");
        assert_eq!(&field[..3], &[b'a' as u16, b'b' as u16, b'c' as u16]);
        assert_eq!(field[3], 0);
    }

    /// `szTip` is 128 units and tooltips can outgrow it. Truncation is Windows' problem to draw,
    /// but leaving no terminator is ours.
    #[test]
    fn an_oversized_field_still_ends_in_a_nul() {
        let mut field = [0xffffu16; 4];
        copy_into(&mut field, "abcdefgh");
        assert_eq!(field[3], 0);
        assert_eq!(&field[..3], &[b'a' as u16, b'b' as u16, b'c' as u16]);
    }

    /// Cutting a non-BMP character in half leaves a lone surrogate, which is not text and which
    /// some shell versions refuse to draw at all.
    #[test]
    fn truncation_never_leaves_half_a_character() {
        let mut field = [0xffffu16; 3];
        // Two code units each.
        copy_into(&mut field, "\u{1f600}\u{1f600}");
        assert_eq!(field[2], 0);
        for unit in field {
            assert!(!(0xd800..0xdc00).contains(&unit), "lone lead surrogate: {field:?}");
        }
    }
}
