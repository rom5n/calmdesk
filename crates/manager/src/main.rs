//! Calmdesk: lives in the tray, hosts the user's widgets, and brings them back at sign-in.
//! Its gallery shows the built-in widgets and the ones other apps installed (see `catalog`):
//! click one to add it, or drag it onto the desktop.
//!
//! Run: cargo run -p calmdesk --release
//! `--background`: start without opening the gallery (sign-in). `--uninstall`: Settings > Apps.

#![windows_subsystem = "windows"]

mod catalog;
mod feeds;
mod gallery;
mod shell;
mod remote;
mod timers;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use catalog::Entry;
use widget_engine::{Engine, MenuItem, tr};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

const MEDIA_ID: &str = "dev.widget-engine.media";

thread_local! {
    /// Everything the gallery shows, shared with it.
    static ENTRIES: Rc<RefCell<Vec<Entry>>> = Rc::new(RefCell::new(Vec::new()));
    /// Widgets on the desktop: (instance id, package dir), saved across runs.
    static INSTANCES: RefCell<Vec<(String, PathBuf)>> = const { RefCell::new(Vec::new()) };
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
    static TRAY: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
}

/// Packages found by the background scan, for the UI thread.
static SCANNED: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);

/// Settings and positions live next to the engine's (named after the exe),
/// `%LOCALAPPDATA%\calmdesk`.
pub(crate) fn data_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("calmdesk")
}

/// What the user picked for option `key` of widget `id` (the engine keeps it in widgets.ini).
// ponytail: reads the file on each call; fine for a few widgets every couple of seconds.
pub(crate) fn option(id: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(data_dir().join("widgets.ini")).ok()?;
    let pat = format!("{id}.opt.{key}=");
    text.lines().find_map(|l| l.strip_prefix(pat.as_str())).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The app was called Widget Center, and Widget Manager before that: its widgets and settings
/// move over once. True when they did.
fn migrate() -> bool {
    if data_dir().exists() {
        return false;
    }
    ["widget-center", "widget-manager"]
        .iter()
        .map(|n| data_dir().with_file_name(n))
        .find(|d| d.is_dir())
        .is_some_and(|old| std::fs::rename(old, data_dir()).is_ok())
}

fn load_instances() -> Vec<(String, PathBuf)> {
    let text = std::fs::read_to_string(data_dir().join("instances.txt")).unwrap_or_default();
    let builtin = catalog::builtin_dir();
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(id, dir)| {
            // A built-in widget from an old install folder: the same one in today's.
            let mut dir = PathBuf::from(dir);
            if let Some(name) = dir.file_name().filter(|_| !dir.is_dir()).map(|n| builtin.join(n)).filter(|d| d.is_dir()) {
                dir = name;
            }
            (id.to_string(), dir)
        })
        .collect()
}

fn save_instances() {
    let text: String = INSTANCES.with_borrow(|v| v.iter().map(|(id, dir)| format!("{id}\t{}\n", dir.display())).collect());
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(data_dir().join("instances.txt"), text);
}

fn is_media(dir: &Path) -> bool {
    widget_engine::package_info(dir).is_ok_and(|i| i.id.starts_with(MEDIA_ID))
}

/// "dev.widget-engine.clock" -> "dev-widget-engine-clock-2": the first number not in use.
fn new_id(package_id: &str) -> String {
    let base: String = package_id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let used = INSTANCES.with_borrow(|v| v.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>());
    (1..).map(|n| format!("{base}-{n}")).find(|id| !used.contains(id)).unwrap()
}

/// Primary monitor: work area in pixels and pixels per DIP.
fn primary() -> (RECT, f32) {
    let mut area = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some(&mut area as *mut RECT as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        let monitor = MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY);
        let (mut x, mut y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
        (area, x as f32 / 96.0)
    }
}

/// Top-left corner that centers `size` (DIPs) on the primary monitor, shifted by (dx, dy) DIPs.
fn centered(size: (f32, f32), dx: f32, dy: f32) -> (i32, i32) {
    let (a, s) = primary();
    let x = (a.left + a.right) / 2 - ((size.0 / 2.0 - dx) * s) as i32;
    let y = (a.top + a.bottom) / 2 - ((size.1 / 2.0 - dy) * s) as i32;
    (x, y)
}

/// Media widgets need the media session reader.
fn feed_media(id: &str) {
    media::start();
    media::add(id);
}

/// From the gallery: a new widget, following the pointer or placed in the middle of the screen.
fn add(entry: &Entry, drag: bool) {
    let id = new_id(&entry.info.id);
    let opened = if drag {
        widget_engine::drag_out(&id, &entry.dir)
    } else {
        // Each click lands a bit lower right, so new widgets do not hide each other.
        let n = INSTANCES.with_borrow(|v| v.len()) % 6;
        let (x, y) = centered(entry.info.size, 28.0 * n as f32, 28.0 * n as f32);
        widget_engine::open(&id, &entry.dir, x, y)
    };
    if let Err(e) = opened {
        eprintln!("calmdesk: {}: {e}", entry.dir.display());
        return;
    }
    if entry.info.id.starts_with(MEDIA_ID) {
        feed_media(&id);
    }
    feeds::add(&id, &entry.info.id);
    remote::add(&id, &entry.info.id, &entry.dir);
    INSTANCES.with_borrow_mut(|v| v.push((id, entry.dir.clone())));
    save_instances();
}

