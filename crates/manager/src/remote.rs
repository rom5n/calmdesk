//! Widgets from other apps' packages. Their apps feed them `{app.*}` values and get their
//! clicks through `widgine::host`: `WM_COPYDATA` to the tray window, see that module.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, IsWindow, PostMessageW};

/// Values one app may keep here per package; more are not stored (they still show).
const MAX_KEYS: usize = 512;

thread_local! {
    /// (widget id, package id) of the widgets on the desktop from other apps' packages.
    static WIDGETS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    /// Last values by package, for widgets added later.
    static VALUES: RefCell<BTreeMap<String, BTreeMap<String, String>>> = const { RefCell::new(BTreeMap::new()) };
    /// Window of the app that wants each package's clicks.
    static APPS: RefCell<BTreeMap<String, isize>> = const { RefCell::new(BTreeMap::new()) };
}

/// Not one of the app's own packages, which the app feeds itself.
fn foreign(dir: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    !canon(dir).starts_with(canon(&crate::catalog::builtin_dir()))
}

/// Widget `id` of package `package` in `dir` is on the desktop now.
pub fn add(id: &str, package: &str, dir: &Path) {
    if !foreign(dir) {
        return;
    }
    WIDGETS.with_borrow_mut(|w| w.push((id.into(), package.into())));
    VALUES.with_borrow(|v| v.get(package).into_iter().flatten().for_each(|(k, val)| widgine::set(id, k, val)));
}

pub fn remove(id: &str) {
    WIDGETS.with_borrow_mut(|w| w.retain(|(i, _)| i != id));
}

/// A message from an app (tray window, UI thread). `from`: its window, for the clicks.
pub fn message(from: HWND, fields: &[String]) {
    let package = match fields {
        // Later protocol versions may add fields at the end: ignored.
        [kind, package, ..] if kind == "hello" => package,
        [kind, package, key, value, ..] if kind == "set" => {
            VALUES.with_borrow_mut(|v| {
                let keys = v.entry(package.clone()).or_default();
                if keys.len() < MAX_KEYS || keys.contains_key(key) {
                    keys.insert(key.clone(), value.clone());
                }
            });
            WIDGETS.with_borrow(|w| w.iter().filter(|(_, p)| p == package).for_each(|(id, _)| widgine::set(id, key, value)));
            package
        }
        _ => return,
    };
    if unsafe { IsWindow(Some(from)) }.as_bool() {
        APPS.with_borrow_mut(|a| a.insert(package.clone(), from.0 as isize));
    }
}

/// A click in widget `id`. False: not a widget of another app's package.
pub fn action(id: &str, action: &str, tray: HWND) -> bool {
    let Some(package) = WIDGETS.with_borrow(|w| w.iter().find(|(i, _)| i == id).map(|(_, p)| p.clone())) else { return false };
    if let Some(app) = APPS.with_borrow(|a| a.get(&package).copied()) {
        widgine::host::send(HWND(app as _), tray, &["action", &package, id, action]);
    }
    true
}

/// Tells running apps to send their values again: after this app starts.
pub fn announce() {
    unsafe {
        let _ = PostMessageW(Some(HWND_BROADCAST), widgine::host::started_message(), WPARAM(0), LPARAM(0));
    }
}
