//! Community widgets from the store index (github.com/rom5n/calmdesk-widgets). The index is a
//! text file: "#" comment lines, then one package per line with tab-separated id, sha256,
//! download url, version, author, category, name and download count. Its CI builds it from the
//! reviewed packages. `featured.txt` next to it lists the Featured shelves.
//!
//! The gallery lists every package from the index alone. A package is downloaded only when its
//! tile is shown (for the live preview) or picked, and unpacked into
//! `data_dir()\store\packages\<sha256>`. Downloads not used for a month are deleted.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

const RELEASE: &str = "https://github.com/rom5n/calmdesk-widgets/releases/download/packages/";
/// The same limit as `install.rs` has for a .cdwidget.
const MAX_FILE: &str = "20971520";
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// One line of the index.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub sha: String,
    pub url: String,
    pub version: String,
    pub author: String,
    pub category: String,
    pub name: String,
    pub downloads: u64,
}

/// A Featured shelf showing today: its title and package ids.
#[derive(Clone, Debug, PartialEq)]
pub struct Shelf {
    pub title: String,
    pub ids: Vec<String>,
}

impl Item {
    /// The unpacked package, once downloaded.
    pub fn dir(&self) -> PathBuf {
        self.dir_in(&dir())
    }

    fn dir_in(&self, store: &Path) -> PathBuf {
        store.join("packages").join(&self.sha)
    }

    fn file_in(&self, store: &Path) -> PathBuf {
        store.join("files").join(format!("{}.cdwidget", self.sha))
    }
}

fn dir() -> PathBuf {
    crate::data_dir().join("store")
}

/// Fetches the index and the shelves at most once an hour and deletes downloads that left the
/// index or were not used for a month. Offline: what was fetched before. Slow: call it off the
/// UI thread.
pub fn refresh() -> (Vec<Item>, Vec<Shelf>) {
    let dir = dir();
    let index = dir.join("index.txt");
    let featured = dir.join("featured.txt");
    // Hourly: a 1 KB file from release downloads, which have no API rate limit.
    if age(&index) >= Duration::from_secs(60 * 60) {
        let content = |t: &str| t.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')).count();
        let _ = std::fs::create_dir_all(&dir);
        // Only whole files: a "Not Found" page is no index.
        if let Some(text) = crate::feeds::curl(&format!("{RELEASE}index.txt")).filter(|t| t.starts_with('#') && parse(t).count() == content(t)) {
            let _ = std::fs::write(&index, text);
        }
        if let Some(text) = crate::feeds::curl(&format!("{RELEASE}featured.txt")).filter(|t| t.starts_with('#')) {
            let _ = std::fs::write(&featured, text);
        }
    }
    let items: Vec<Item> = parse(&std::fs::read_to_string(&index).unwrap_or_default()).collect();
    let shelves = shelves(&std::fs::read_to_string(&featured).unwrap_or_default(), &today());
    for d in [dir.join("files"), dir.join("packages")] {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let listed = items.iter().any(|i| name.starts_with(i.sha.as_str()));
            if !listed || age(&e.path()) > 30 * DAY {
                let _ = std::fs::remove_dir_all(e.path()).or_else(|_| std::fs::remove_file(e.path()));
            }
        }
    }
    (items, shelves)
}

/// "2026-11-06", local time.
fn today() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay)
}

/// The shelves of featured.txt (title, first day, last day, ids) whose days include `today`.
fn shelves(text: &str, today: &str) -> Vec<Shelf> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
            let [title, from, to, ids] = f[..] else { return None };
            // ISO dates compare as text.
            (from <= today && today <= to).then(|| Shelf { title: title.into(), ids: ids.split(',').map(|i| i.trim().to_string()).collect() })
        })
        .collect()
}

