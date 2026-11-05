//! Embeds assets/app.ico as the exe icon, see icon.rs.

include!("icon.rs");

fn main() {
    println!("cargo:rerun-if-changed=icon.rs");
    embed_icon(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join("app.ico"));
}
