//! `.cdwidget` files: a zip of a widget package (`widget-dev pack`). Double-clicking one runs
//! `calmdesk.exe --install <file>`, which unpacks it into `%LOCALAPPDATA%\Widgets\<id>`, where
//! the gallery finds it.
//!
//! The file comes from anywhere, so it is checked before anything lands in the widgets folder:
//! size limits read from the zip's own directory, extraction into a private folder (tar.exe
//! rejects `..` and absolute paths), no links, and a package the engine accepts.

use std::path::{Path, PathBuf};

use widgine::PackageInfo;

/// The .cdwidget file itself.
const MAX_FILE: u64 = 20 << 20;
/// Everything in it, unpacked.
const MAX_UNPACKED: u64 = 50 << 20;
const MAX_ENTRIES: usize = 1000;

/// Where packages for this user go: `%LOCALAPPDATA%\Widgets`.
pub fn widgets_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Widgets")
}

/// Installs `file`. `confirm(info, replacing)` asks the user; false leaves everything as it
/// was. Ok(Some((info, folder))) when installed, Ok(None) when the user said no.
pub fn install(file: &Path, confirm: impl Fn(&PackageInfo, bool) -> bool) -> Result<Option<(PackageInfo, PathBuf)>, String> {
    // In the data folder: on the same drive as the widgets folder, so the move is a rename.
    let work = crate::data_dir().join("install");
    let _ = std::fs::remove_dir_all(&work);
    let result = unpack_and_move(file, &work, &widgets_dir(), confirm);
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn unpack_and_move(file: &Path, work: &Path, widgets: &Path, confirm: impl Fn(&PackageInfo, bool) -> bool) -> Result<Option<(PackageInfo, PathBuf)>, String> {
    let dir = unpack(file, &work.join("new"))?;
    let info = widgine::package_info(&dir).map_err(|e| e.to_string())?;
    let target = widgets.join(folder_name(&info.id));
    if !confirm(&info, target.exists()) {
        return Ok(None);
    }
    std::fs::create_dir_all(widgets).map_err(|e| format!("{}: {e}", widgets.display()))?;
    let old = work.join("old");
    let replacing = target.exists();
    if replacing {
        std::fs::rename(&target, &old).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    if let Err(e) = std::fs::rename(&dir, &target) {
        if replacing {
            let _ = std::fs::rename(&old, &target);
        }
        return Err(format!("{}: {e}", target.display()));
    }
    Ok(Some((info, target)))
}

/// Unpacks `file` into `into` (made fresh) and returns the package folder in it: `into` itself,
/// or its only subfolder when the zip holds one folder.
pub(crate) fn unpack(file: &Path, into: &Path) -> Result<PathBuf, String> {
    let bytes = std::fs::metadata(file).map_err(|e| format!("{}: {e}", file.display()))?.len();
    if bytes > MAX_FILE {
        return Err(format!("{}: larger than {} MB", file.display(), MAX_FILE >> 20));
    }
    let zip = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let (entries, unpacked) = zip_totals(&zip).ok_or("not a .cdwidget (zip) file")?;
    if entries > MAX_ENTRIES || unpacked > MAX_UNPACKED {
        return Err(format!("too large: {entries} files, {} MB unpacked", unpacked >> 20));
    }
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("tar.exe")
        .arg("-xf")
        .arg(file)
        .arg("-C")
        .arg(into)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("tar.exe: {e}"))?;
    if !status.success() {
        return Err("the file is damaged or holds paths outside its folder".into());
    }
    no_links(into)?;
    if into.join("widget.toml").is_file() {
        return Ok(into.to_path_buf());
    }
    let subs: Vec<PathBuf> = std::fs::read_dir(into).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
    match subs.as_slice() {
        [only] if only.join("widget.toml").is_file() => Ok(only.clone()),
        _ => Err("no widget.toml in the file".into()),
    }
}

/// Symbolic links and junctions could point the package anywhere on the PC.
fn no_links(dir: &Path) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let meta = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(format!("{}: links are not allowed", entry.file_name().to_string_lossy()));
        }
        if meta.is_dir() {
            no_links(&entry.path())?;
        }
    }
    Ok(())
}