/// Time since `path` was written; very long when it is missing.
fn age(path: &Path) -> Duration {
    let written = std::fs::metadata(path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    SystemTime::now().duration_since(written).unwrap_or_default()
}

fn parse(text: &str) -> impl Iterator<Item = Item> + '_ {
    text.lines().filter(|l| !l.starts_with('#')).filter_map(|l| {
        let f: Vec<&str> = l.trim_end_matches('\r').split('\t').collect();
        // Indexes from before the download count have 7 fields.
        let (&[id, sha, url, version, author, category, name], downloads) = (f.get(..7)?, f.get(7)) else { return None };
        let ok = !id.is_empty() && sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()) && url.starts_with("https://");
        let s = |x: &str| x.to_string();
        ok.then(|| Item { id: s(id), sha: sha.to_ascii_lowercase(), url: s(url), version: s(version), author: s(author), category: s(category), name: s(name), downloads: downloads.and_then(|d| d.parse().ok()).unwrap_or(0) })
    })
}

/// Shas being downloaded, so each is fetched once at a time.
static FETCHING: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// Downloads and unpacks `item` on a thread of its own, then calls `done`. Does nothing when
/// it is there already or on its way.
pub fn fetch(item: &Item, done: impl FnOnce() + Send + 'static) {
    if item.dir().is_dir() || !FETCHING.lock().unwrap().get_or_insert_default().insert(item.sha.clone()) {
        return;
    }
    let item = item.clone();
    std::thread::spawn(move || {
        if let Err(e) = get(&item) {
            eprintln!("calmdesk: store {}: {e}", item.id);
        }
        FETCHING.lock().unwrap().get_or_insert_default().remove(&item.sha);
        done();
    });
}

/// Downloads `item` if needed, unpacks it, and returns its .cdwidget. Blocks.
pub fn get(item: &Item) -> Result<PathBuf, String> {
    get_in(&dir(), item)
}

fn get_in(store: &Path, item: &Item) -> Result<PathBuf, String> {
    let file = item.file_in(store);
    if !file.is_file() && !download(&item.url, &file, &item.sha) {
        return Err(format!("{}: download failed or the file does not match the index", item.url));
    }
    let pkg = item.dir_in(store);
    if !pkg.is_dir() {
        unpack(&file, &pkg)?;
    }
    // The index names the package; a package claiming another id is not used.
    match widgine::package_info(&pkg) {
        Ok(info) if info.id == item.id => Ok(file),
        _ => {
            let _ = std::fs::remove_dir_all(&pkg);
            Err("not the package the index names".into())
        }
    }
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
        let text = format!("# index\ncom.a.b\t{sha}\thttps://x/y.cdwidget\t1.0\tMe\tTime\tBig Clock\t42\nbad line\ncom.c\tshort\thttps://x\t1\ta\tb\tc\n\n");
        let got: Vec<_> = super::parse(&text).collect();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].downloads, 42);
        let shelves = super::shelves("# x\nWeek\t2026-11-01\t2026-11-07\ta.b, c.d\nOld\t2025-01-01\t2025-01-02\te\n", "2026-11-06");
        assert_eq!(shelves, vec![super::Shelf { title: "Week".into(), ids: vec!["a.b".into(), "c.d".into()] }]);
        assert_eq!((got[0].id.as_str(), got[0].name.as_str(), got[0].version.as_str()), ("com.a.b", "Big Clock", "1.0"));
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(super::sha256(&file).map(|h| h.len()), Some(64));
    }

    /// A downloaded file is unpacked and checked against the index's id.
    #[test]
    fn get_unpacks_and_checks_the_id() {
        let root = std::env::temp_dir().join(format!("calmdesk-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../widgets/clock");
        let id = widgine::package_info(&src).unwrap().id;
        let item = |id: &str| super::Item { id: id.into(), sha: "c".repeat(64), url: "https://x".into(), version: "1".into(), author: "a".into(), category: "Time".into(), name: "n".into(), downloads: 0 };
        let file = item(&id).file_in(&root);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let ok = std::process::Command::new("tar.exe").args(["--format", "zip", "-cf"]).arg(&file).arg("-C").arg(&src).arg(".").status().unwrap().success();
        assert!(ok);
        assert!(super::get_in(&root, &item("com.someone.else")).is_err());
        assert!(!item(&id).dir_in(&root).exists());
        assert_eq!(super::get_in(&root, &item(&id)).unwrap(), file);
        assert!(item(&id).dir_in(&root).join("widget.toml").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
