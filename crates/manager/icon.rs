// Shared by the build scripts of calmdesk and calmdesk-setup (include!): embeds an .ico as
// the exe icon (resource 1, also used for the tray icon), with rc.exe from the Windows SDK.
// Without the SDK the exe gets the default icon.

fn embed_icon(ico: &std::path::Path) {
    println!("cargo:rerun-if-changed={}", ico.display());
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe (Windows SDK) not found: the exe gets no icon");
        return;
    };
    let script = out.join("app.rc");
    std::fs::write(&script, format!("1 ICON \"{}\"\n", ico.display().to_string().replace('\\', "\\\\"))).unwrap();
    let res = out.join("app.res");
    let ok = std::process::Command::new(rc).arg("/nologo").arg("/fo").arg(&res).arg(&script).status().is_ok_and(|s| s.success());
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=rc.exe failed: the exe gets no icon");
    }
}

/// `RC` from the environment, else the newest x64 rc.exe of the Windows 10/11 SDK.
fn find_rc() -> Option<std::path::PathBuf> {
    if let Ok(rc) = std::env::var("RC") {
        return Some(rc.into());
    }
    let bin = std::path::Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut found: Vec<_> = std::fs::read_dir(bin).ok()?.flatten().map(|e| e.path().join("x64").join("rc.exe")).filter(|p| p.exists()).collect();
    found.sort();
    found.pop()
}
