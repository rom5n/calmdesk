//! Standalone Now Playing widget. Calmdesk shows the same widget from its gallery.
//!
//! Run: cargo run -p media --release

use widgine::Engine;
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};

const ID: &str = "media";

fn main() -> widgine::windows::core::Result<()> {
    let mut engine = Engine::new();
    let x = unsafe { GetSystemMetrics(SM_CXSCREEN) } - 400;
    engine.load(ID, concat!(env!("CARGO_MANIFEST_DIR"), "/../../widgets/media"), x, 460)?;
    engine.on_action(|_, action| {
        media::action(action);
    });
    media::start();
    media::add(ID);
    engine.run()
}
