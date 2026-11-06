//! Community widgets from the store index (github.com/rom5n/calmdesk-widgets). The index is a
//! text file, one package per line: `id<TAB>sha256<TAB>download url`. Its CI builds it from
//! the reviewed packages.
//!
//! Each package is downloaded once and unpacked into `data_dir()\store\packages\<sha256>`, so
//! the gallery shows live previews. A click installs the kept `.cdwidget` (see `install.rs`).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const INDEX: &str = "https://github.com/rom5n/calmdesk-widgets/releases/download/packages/index.txt";
/// The same limit as `install.rs` has for a .cdwidget.
const MAX_FILE: &str = "20971520";

fn dir() -> PathBuf {
    crate::data_dir().join("store")
}

/// Fetches the index at most once a day, downloads what is new and forgets what left the
/// index. Returns (package folder, .cdwidget) for every package ready to show. Offline: what
/// was fetched before. Slow: call it off the UI thread.
pub fn refresh() -> Vec<(PathBuf, PathBuf)> {
    let dir = dir();
    let index = dir.join("index.txt");
    let day = Duration::from_secs(24 * 60 * 60);
    let fresh = std::fs::metadata(&index).and_then(|m| m.modified()).is_ok_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < day);
    if !fresh {
        if let Some(text) = crate::feeds::curl(INDEX).filter(|t| parse(t).count() == t.lines().filter(|l| !l.trim().is_empty()).count()) {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&index, text);
        }
    }
    let text = std::fs::read_to_string(&index).unwrap_or_default();
    let (files, packages) = (dir.join("files"), dir.join("packages"));
    let mut ready = Vec::new();
    for (id, sha, url) in parse(&text) {
        let file = files.join(format!("{sha}.cdwidget"));
        let pkg = packages.join(sha);
        if !file.is_file() && !download(url, &file, sha) {
            continue;
        }
        if !pkg.is_dir() {
            if let Err(e) = unpack(&file, &pkg) {
                eprintln!("calmdesk: store {id}: {e}");
                continue;
            }
        }
        // The index names the package; a package claiming another id is not shown.
        match widgine::package_info(&pkg) {
            Ok(info) if info.id == id => ready.push((pkg, file)),
            _ => eprintln!("calmdesk: store {id}: not the package the index names"),
        }
    }
    // Packages that left the index, or were updated.
    let keep: Vec<String> = parse(&text).map(|(_, sha, _)| sha.to_string()).collect();
    for d in [&files, &packages] {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !keep.iter().any(|k| name.starts_with(k.as_str())) {
                let _ = std::fs::remove_dir_all(e.path()).or_else(|_| std::fs::remove_file(e.path()));
            }
        }
    }
    ready
}

/// (id, sha256, url) of each well-formed line.
fn parse(text: &str) -> impl Iterator<Item = (&str, &str, &str)> {
    text.lines().filter_map(|l| {
        let mut f = l.trim().split('\t');
        let (id, sha, url) = (f.next()?, f.next()?, f.next()?);
        let ok = !id.is_empty() && sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()) && url.starts_with("https://");
        ok.then_some((id, sha, url))
    })
}

/// Downloads `url` to `file` when its SHA-256 is `sha`.
fn download(url: &str, file: &Path, sha: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let tmp = file.with_extension("part");
    let _ = std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")));
    let ok = std::process::Command::new("curl.exe")
        .args(["-sfL", "-m", "120", "--max-filesize", MAX_FILE, "-o"])
        .arg(&tmp)
        .arg(url)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|s| s.success());
    let ok = ok && sha256(&tmp).is_some_and(|h| h.eq_ignore_ascii_case(sha)) && std::fs::rename(&tmp, file).is_ok();
    let _ = std::fs::remove_file(&tmp);
    ok
}

/// Hex SHA-256 of a file, by certutil (part of Windows).
fn sha256(file: &Path) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("certutil.exe").arg("-hashfile").arg(file).arg("SHA256").creation_flags(CREATE_NO_WINDOW).output().ok()?;
    // Line 2 is the hash; older Windows put spaces between the bytes.
    let hash: String = String::from_utf8_lossy(&out.stdout).lines().nth(1)?.chars().filter(|c| !c.is_whitespace()).collect();
    (hash.len() == 64).then_some(hash)
}

/// Unpacks into `pkg` through a temporary folder, so a half-unpacked package is never shown.
fn unpack(file: &Path, pkg: &Path) -> Result<(), String> {
    let tmp = pkg.with_extension("tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    let inner = crate::install::unpack(file, &tmp);
    let result = inner.and_then(|d| std::fs::rename(&d, pkg).map_err(|e| e.to_string()));
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn index_lines() {
        let sha = "a".repeat(64);
        let text = format!("com.a.b\t{sha}\thttps://x/y.cdwidget\nbad line\ncom.c\tshort\thttps://x\n\n");
        let got: Vec<_> = super::parse(&text).collect();
        assert_eq!(got, vec![("com.a.b", sha.as_str(), "https://x/y.cdwidget")]);
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(super::sha256(&file).map(|h| h.len()), Some(64));
    }
}
