//! The app has no console, so everything it (and the engine) writes to stderr goes to
//! `calmdesk.log` in the data folder, and a crash leaves `crash-<time>.txt` next to it.

use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{STD_ERROR_HANDLE, SetStdHandle};

/// Bigger than this at start: the log becomes `calmdesk.old.log` and a new one begins.
const MAX_LOG: u64 = 1024 * 1024;

pub fn log_file(dir: &Path) -> PathBuf {
    dir.join("calmdesk.log")
}

/// Sends stderr to the log and installs the crash report. Call first thing in `main`.
pub fn start(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = log_file(dir);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG) {
        let _ = std::fs::rename(&path, dir.join("calmdesk.old.log"));
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "\n--- {} Calmdesk {} started", now(), env!("CARGO_PKG_VERSION"));
        // Rust's stderr looks up the handle on each write: this redirects every eprintln.
        unsafe {
            let _ = SetStdHandle(STD_ERROR_HANDLE, HANDLE(file.as_raw_handle()));
        }
        // The handle must stay open for the life of the process.
        std::mem::forget(file);
    }
    let dir = dir.to_path_buf();
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let report = format!(
            "Calmdesk {} crashed at {}\n\n{info}\n\n{}\n",
            env!("CARGO_PKG_VERSION"),
            now(),
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::fs::write(dir.join(format!("crash-{}.txt", now().replace([':', ' '], "-"))), &report);
        eprintln!("{report}");
        default(info);
    }));
}

/// Local time, "2026-11-06 01:02:03".
fn now() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!("{}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}
