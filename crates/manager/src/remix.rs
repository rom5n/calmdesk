//! "Remix" on a gallery tile: a copy of the package in `%LOCALAPPDATA%\\Widgets` to edit. The
//! engine reloads placed widgets when their files change, so edits show as they are saved.
//!
//! The copy's id is the original's plus `.remix` (`.remix2`, ...), so it is fed like the
//! original: weather, media and the other built-in feeds key on [`original`].

use std::path::{Path, PathBuf};

/// "dev.widget-engine.weather.remix2" -> "dev.widget-engine.weather".
pub fn original(id: &str) -> &str {
    match id.rfind(".remix") {
        Some(i) if id[i + 6..].bytes().all(|b| b.is_ascii_digit()) => &id[..i],
        _ => id,
    }
}

/// Copies the package in `dir` to a new folder in the widgets folder with a new id and
/// "(remix)" after its name. Returns the copy's layout.xml.
pub fn remix(dir: &Path) -> Result<PathBuf, String> {
    remix_into(dir, &crate::install::widgets_dir())
}

fn remix_into(dir: &Path, widgets: &Path) -> Result<PathBuf, String> {
    let toml = std::fs::read_to_string(dir.join("widget.toml")).map_err(|e| e.to_string())?;
    let info = widgine::package_info(dir).map_err(|e| e.to_string())?;
    let base = original(&info.id).to_string();
    let folder = |n: usize| widgets.join(format!("{}-remix{}", dir.file_name().unwrap_or_default().to_string_lossy(), if n == 1 { String::new() } else { n.to_string() }));
    let n = (1..).find(|&n| !folder(n).exists()).unwrap_or(1);
    let target = folder(n);
    let id = if n == 1 { format!("{base}.remix") } else { format!("{base}.remix{n}") };
    copy_dir(dir, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    // The first `id = ` and `name = ` lines, before any [table].
    let mut seen = (false, false);
    let mut out = String::new();
    for line in toml.lines() {
        let key = line.split('=').next().unwrap_or("").trim();
        if key == "id" && !seen.0 {
            seen.0 = true;
            out += &format!("id = \"{id}\"\n");
        } else if key == "name" && !seen.1 {
            seen.1 = true;
            let name = line.split_once('=').map_or("", |(_, v)| v).trim().trim_matches('"');
            out += &format!("name = \"{name} (remix)\"\n");
        } else {
            out += line;
            out += "\n";
        }
    }
    std::fs::write(target.join("widget.toml"), out).map_err(|e| e.to_string())?;
    Ok(target.join("layout.xml"))
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_ids() {
        assert_eq!(original("dev.widget-engine.weather.remix"), "dev.widget-engine.weather");
        assert_eq!(original("dev.widget-engine.weather.remix12"), "dev.widget-engine.weather");
        assert_eq!(original("com.a.remixer"), "com.a.remixer");
    }

    #[test]
    fn remix_copies_and_renames() {
        let root = std::env::temp_dir().join(format!("calmdesk-remix-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../widgets/clock");
        let a = remix_into(&src, &root).unwrap();
        let b = remix_into(&src, &root).unwrap();
        let ia = widgine::package_info(a.parent().unwrap()).unwrap();
        let ib = widgine::package_info(b.parent().unwrap()).unwrap();
        let orig = widgine::package_info(&src).unwrap().id;
        assert_eq!((ia.id, ib.id), (format!("{orig}.remix"), format!("{orig}.remix2")));
        assert!(a.is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
