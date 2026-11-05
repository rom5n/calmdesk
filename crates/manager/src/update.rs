//! Asks GitHub once a day whether a newer Calmdesk is out. The tray menu then offers it.

use std::sync::Mutex;
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/rom5n/calmdesk/releases/latest";

/// (version, release page) of a newer release.
static NEWER: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Checks now, then every day, on a thread of its own.
pub fn start() {
    std::thread::spawn(|| {
        loop {
            check();
            std::thread::sleep(Duration::from_secs(24 * 60 * 60));
        }
    });
}

/// Checks once, now: whether a newer release is out. None: GitHub did not answer with one.
pub fn check() -> Option<bool> {
    // ponytail: anonymous API, 60 requests an hour per IP; fine for one a day. A private repo
    // answers 404 until it is public.
    let json = crate::feeds::curl(LATEST)?;
    let tag = crate::feeds::field(&json, "tag_name")?.to_string();
    let url = crate::feeds::field(&json, "html_url")?.to_string();
    let found = newer(&tag, env!("CARGO_PKG_VERSION")).then_some((tag, url));
    let any = found.is_some();
    *NEWER.lock().unwrap() = found;
    Some(any)
}

/// The newer version's tag and page, if any.
pub fn available() -> Option<(String, String)> {
    NEWER.lock().unwrap().clone()
}

/// "v0.2.0" is newer than "0.1.3": numbers compared part by part.
fn newer(tag: &str, current: &str) -> bool {
    let parts = |v: &str| v.trim_start_matches('v').split('.').map(|p| p.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
    parts(tag) > parts(current)
}

#[cfg(test)]
mod tests {
    #[test]
    fn versions() {
        assert!(super::newer("v0.2.0", "0.1.9"));
        assert!(super::newer("0.10.0", "0.9.0"));
        assert!(!super::newer("v0.1.0", "0.1.0"));
        assert!(!super::newer("v0.0.9", "0.1.0"));
    }
}
