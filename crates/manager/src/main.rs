//! Calmdesk: lives in the tray, hosts the user's widgets, and brings them back at sign-in.
//! Its gallery shows the built-in widgets and the ones other apps installed (see `catalog`):
//! click one to add it, or drag it onto the desktop.
//!
//! Run: cargo run -p calmdesk --release
//! `--background`: start without opening the gallery (sign-in). `--uninstall`: Settings > Apps.
//! `--install <file.cdwidget>`: a double-clicked widget file (see `install`).

#![windows_subsystem = "windows"]

mod catalog;
mod events;
mod feeds;
mod gallery;
mod install;
mod remix;
mod sources;
mod store;
mod log;
mod shell;
mod remote;
mod sensors;
mod timers;
mod update;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use catalog::Entry;
use widgine::{Engine, MenuItem, tr};
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
    /// The tray menu offers "What's new" (this version is new on this PC).
    static WHATS_NEW: Cell<bool> = const { Cell::new(false) };
    /// The gallery tile whose menu is about to show.
    static TILE: std::cell::RefCell<Option<Entry>> = const { std::cell::RefCell::new(None) };
    static TRAY: Cell<HWND> = const { Cell::new(HWND(std::ptr::null_mut())) };
}

/// Packages found by the background scan, for the UI thread.
static SCANNED: Mutex<Option<Vec<PathBuf>>> = Mutex::new(None);
/// The store index.
static STORE: Mutex<(Vec<store::Item>, Vec<store::Shelf>)> = Mutex::new((Vec::new(), Vec::new()));
/// The tray window, for threads that post to it.
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// From any thread: merge the scan results and the store again (WM_APP_SCANNED).
pub(crate) fn post_scanned() {
    let tray = TRAY_HWND.load(std::sync::atomic::Ordering::Relaxed);
    let _ = unsafe { PostMessageW(Some(HWND(tray as _)), shell::WM_APP_SCANNED, WPARAM(0), LPARAM(0)) };
}

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

/// The app's own setting `key` (settings.ini, `key=value` lines).
fn setting(key: &str) -> Option<String> {
    let text = std::fs::read_to_string(data_dir().join("settings.ini")).ok()?;
    text.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).map(str::to_string)
}

fn set_setting(key: &str, value: &str) {
    let path = data_dir().join("settings.ini");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with(&format!("{key}="))).map(String::from).collect();
    lines.push(format!("{key}={value}"));
    let _ = std::fs::write(path, lines.join("\n") + "\n");
}

/// Display languages the engine has texts for: code, own name.
const LANGUAGES: [(&str, &str); 12] = [
    ("en", "English"),
    ("ru", "Русский"),
    ("uk", "Українська"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("pt", "Português"),
    ("pl", "Polski"),
    ("zh", "中文"),
    ("ja", "日本語"),
    ("ko", "한국어"),
];

/// Export and import go through this folder in Documents.
fn backup_dir() -> PathBuf {
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    let docs = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None) }
        .ok()
        .and_then(|p| unsafe { p.to_string() }.ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Documents"));
    docs.join("Calmdesk backup")
}

