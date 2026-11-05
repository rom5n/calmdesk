//! Packs the payload into the exe: calmdesk.exe from the same target folder and the
//! built-in widgets (every package in `widgets`) and fonts. Build
//! the app first:
//!   cargo build --release -p calmdesk
//!   cargo build --release -p calmdesk-setup

use std::path::{Path, PathBuf};

include!("../manager/icon.rs");

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    embed_icon(&root.join("../manager/assets/app.ico"));
    println!("cargo:rerun-if-changed=../manager/icon.rs");

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR is <target>/<profile>/build/<crate>-<hash>/out.
    let exe = std::env::var_os("CALMDESK_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| out.ancestors().nth(3).unwrap().join("calmdesk.exe"));
    println!("cargo:rerun-if-env-changed=CALMDESK_EXE");
    println!("cargo:rerun-if-changed={}", exe.display());
    assert!(exe.exists(), "{} not found: build it first with `cargo build --release -p calmdesk`", exe.display());

    let mut files = vec![("calmdesk.exe".to_string(), exe)];
    let widgets = root.join("../../widgets");
    println!("cargo:rerun-if-changed={}", widgets.display());
    for e in std::fs::read_dir(&widgets).unwrap().flatten() {
        let w = e.file_name().to_string_lossy().into_owned();
        if e.path().join("widget.toml").is_file() {
            println!("cargo:rerun-if-changed={}", e.path().display());
            collect(&e.path(), &format!("widgets/{w}"), &mut files);
        }
    }
    // The fonts widgets can pick (with their licenses).
    let fonts = root.join("../../fonts");
    println!("cargo:rerun-if-changed={}", fonts.display());
    collect(&fonts, "fonts", &mut files);
    let mut code = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (rel, path) in files {
        code += &format!("    ({rel:?}, include_bytes!({:?})),\n", path.canonicalize().unwrap());
    }
    code += "];\n";
    std::fs::write(out.join("payload.rs"), code).unwrap();
}

fn collect(dir: &Path, rel: &str, files: &mut Vec<(String, PathBuf)>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let name = format!("{rel}/{}", e.file_name().to_string_lossy());
        if e.path().is_dir() {
            collect(&e.path(), &name, files);
        } else if e.path().extension().is_none_or(|x| x != "py") {
            // Not the generator scripts some widgets keep next to their layout.
            files.push((name, e.path()));
        }
    }
}
