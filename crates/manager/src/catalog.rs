//! Which widgets exist on this PC: the built-in ones shipped with the manager, and packages
//! other apps installed.
//!
//! Apps make their widgets known in one of these ways:
//! - a value under `HKCU` or `HKLM\Software\Widgine\Packages` (or the older
//!   `Software\WidgetEngine\Packages`), any name, whose data is a
//!   package directory, or a directory of packages;
//! - a package directory in `%LOCALAPPDATA%\Widgets` or `%ProgramData%\Widgets`;
//! - a package anywhere in the app's install folder, up to a few levels deep, under Program
//!   Files, `%LOCALAPPDATA%\Programs`, or `%APPDATA%`.
//!
//! A package is a directory with `widget.toml` and `layout.xml` that the engine accepts.

use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use widgine::{PackageInfo, package_info};
use windows::Win32::System::Registry::*;

#[derive(Clone, Debug)]
pub struct Entry {
    pub dir: PathBuf,
    pub info: PackageInfo,
    /// Under the name on its tile: "Built-in", the app's folder name, or a store widget's author.
    pub source: String,
    pub kind: Kind,
    /// Store widgets: the .cdwidget to install when the user picks it.
    pub store: Option<PathBuf>,
}

/// The gallery groups widgets by where they come from, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Builtin,
    App,
    Community,
}

impl Entry {
    /// Gallery section, in the display language.
    pub fn category(&self) -> String {
        match self.info.category.as_str() {
            "" => widgine::tr("Other").into(),
            c => widgine::tr(c).into(),
        }
    }
}

/// Built-in widgets: `widgets` next to the exe when installed, the repository's when run from
/// `target`.
pub fn builtin_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let installed = exe.parent().unwrap_or(Path::new(".")).join("widgets");
    if installed.is_dir() {
        return installed;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../widgets")
}

/// Shipped with the app and fed by it: every package in `builtin_dir`, by folder name.
pub fn builtins() -> Vec<Entry> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(builtin_dir()).into_iter().flatten().flatten().map(|e| e.path()).collect();
    dirs.sort();
    let mut all: Vec<Entry> = dirs
        .into_iter()
        .filter(|d| d.join("widget.toml").is_file())
        .filter_map(|dir| {
            let info = package_info(&dir).map_err(|e| eprintln!("calmdesk: {e}")).ok()?;
            Some(Entry { dir, info, source: widgine::tr("Built-in").into(), kind: Kind::Builtin, store: None })
        })
        .collect();
    // Like `merge`: by name, variants of one widget in folder order.
    all.sort_by_key(|e| e.info.name.to_lowercase());
    all
}

/// Packages of other apps. Slow (walks install folders): call it off the UI thread.
// ponytail: walks the folders on every gallery start; cache by folder time if it gets slow.
pub fn scan() -> Vec<PathBuf> {
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for key in [windows::core::w!(r"Software\Widgine\Packages"), windows::core::w!(r"Software\WidgetEngine\Packages")] {
            roots.extend(registered(hive, key).into_iter().map(|d| (d, 1)));
        }
    }
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    for (var, sub, depth) in [
        ("LOCALAPPDATA", "Widgets", 2),
        ("ProgramData", "Widgets", 2),
        ("ProgramFiles", "", 4),
        ("ProgramFiles(x86)", "", 4),
        ("LOCALAPPDATA", "Programs", 4),
        ("APPDATA", "", 3),
    ] {
        if let Some(d) = env(var) {
            roots.push((d.join(sub), depth));
        }
    }
    let mut found = Vec::new();
    for (root, depth) in roots {
        walk(&root, depth, &mut found);
    }
    found
}

/// Built-ins first, then the scanned packages that load, without duplicates (same directory
/// or same package id), then store packages (folder, .cdwidget) that are not installed or are
/// newer than the installed one. Sorted by name.
pub fn merge(builtins: Vec<Entry>, scanned: Vec<PathBuf>, store: Vec<(PathBuf, PathBuf)>) -> Vec<Entry> {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut seen_dirs: HashSet<PathBuf> = builtins.iter().map(|e| canon(&e.dir)).collect();
    let mut seen_ids: HashSet<String> = builtins.iter().map(|e| e.info.id.clone()).collect();
    let mut all = builtins;
    for dir in scanned {
        if !seen_dirs.insert(canon(&dir)) {
            continue;
        }
        match package_info(&dir) {
            Ok(info) if seen_ids.insert(info.id.clone()) => {
                let source = app_folder(&dir);
                all.push(Entry { dir, info, source, kind: Kind::App, store: None });
            }
            Ok(_) => {}
            Err(e) => eprintln!("calmdesk: skipping {}: {e}", dir.display()),
        }
    }
    for (dir, file) in store {
        let Ok(info) = package_info(&dir) else { continue };
        let source = match all.iter().find(|e| e.info.id == info.id) {
            Some(e) if !crate::update::newer(&info.version, &e.info.version) => continue,
            Some(_) => widgine::tr("Update available").into(),
            None if info.author.is_empty() => widgine::tr("Community").into(),
            None => info.author.clone(),
        };
        all.push(Entry { dir, info, source, kind: Kind::Community, store: Some(file) });
    }
    all.sort_by_key(|e| e.info.name.to_lowercase());
    all
}