/// The data folder's files except logs and crash reports: widgets, their settings, notes, tasks.
// ponytail: a fixed folder, no file dialog; add IFileDialog if users want to pick one.
fn copy_data(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Downloads and the profiles' own copies are not part of a backup.
        if name.ends_with(".log") || name.starts_with("crash-") || ["store", "install", "profiles"].contains(&name.as_str()) {
            continue;
        }
        let target = to.join(&name);
        if entry.path().is_dir() {
            copy_data(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Starts a new instance (which waits for this one to exit) and quits. `extra`: its arguments.
/// Profiles: `data_dir()\profiles\<name>` with the desktop's layout files. Notes, tasks and
/// settings stay shared. Rename or delete a profile by its folder.
const PROFILE_FILES: [&str; 2] = ["instances.txt", "widgets.ini"];

fn profiles_dir() -> PathBuf {
    data_dir().join("profiles")
}

/// Profile names, sorted.
fn profiles() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(profiles_dir()).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Copies the layout files from `from` to `to`; a missing file is removed at `to`.
fn copy_layout(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for f in PROFILE_FILES {
        if from.join(f).is_file() {
            std::fs::copy(from.join(f), to.join(f))?;
        } else {
            let _ = std::fs::remove_file(to.join(f));
        }
    }
    Ok(())
}

/// "Save as new profile": the current desktop as "Profile N", which becomes the current one.
fn save_new_profile() {
    let existing = profiles();
    let name = (1..).map(|n| format!("{} {n}", tr("Profile"))).find(|n| !existing.contains(n)).unwrap_or_default();
    match copy_layout(&data_dir(), &profiles_dir().join(&name)) {
        Ok(()) => set_setting("profile", &name),
        Err(e) => eprintln!("calmdesk: save profile {name}: {e}"),
    }
}

/// At start, with `--profile <name>`: keeps the desktop the last instance left in its profile,
/// then brings in `name`'s.
fn switch_profile(name: &str) {
    if let Some(current) = setting("profile").filter(|c| profiles_dir().join(c).is_dir()) {
        if let Err(e) = copy_layout(&data_dir(), &profiles_dir().join(current)) {
            eprintln!("calmdesk: save profile: {e}");
            return;
        }
    }
    match copy_layout(&profiles_dir().join(name), &data_dir()) {
        Ok(()) => set_setting("profile", name),
        Err(e) => eprintln!("calmdesk: open profile {name}: {e}"),
    }
}

fn restart(extra: &[&str]) {
    let _ = std::process::Command::new(std::env::current_exe().unwrap_or_default()).args(["--background", "--restart"]).args(extra).spawn();
    unsafe { PostQuitMessage(0) };
}

fn message(text: &str, flags: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    use windows::core::HSTRING;
    unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(shell::APP_NAME), flags) }
}

/// "Check for updates": asks GitHub on a thread, then says what it found.
fn check_for_updates() {
    std::thread::spawn(|| match update::check() {
        Some(true) => {
            if message(tr("A new version of Calmdesk is out. Open its page?"), MB_YESNO | MB_ICONINFORMATION) == IDYES {
                if let Some((_, url)) = update::available() {
                    feeds::open(&url);
                }
            }
        }
        Some(false) => {
            message(tr("Calmdesk is up to date."), MB_OK | MB_ICONINFORMATION);
        }
        None => {
            message(tr("Could not check for updates."), MB_OK | MB_ICONWARNING);
        }
    });
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
    widgine::package_info(dir).is_ok_and(|i| i.id.starts_with(MEDIA_ID))
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
    // A store widget: install it first, then add the installed one.
    let installed;
    let entry = match &entry.store {
        Some(item) => match store::get(item).and_then(|file| install::install(&file, |_, _| true)) {
            Ok(Some((info, dir))) => {
                start_scan();
                installed = Entry { dir, info, source: String::new(), kind: catalog::Kind::App, store: None };
                &installed
            }
            Ok(None) => return,
            Err(e) => {
                eprintln!("calmdesk: install {}: {e}", item.id);
                message(&format!("{}
{}

{e}", tr("Installation failed:"), entry.info.name), MB_OK | MB_ICONWARNING);
                return;
            }
        },
        None => entry,
    };
    let id = new_id(&entry.info.id);
    let opened = if drag {
        widgine::drag_out(&id, &entry.dir)
    } else {
        // Each click lands a bit lower right, so new widgets do not hide each other.
        let n = INSTANCES.with_borrow(|v| v.len()) % 6;
        let (x, y) = centered(entry.info.size, 28.0 * n as f32, 28.0 * n as f32);
        widgine::open(&id, &entry.dir, x, y)
    };
    if let Err(e) = opened {
        eprintln!("calmdesk: {}: {e}", entry.dir.display());
        return;
    }
    if entry.info.id.starts_with(MEDIA_ID) {
        feed_media(&id);
    }
    feeds::add(&id, &entry.info.id);
    sources::add(&id, &entry.info);
    remote::add(&id, &entry.info.id, &entry.dir);
    INSTANCES.with_borrow_mut(|v| v.push((id, entry.dir.clone())));
    save_instances();
}

fn open_gallery() {
    let entries = ENTRIES.with(|e| e.clone());
    let (x, y) = centered(gallery::SIZE, 0.0, 0.0);
    widgine::open_panel(gallery::ID, gallery::Gallery::new(entries, Rc::new(add)), x, y);
    // Apps may have installed widgets since the last look.
    start_scan();
}

fn start_scan() {
    std::thread::spawn(move || {
        let found = catalog::scan();
        *SCANNED.lock().unwrap() = Some(found);
        post_scanned();
        // The index may take a while to fetch: the gallery shows the other widgets meanwhile.
        *STORE.lock().unwrap() = store::refresh();
        post_scanned();
    });
}

fn scanned() {
    let Some(found) = SCANNED.lock().unwrap().clone() else { return };
    let (items, shelves) = STORE.lock().unwrap().clone();
    let entries = catalog::merge(catalog::builtins(), found, items, &shelves);
    // A store package that finished downloading changes only whether its folder exists.
    let key = |x: &Entry| (x.dir.clone(), x.dir.is_dir());
    let changed = ENTRIES.with(|e| {
        let old: Vec<_> = e.borrow().iter().map(key).collect();
        let new: Vec<_> = entries.iter().map(key).collect();
        *e.borrow_mut() = entries;
        old != new
    });
    if changed {
        // The gallery rebuilds on any data change.
        widgine::set(gallery::ID, "catalog", &format!("{:?}", std::time::Instant::now()));
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
    let lang = setting("lang").unwrap_or_default();
    let mut languages = vec![MenuItem::new(tr("System default"), "lang:").checked(lang.is_empty())];
    languages.extend(LANGUAGES.iter().map(|(code, name)| MenuItem::new(name, &format!("lang:{code}")).checked(lang == *code)));
    let current = setting("profile").unwrap_or_default();
    let mut profile_items: Vec<MenuItem> = profiles().iter().map(|p| MenuItem::new(p, &format!("profile:{p}")).checked(*p == current)).collect();
    profile_items.push(MenuItem::new(tr("Save as new profile"), "profile-new"));
    profile_items.push(MenuItem::new(tr("Open profiles folder"), "profiles"));
    items.push(MenuItem::submenu(tr("Profiles"), profile_items));
    let settings = vec![
        MenuItem::new(tr("Start with Windows"), "autostart").checked(shell::autostart()),
        MenuItem::new(tr("Hide widgets during fullscreen apps"), "fullscreen").checked(setting("fullscreen").as_deref() != Some("show")),
        MenuItem::submenu(tr("Language"), languages),
        MenuItem::new(tr("Export widgets"), "export"),
        MenuItem::new(tr("Import widgets"), "import"),
        MenuItem::new(tr("Open log folder"), "logs"),
        MenuItem::new(tr("Check for updates"), "check-update"),
    ];
    items.push(MenuItem::submenu(tr("Settings"), settings));
    if let Some((version, _)) = update::available() {
        items.push(MenuItem::new(&format!("{} ({version})", tr("Update available")), "update"));
    }
    if WHATS_NEW.get() {
        items.push(MenuItem::new(&format!("{} {}", tr("What's new in"), env!("CARGO_PKG_VERSION")), "whats-new"));
    }
    items.push(MenuItem::new(tr("Quit"), "quit"));
    items
}

/// From the gallery: shows `entry`'s menu after the gallery's event is done (menus block).
pub(crate) fn tile_menu(entry: Entry) {
    TILE.set(Some(entry));
    let _ = unsafe { PostMessageW(Some(TRAY.get()), shell::WM_APP_TILE_MENU, WPARAM(0), LPARAM(0)) };
}

fn show_tile_menu() {
    let Some(entry) = TILE.take() else { return };
    let mut pt = POINT::default();
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pt) };
    if widgine::popup_menu(pt.x, pt.y, &[MenuItem::new(tr("Remix"), "remix")]).as_deref() == Some("remix") {
        match remix::remix(&entry.dir) {
            Ok(layout) => {
                let _ = std::process::Command::new("notepad.exe").arg(&layout).spawn();
                if let Some(dir) = layout.parent() {
                    feeds::open(&dir.display().to_string());
                }
                start_scan();
            }
            Err(e) => {
                message(&format!("{}\n\n{e}", tr("Remix")), MB_OK | MB_ICONWARNING);
            }
        }
    }
}

fn request(r: shell::Request) {
    match r {
        shell::Request::TileMenu => show_tile_menu(),
        shell::Request::Gallery => open_gallery(),
        shell::Request::Scanned => scanned(),
        shell::Request::Remote(from, fields) => remote::message(from, &fields),
        shell::Request::Menu(action) => match action.as_str() {
            "gallery" => open_gallery(),
            "hide" | "show" => {
                HIDDEN.set(action == "hide");
                widgine::set_hidden(HIDDEN.get());
            }
            "remove-all" => {
                for (id, _) in INSTANCES.take() {
                    widgine::close(&id);
                    media::remove(&id);
                    feeds::remove(&id);
                    sources::remove(&id);
                    remote::remove(&id);
                }
                save_instances();
            }
            "autostart" => shell::set_autostart(!shell::autostart()),
            "fullscreen" => {
                let hide = setting("fullscreen").as_deref() == Some("show");
                set_setting("fullscreen", if hide { "hide" } else { "show" });
                widgine::set_fullscreen_hiding(hide);
            }
            "export" => {
                let dir = backup_dir();
                match copy_data(&data_dir(), &dir) {
                    Ok(()) => feeds::open(&dir.display().to_string()),
                    Err(e) => eprintln!("calmdesk: export to {}: {e}", dir.display()),
                }
            }
            // The new instance copies the backup in, after this one has saved its last state.
            "import" if backup_dir().join("instances.txt").is_file() => restart(&["--import"]),
            "import" => {
                message(&format!("{}\n{}", tr("No backup found:"), backup_dir().display()), MB_OK | MB_ICONWARNING);
            }
            "logs" => feeds::open(&data_dir().display().to_string()),
            "profile-new" => save_new_profile(),
            "profiles" => {
                let _ = std::fs::create_dir_all(profiles_dir());
                feeds::open(&profiles_dir().display().to_string());
            }
            "check-update" => check_for_updates(),
            "update" => {
                if let Some((_, url)) = update::available() {
                    feeds::open(&url);
                }
            }
            "whats-new" => {
                WHATS_NEW.set(false);
                feeds::open(concat!("https://github.com/rom5n/calmdesk/releases/tag/v", env!("CARGO_PKG_VERSION")));
            }
            "quit" => unsafe { PostQuitMessage(0) },
            a => {
                if let Some(name) = a.strip_prefix("profile:") {
                    // The new instance switches, after this one has saved its last state.
                    if Some(name) != setting("profile").as_deref() {
                        restart(&["--profile", name]);
                    }
                } else if let Some(code) = a.strip_prefix("lang:") {
                    set_setting("lang", code);
                    restart(&[]);
                }
            }
        },
    }
}

/// `--install`: asks, then installs. True when the widget was installed.
fn install_file(file: &Path) -> bool {
    let ask = |info: &widgine::PackageInfo, replacing: bool| {
        let q = if replacing { tr("Update this widget? The installed version is replaced.") } else { tr("Install this widget?") };
        message(&format!("{q}

{}", info.name), MB_YESNO | MB_ICONQUESTION) == IDYES
    };
    match install::install(file, ask) {
        Ok(done) => done.is_some(),
        Err(e) => {
            eprintln!("calmdesk: install {}: {e}", file.display());
            message(&format!("{}
{}

{e}", tr("Installation failed:"), file.display()), MB_OK | MB_ICONWARNING);
            false
        }
    }
}

fn on_action(id: &str, action: &str) {
    if action == "delete" {
        INSTANCES.with_borrow_mut(|v| v.retain(|(i, _)| i != id));
        media::remove(id);
        feeds::remove(id);
        sources::remove(id);
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
    // Before the log, which creates the data folder the old one moves to.
    let moved = migrate();
    log::start(&data_dir());
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--uninstall") {
        shell::uninstall();
        return Ok(());
    }
    // Before any text is looked up: the engine reads the language once.
    if let Some(lang) = setting("lang").filter(|l| !l.is_empty()) {
        unsafe { std::env::set_var("WIDGET_LANG", lang) };
    }
    // Then the gallery opens, in this instance or the running one, and finds the new widget.
    if let Some(file) = args.iter().position(|a| a == "--install").and_then(|i| args.get(i + 1)) {
        if !install_file(Path::new(file)) {
            return Ok(());
        }
    }
    let background = args.iter().any(|a| a == "--background");
    // One instance per data folder. Starting it again (Start menu) opens the running one's gallery.
    let name = format!("Calmdesk.SingleInstance/{}", data_dir().display()).replace('\\', "/");
    let mutex = unsafe { CreateMutexW(None, true, &windows::core::HSTRING::from(name))? };
    let running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if running && args.iter().any(|a| a == "--restart") {
        // Settings changed: the old instance is quitting. Owned (or abandoned) once it exits.
        unsafe { windows::Win32::System::Threading::WaitForSingleObject(mutex, 20_000) };
    } else if running {
        if !background {
            if let Ok(other) = unsafe { FindWindowW(shell::TRAY_CLASS, None) } {
                unsafe { PostMessageW(Some(other), shell::WM_APP_OPEN, WPARAM(0), LPARAM(0))? };
            }
        }
        return Ok(());
    }

    if moved {
        shell::migrate();
    }
    if let Some(name) = args.iter().position(|a| a == "--profile").and_then(|i| args.get(i + 1)) {
        switch_profile(name);
    }
    if args.iter().any(|a| a == "--import") {
        if let Err(e) = copy_data(&backup_dir(), &data_dir()) {
            eprintln!("calmdesk: import from {}: {e}", backup_dir().display());
        }
    }
    if setting("fullscreen").as_deref() == Some("show") {
        widgine::set_fullscreen_hiding(false);
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
                if let Ok(info) = widgine::package_info(&dir) {
                    feeds::add(&id, &info.id);
                    sources::add(&id, &info);
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
    update::start();
    // Updated since the last start: the tray offers the release notes until they are opened.
    let seen = setting("version");
    if seen.as_deref() != Some(env!("CARGO_PKG_VERSION")) {
        // ponytail: also shows after updating from 0.1, which never saved a version.
        WHATS_NEW.set(seen.is_some() || !INSTANCES.with_borrow(|v| v.is_empty()));
        set_setting("version", env!("CARGO_PKG_VERSION"));
    }

    let tray = shell::create_tray(request, tray_menu)?;
    TRAY.set(tray);
    TRAY_HWND.store(tray.0 as isize, std::sync::atomic::Ordering::Relaxed);
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
