//! Windows integration: the tray icon and its menu, start with Windows, uninstall.

use std::path::PathBuf;

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::*;

/// Also the address apps send their widgets' values to: `widgine::host::HOST_CLASS`.
pub const TRAY_CLASS: PCWSTR = w!("Calmdesk.Tray");
/// Tray icon notifications.
const WM_TRAY: u32 = WM_APP + 50;
/// Open the gallery: from a second start of the app, or after start.
pub const WM_APP_OPEN: u32 = WM_APP + 51;
/// The background scan finished.
pub const WM_APP_SCANNED: u32 = WM_APP + 52;
/// A gallery tile was right-clicked: show its menu.
pub const WM_APP_TILE_MENU: u32 = WM_APP + 53;

pub const APP_NAME: &str = "Calmdesk";
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Calmdesk";

/// What the tray menu and window messages ask the app to do.
pub enum Request {
    Gallery,
    Scanned,
    TileMenu,
    /// The `action` of the picked tray menu item.
    Menu(String),
    /// A `widgine::host` message from another app: its window, the fields.
    Remote(HWND, Vec<String>),
}

thread_local! {
    static HANDLER: std::cell::RefCell<Option<Box<dyn Fn(Request)>>> = const { std::cell::RefCell::new(None) };
    static MENU: std::cell::RefCell<Option<Box<dyn Fn() -> Vec<widgine::MenuItem>>>> = const { std::cell::RefCell::new(None) };
}

/// Creates the hidden tray window and the icon. `menu` builds the right-click menu each time;
/// `handler` gets clicks and picks.
pub fn create_tray(handler: impl Fn(Request) + 'static, menu: impl Fn() -> Vec<widgine::MenuItem> + 'static) -> Result<HWND> {
    HANDLER.set(Some(Box::new(handler)));
    MENU.set(Some(Box::new(menu)));
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(tray_proc), hInstance: instance, lpszClassName: TRAY_CLASS, ..Default::default() });
        let hwnd = CreateWindowExW(Default::default(), TRAY_CLASS, w!("Calmdesk"), WS_POPUP, 0, 0, 0, 0, None, None, Some(instance), None)?;
        add_icon(hwnd);
        Ok(hwnd)
    }
}

fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() };
    data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    data
}

fn add_icon(hwnd: HWND) {
    unsafe {
        let instance = GetModuleHandleW(None).unwrap_or_default();
        // Resource 1 is the app icon (build.rs).
        let icon = LoadIconW(Some(instance.into()), PCWSTR(1 as _)).or_else(|_| LoadIconW(None, IDI_APPLICATION)).unwrap_or_default();
        let mut data = icon_data(hwnd);
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = icon;
        let tip: Vec<u16> = APP_NAME.encode_utf16().collect();
        data.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
        let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
    }
}

pub fn remove_tray(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &icon_data(hwnd));
        let _ = DestroyWindow(hwnd);
    }
}

fn handle(r: Request) {
    HANDLER.with_borrow(|h| {
        if let Some(h) = h {
            h(r)
        }
    });
}