/// Number of entries and their unpacked size, from the zip's central directory. None: not a zip.
fn zip_totals(zip: &[u8]) -> Option<(usize, u64)> {
    let u16_at = |i: usize| Some(u16::from_le_bytes(zip.get(i..i + 2)?.try_into().ok()?) as usize);
    let u32_at = |i: usize| Some(u32::from_le_bytes(zip.get(i..i + 4)?.try_into().ok()?) as u64);
    // The end record is the last 22 bytes, plus a comment of up to 64 KB.
    let end = (0..zip.len().saturating_sub(21)).rev().take(22 + 0xFFFF).find(|&i| zip[i..].starts_with(&[0x50, 0x4B, 0x05, 0x06]))?;
    let count = u16_at(end + 10)?;
    let mut at = u32_at(end + 16)? as usize;
    let mut total = 0u64;
    for _ in 0..count {
        if zip.get(at..at + 4)? != [0x50, 0x4B, 0x01, 0x02] {
            return None;
        }
        total += u32_at(at + 24)?;
        at += 46 + u16_at(at + 28)? + u16_at(at + 30)? + u16_at(at + 32)?;
    }
    Some((count, total))
}

/// "com.example.weather" -> "com.example.weather"; anything but letters, digits, '.', '-', '_'
/// becomes '-', so an id cannot name another folder.
fn folder_name(id: &str) -> String {
    let name: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '-' }).collect();
    match name.trim_matches('.') {
        "" => "widget".into(),
        n => n.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip made by tar.exe, the way `widget-dev pack` makes them.
    fn pack(dir: &Path, out: &Path) {
        let o = std::process::Command::new("tar.exe").args(["--format", "zip", "-cf"]).arg(out).arg("-C").arg(dir).arg(".").output().unwrap();
        assert!(o.status.success(), "tar: {}", String::from_utf8_lossy(&o.stderr));
    }

    #[test]
    fn installs_and_replaces() {
        let root = std::env::temp_dir().join(format!("calmdesk-install-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../widgets/clock");
        let file = root.join("clock.cdwidget");
        std::fs::create_dir_all(&root).unwrap();
        pack(&src, &file);

        let zip = std::fs::read(&file).unwrap();
        let (n, size) = zip_totals(&zip).unwrap();
        assert!(n >= 2 && size > 100, "{n} {size}");
        assert!(zip_totals(b"not a zip at all, just some text here").is_none());

        let widgets = root.join("Widgets");
        let (_, target) = unpack_and_move(&file, &root.join("work1"), &widgets, |_, replacing| !replacing).unwrap().unwrap();
        assert!(target.join("layout.xml").is_file());
        // Declined: nothing changes.
        std::fs::write(target.join("marker"), "old").unwrap();
        assert!(unpack_and_move(&file, &root.join("work2"), &widgets, |_, _| false).unwrap().is_none());
        assert!(target.join("marker").is_file());
        // Accepted: the new version replaces the old one.
        unpack_and_move(&file, &root.join("work3"), &widgets, |_, replacing| replacing).unwrap().unwrap();
        assert!(!target.join("marker").exists() && target.join("widget.toml").is_file());

        // Not a package.
        let empty = root.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("readme.txt"), "hi").unwrap();
        let bad = root.join("bad.cdwidget");
        pack(&empty, &bad);
        assert!(unpack_and_move(&bad, &root.join("work4"), &widgets, |_, _| true).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn zip_with_one_folder() {
        let root = std::env::temp_dir().join(format!("calmdesk-unpack-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("src");
        let pkg = src.join("clock");
        std::fs::create_dir_all(&pkg).unwrap();
        for f in ["widget.toml", "layout.xml"] {
            std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../widgets/clock").join(f), pkg.join(f)).unwrap();
        }
        let file = root.join("c.cdwidget");
        pack(&src, &file);
        assert_eq!(unpack(&file, &root.join("out")).unwrap(), root.join("out").join("clock"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name("com.example.weather"), "com.example.weather");
        assert_eq!(folder_name("..\\..\\Windows"), "-..-Windows");
        assert_eq!(folder_name(".."), "widget");
    }
}