fn open_gallery() {
    let entries = ENTRIES.with(|e| e.clone());
    let (x, y) = centered(gallery::SIZE, 0.0, 0.0);
    widget_engine::open_panel(gallery::ID, gallery::Gallery::new(entries, Rc::new(add)), x, y);
    // Apps may have installed widgets since the last look.
    start_scan();
}

fn start_scan() {
    let tray = TRAY.get().0 as isize;
    std::thread::spawn(move || {
        let found = catalog::scan();
        *SCANNED.lock().unwrap() = Some(found);
        let _ = unsafe { PostMessageW(Some(HWND(tray as _)), shell::WM_APP_SCANNED, WPARAM(0), LPARAM(0)) };
    });
}

fn scanned() {
    let Some(found) = SCANNED.lock().unwrap().take() else { return };
    let entries = catalog::merge(catalog::builtins(), found);
    let changed = ENTRIES.with(|e| {
        let old: Vec<PathBuf> = e.borrow().iter().map(|x| x.dir.clone()).collect();
        let new: Vec<PathBuf> = entries.iter().map(|x| x.dir.clone()).collect();
        *e.borrow_mut() = entries;
        old != new
    });
    if changed {
        // The gallery rebuilds on any data change.
        widget_engine::set(gallery::ID, "catalog", &format!("{:?}", std::time::Instant::now()));
    }
}

fn tray_menu() -> Vec<MenuItem> {
    let mut items = vec![MenuItem::new(tr("Widget gallery"), "gallery")];
    if HIDDEN.get() {
        items.push(MenuItem::new(tr("Show widgets"), "show"));
    } else {
        items.push(MenuItem::new(tr("Hide widgets"), "hide"));
    }
    if INSTANCES.with_borrow(|v| !v.is_empty()) {
        items.push(MenuItem::new(tr("Remove all widgets"), "remove-all").danger());
    }
    items.push(MenuItem::new(tr("Start with Windows"), "autostart").checked(shell::autostart()));
    items.push(MenuItem::new(tr("Quit"), "quit"));
    items
}

fn request(r: shell::Request) {
    match r {
        shell::Request::Gallery => open_gallery(),
        shell::Request::Scanned => scanned(),
        shell::Request::Remote(from, fields) => remote::message(from, &fields),
        shell::Request::Menu(action) => match action.as_str() {
            "gallery" => open_gallery(),
            "hide" | "show" => {
                HIDDEN.set(action == "hide");
                widget_engine::set_hidden(HIDDEN.get());
            }
            "remove-all" => {
                for (id, _) in INSTANCES.take() {
                    widget_engine::close(&id);
                    media::remove(&id);
                    feeds::remove(&id);
                    remote::remove(&id);
                }
                save_instances();
            }
            "autostart" => shell::set_autostart(!shell::autostart()),
            "quit" => unsafe { PostQuitMessage(0) },
            _ => {}
        },
    }
}

fn on_action(id: &str, action: &str) {
    if action == "delete" {
        INSTANCES.with_borrow_mut(|v| v.retain(|(i, _)| i != id));
        media::remove(id);
        feeds::remove(id);
        remote::remove(id);
        save_instances();
        return;
    }
    // Other apps' widgets first: their "next" is not the media player's.
    if !remote::action(id, action, TRAY.get()) && !feeds::action(id, action) {
        media::action(action);
    }
}

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--uninstall") {
        shell::uninstall();
        return Ok(());
    }
    let background = args.iter().any(|a| a == "--background");
    // One instance per data folder. Starting it again (Start menu) opens the running one's gallery.
    let name = format!("Calmdesk.SingleInstance/{}", data_dir().display()).replace('\\', "/");
    let _mutex = unsafe { CreateMutexW(None, true, &windows::core::HSTRING::from(name))? };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if !background {
            if let Ok(other) = unsafe { FindWindowW(shell::TRAY_CLASS, None) } {
                unsafe { PostMessageW(Some(other), shell::WM_APP_OPEN, WPARAM(0), LPARAM(0))? };
            }
        }
        return Ok(());
    }

    if migrate() {
        shell::migrate();
    }
    let mut engine = Engine::new();
    engine.keep_alive();
    // Widgets look the way the gallery shows them: plain cards. Until the user restyles one:
    // the next widget of that kind starts with the same style.
    engine.plain_cards();
    engine.remember_styles();
    ENTRIES.with(|e| *e.borrow_mut() = catalog::builtins());

    // The widgets that were on the desktop last time. Saved positions win over (0, 0).
    let mut restored = Vec::new();
    for (id, dir) in load_instances() {
        match engine.load(&id, &dir, 0, 0) {
            Ok(()) => {
                if is_media(&dir) {
                    feed_media(&id);
                }
                if let Ok(info) = widget_engine::package_info(&dir) {
                    feeds::add(&id, &info.id);
                    remote::add(&id, &info.id, &dir);
                }
                restored.push((id, dir));
            }
            // The app that brought it was uninstalled: forget it.
            Err(e) => eprintln!("calmdesk: {}: {e}", dir.display()),
        }
    }
    INSTANCES.set(restored);
    save_instances();
    engine.on_action(on_action);
    // Screen time counts whether or not its widget is on the desktop.
    feeds::start();

    let tray = shell::create_tray(request, tray_menu)?;
    TRAY.set(tray);
    // Apps that fed their widgets before this start send their values again.
    remote::announce();
    if background {
        start_scan();
    } else {
        unsafe { PostMessageW(Some(tray), shell::WM_APP_OPEN, WPARAM(0), LPARAM(0))? };
    }
    let result = engine.run();
    shell::remove_tray(tray);
    result
}