extern "system" fn tray_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    static TASKBAR_CREATED: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    let taskbar_created = *TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) });
    unsafe {
        match msg {
            WM_TRAY => {
                // NOTIFYICON_VERSION_4: the event in the low word, the anchor point in wparam.
                // NIN_SELECT (click) and NIN_KEYSELECT (Enter, Space).
                const SELECT: u32 = WM_USER;
                const KEYSELECT: u32 = WM_USER + 1;
                match (lparam.0 & 0xFFFF) as u32 {
                    SELECT | KEYSELECT => handle(Request::Gallery),
                    WM_CONTEXTMENU => {
                        let (x, y) = ((wparam.0 & 0xFFFF) as i16 as i32, ((wparam.0 >> 16) & 0xFFFF) as i16 as i32);
                        let _ = SetForegroundWindow(hwnd);
                        let items = MENU.with_borrow(|m| m.as_ref().map(|f| f()).unwrap_or_default());
                        if let Some(action) = widgine::popup_menu(x, y, &items) {
                            handle(Request::Menu(action));
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_APP_OPEN => {
                handle(Request::Gallery);
                LRESULT(0)
            }
            WM_COPYDATA => match widgine::host::decode(lparam) {
                Some(fields) => {
                    handle(Request::Remote(HWND(wparam.0 as _), fields));
                    LRESULT(widgine::host::VERSION as isize)
                }
                None => LRESULT(0),
            },
            WM_APP_TILE_MENU => {
                handle(Request::TileMenu);
                LRESULT(0)
            }
            WM_APP_SCANNED => {
                handle(Request::Scanned);
                LRESULT(0)
            }
            m if m == taskbar_created => {
                add_icon(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// The command line Windows runs at sign-in.
fn run_command() -> String {
    format!("\"{}\" --background", exe().display())
}

pub fn autostart() -> bool {
    let mut size = 0u32;
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, &HSTRING::from(APP_NAME), RRF_RT_REG_SZ, None, None, Some(&mut size)) }.is_ok()
}

pub fn set_autostart(on: bool) {
    unsafe {
        if on {
            let value: Vec<u16> = run_command().encode_utf16().chain([0]).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                &HSTRING::from(APP_NAME),
                REG_SZ.0,
                Some(value.as_ptr() as *const _),
                (value.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, &HSTRING::from(APP_NAME));
        }
    }
}

/// `--uninstall`, from Settings > Apps: asks, then removes the autostart entry, the Start menu
/// shortcut, the uninstall entry, the saved widgets, and (after this process exits) the
/// install folder.
pub fn uninstall() {
    let question = HSTRING::from(widgine::tr("Uninstall Calmdesk and remove its widgets?"));
    let answer = unsafe { MessageBoxW(None, &question, &HSTRING::from(APP_NAME), MB_YESNO | MB_ICONQUESTION) };
    if answer != IDYES {
        return;
    }
    // A running instance holds the exe open.
    let me = std::process::id();
    let _ = hidden("taskkill", &["/F", "/IM", "calmdesk.exe", "/FI", &format!("PID ne {me}")]).status();
    set_autostart(false);
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(UNINSTALL_KEY));
        // The .cdwidget file type the installer registered.
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, w!(r"Software\Classes\.cdwidget"));
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, w!(r"Software\Classes\Calmdesk.Widget"));
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let _ = std::fs::remove_file(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs").join(format!("{APP_NAME}.lnk")));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        // The engine keeps each exe's data in a folder named after it.
        // And the installers' folders of the app's old name.
        for data in ["calmdesk", "calmdesk-setup", "Calmdesk Setup", "widget-setup", "Widget Center Setup"] {
            let _ = std::fs::remove_dir_all(PathBuf::from(&local).join(data));
        }
    }
    // The exe cannot delete its own folder while it runs: cmd waits for it to exit. Only the
    // installed folder: a dev build in target\release keeps its folder.
    if let Some(dir) = exe().parent().filter(|d| d.file_name().is_some_and(|n| n == APP_NAME)) {
        // raw_arg: cmd does not understand the \" escapes of a normal argument.
        use std::os::windows::process::CommandExt;
        let script = format!("/c ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"", dir.display());
        let _ = hidden("cmd", &[]).raw_arg(script).spawn();
    }
}

/// The app's old names (Widget Manager, Widget Center): their autostart entry moves to the new name.
pub fn migrate() {
    for old in ["Widget Manager", "Widget Center"] {
        let old = HSTRING::from(old);
        let mut size = 0u32;
        let had = unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, &old, RRF_RT_REG_SZ, None, None, Some(&mut size)) }.is_ok();
        if had {
            unsafe {
                let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, &old);
            }
            set_autostart(true);
        }
    }
}

/// A console command without a console window.
fn hidden(program: &str, args: &[&str]) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new(program);
    c.args(args).creation_flags(CREATE_NO_WINDOW);
    c
}