/// Folders that never hold widget packages, or are huge.
const SKIP: &[&str] = &["windowsapps", "node_modules", ".git", "windows defender", "windowspowershell", "common files", "packages", "cache", "temp"];

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if dir.join("widget.toml").is_file() && dir.join("layout.xml").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Ok(meta) = e.metadata() else { continue };
        // Junctions and links can loop.
        const REPARSE_POINT: u32 = 0x400;
        if !meta.is_dir() || meta.file_attributes() & REPARSE_POINT != 0 {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        walk(&e.path(), depth - 1, out);
    }
}

/// "%LOCALAPPDATA%\x" -> "C:\Users\me\AppData\Local\x". Unknown variables stay as they are.
fn expand(s: &str) -> String {
    let parts: Vec<&str> = s.split('%').collect();
    let mut out = parts[0].to_string();
    let mut i = 1;
    while i < parts.len() {
        match std::env::var(parts[i]).ok().filter(|_| i + 1 < parts.len()) {
            Some(v) => {
                out += &v;
                out += parts[i + 1];
                i += 2;
            }
            None => {
                out += "%";
                out += parts[i];
                i += 1;
            }
        }
    }
    out
}

/// Directories registered under `key`: `Software\Widgine\Packages`, or the name from before
/// the engine was renamed.
fn registered(hive: HKEY, key_name: windows::core::PCWSTR) -> Vec<PathBuf> {
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(hive, key_name, None, KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            let mut data = [0u16; 1024];
            let mut data_len = (data.len() * 2) as u32;
            let mut kind = 0u32;
            let r = RegEnumValueW(
                key,
                i,
                Some(windows::core::PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                Some(&mut kind),
                Some(data.as_mut_ptr() as *mut u8),
                Some(&mut data_len),
            );
            if r.is_err() {
                break;
            }
            if REG_VALUE_TYPE(kind) == REG_SZ || REG_VALUE_TYPE(kind) == REG_EXPAND_SZ {
                let s = String::from_utf16_lossy(&data[..(data_len as usize / 2)]);
                out.push(PathBuf::from(expand(s.trim_end_matches('\0'))));
            }
        }
        let _ = RegCloseKey(key);
    }
    out
}

/// "C:\Program Files\Acme Weather\widgets\forecast" -> "Acme Weather": the first folder below
/// the scan root, or the package folder's parent for drop-in folders.
fn app_folder(dir: &Path) -> String {
    let parts: Vec<String> = dir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let lower: Vec<String> = parts.iter().map(|p| p.to_lowercase()).collect();
    let after = |name: &str| lower.iter().position(|p| p == name).and_then(|i| parts.get(i + 1)).cloned();
    after("program files")
        .or_else(|| after("program files (x86)"))
        .or_else(|| after("programs"))
        .or_else(|| after("roaming"))
        // %LOCALAPPDATA%\Widgets\<app>\<package>, and the same under ProgramData.
        .or_else(|| after("widgets"))
        .filter(|s| !s.eq_ignore_ascii_case(dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default().as_str()))
        .unwrap_or_else(|| widgine::tr("Other").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names() {
        assert_eq!(app_folder(Path::new(r"C:\Program Files\Acme Weather\widgets\forecast")), "Acme Weather");
        assert_eq!(app_folder(Path::new(r"C:\Users\a\AppData\Local\Programs\Notes\w\todo")), "Notes");
        assert_eq!(app_folder(Path::new(r"C:\Users\a\AppData\Local\Widgets\Demo App\spark")), "Demo App");
        assert_eq!(app_folder(Path::new(r"C:\Users\a\AppData\Local\Widgets\spark")), widgine::tr("Other"));
    }

    #[test]
    fn finds_packages_and_skips_the_rest() {
        let root = std::env::temp_dir().join(format!("wm-scan-{}", std::process::id()));
        let pkg = root.join("App").join("widgets").join("w");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("widget.toml"), "").unwrap();
        std::fs::write(pkg.join("layout.xml"), "").unwrap();
        std::fs::create_dir_all(root.join("node_modules").join("x")).unwrap();
        let mut found = Vec::new();
        walk(&root, 3, &mut found);
        assert_eq!(found, vec![pkg.clone()]);
        let mut shallow = Vec::new();
        walk(&root, 2, &mut shallow);
        assert!(shallow.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn expand_variables() {
        let windir = std::env::var("WINDIR").unwrap();
        assert_eq!(expand(r"%WINDIR%\x"), format!(r"{windir}\x"));
        assert_eq!(expand("100% %NO_SUCH_VAR_X% a"), "100% %NO_SUCH_VAR_X% a");
    }
}
