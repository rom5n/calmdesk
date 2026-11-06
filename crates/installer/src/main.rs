//! Calmdesk setup: one window with the app icon and an Install button. Installs for the
//! current user, no admin rights: copies the app and the built-in widgets to
//! %LOCALAPPDATA%\Programs\Calmdesk, adds a Start menu shortcut, an Apps & features entry
//! and the sign-in start, then starts the app. Running it again updates the install. An install
//! from the time the app was called Widget Center or Widget Manager is replaced; the app moves
//! its widgets over.

#![windows_subsystem = "windows"]

use std::path::{Path, PathBuf};

use widgine::windows_numerics::Vector2;
use widgine::{Background, Card, Customization, Event, LayerId, Scene, Transition, Widget, rgba};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
use windows::core::{HSTRING, Interface, Result};

mod payload {
    include!(concat!(env!("OUT_DIR"), "/payload.rs"));
}

const ID: &str = "setup";
const SIZE: (f32, f32) = (420.0, 320.0);
const APP: &str = "Calmdesk";
const EXE: &str = "calmdesk.exe";
const BUTTON: (f32, f32, f32, f32) = (130.0, 236.0, 290.0, 276.0);
const CLOSE: (f32, f32, f32) = (SIZE.0 - 30.0, 30.0, 12.0);
// Same keys as the manager (shell.rs), which removes them on uninstall.
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Calmdesk";

fn install_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("Programs").join(APP)
}

fn hidden(program: impl AsRef<std::ffi::OsStr>, args: &[&str]) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new(program);
    c.args(args).creation_flags(CREATE_NO_WINDOW);
    c
}

/// Removes installs of the app's old names, Widget Manager and Widget Center (not their saved
/// widgets: the app moves those over).
fn remove_old() {
    for (name, exe, key) in [("Widget Manager", "widget-manager.exe", "WidgetManager"), ("Widget Center", "widget-center.exe", "WidgetCenter")] {
        let _ = hidden("taskkill", &["/F", "/IM", exe]).status();
        let dir = install_dir().with_file_name(name);
        if dir.join(exe).is_file() {
            // The killed process may take a moment to let go of its exe.
            for _ in 0..30 {
                if std::fs::remove_dir_all(&dir).is_ok() || !dir.exists() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        let programs = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join(r"Microsoft\Windows\Start Menu\Programs");
        let _ = std::fs::remove_file(programs.join(format!("{name}.lnk")));
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &HSTRING::from(name));
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{key}")));
        }
    }
}

fn install() -> std::result::Result<(), String> {
    remove_old();
    let dir = install_dir();
    let exe = dir.join(EXE);
    // A running manager holds its exe open.
    let _ = hidden("taskkill", &["/F", "/IM", EXE]).status();
    // Widgets the new version no longer ships go away.
    let _ = std::fs::remove_dir_all(dir.join("widgets"));
    for (rel, bytes) in payload::FILES {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| format!("{}: {e}", dir.display()))?;
        // The killed manager may take a moment to let go of the exe.
        let mut tries = 0;
        while let Err(e) = std::fs::write(&path, bytes) {
            tries += 1;
            if tries == 30 {
                return Err(format!("{}: {e}", path.display()));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    shortcut(&exe).map_err(|e| format!("shortcut: {e}"))?;
    let quoted = format!("\"{}\"", exe.display());
    let values = [
        ("DisplayName", APP.to_string()),
        ("DisplayIcon", exe.display().to_string()),
        ("DisplayVersion", env!("CARGO_PKG_VERSION").to_string()),
        ("Publisher", APP.to_string()),
        ("InstallLocation", dir.display().to_string()),
        ("UninstallString", format!("{quoted} --uninstall")),
    ];
    for (name, value) in values {
        reg(UNINSTALL_KEY, name, &value)?;
    }
    for name in ["NoModify", "NoRepair"] {
        reg_dword(UNINSTALL_KEY, name, 1)?;
    }
    reg(RUN_KEY, APP, &format!("{quoted} --background"))?;
    hidden(&exe, &["--background"]).spawn().map_err(|e| format!("{}: {e}", exe.display()))?;
    Ok(())
}

fn reg(key: &str, name: &str, value: &str) -> std::result::Result<(), String> {
    let data: Vec<u16> = value.encode_utf16().chain([0]).collect();
    let r = unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), REG_SZ.0, Some(data.as_ptr() as *const _), (data.len() * 2) as u32)
    };
    r.ok().map_err(|e| format!("{key}: {e}"))
}

fn reg_dword(key: &str, name: &str, value: u32) -> std::result::Result<(), String> {
    let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), REG_DWORD.0, Some(&value as *const u32 as *const _), 4) };
    r.ok().map_err(|e| format!("{key}: {e}"))
}

/// Start menu shortcut, the same one the manager removes on uninstall.
fn shortcut(exe: &Path) -> Result<()> {
    let programs = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join(r"Microsoft\Windows\Start Menu\Programs");
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        link.SetWorkingDirectory(&HSTRING::from(exe.parent().unwrap().as_os_str()))?;
        link.SetDescription(&HSTRING::from(widgine::tr("Desktop widgets for Windows")))?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(programs.join(format!("{APP}.lnk")).as_os_str()), true)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Button,
    Close,
}

enum Step {
    Ready,
    Done,
    Failed(String),
}

/// Parts drawn in their own layers, so they can appear one after another.
const LOGO: usize = 0;
const TEXT: usize = 1;
const ACTION: usize = 2;
/// Center of each part (logo, text, button, close), for its grow-in animation.
const CENTERS: [(f32, f32); 4] = [(SIZE.0 / 2.0, 82.0), (SIZE.0 / 2.0, 180.0), (SIZE.0 / 2.0, 256.0), (CLOSE.0, CLOSE.1)];

struct Setup {
    step: Step,
    hover: Option<Hit>,
    press: Option<Hit>,
    layers: Vec<LayerId>,
    /// Parts already shown by the opening animation; 4 when it is over.
    shown: usize,
}

impl Setup {
    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        let (l, t, r, b) = BUTTON;
        let (cx, cy, cr) = CLOSE;
        if (x - cx).hypot(y - cy) <= cr + 4.0 {
            Some(Hit::Close)
        } else if x >= l && x < r && y >= t && y < b {
            Some(Hit::Button)
        } else {
            None
        }
    }

    fn click(&mut self, hit: Hit) {
        match (hit, &self.step) {
            (Hit::Close, _) => widgine::close(ID),
            (Hit::Button, Step::Done) => {
                // A second start of the manager opens its gallery.
                let _ = std::process::Command::new(install_dir().join(EXE)).spawn();
                widgine::close(ID);
            }
            (Hit::Button, _) => {
                // ponytail: installs on the UI thread (a few MB, well under a second), so no "Installing…" state.
                self.step = match install() {
                    Ok(()) => Step::Done,
                    Err(e) => Step::Failed(e),
                };
            }
        }
    }

    fn draw(&self, scene: &mut Scene) -> Result<()> {
        let theme = *scene.theme();
        let fg = theme.foreground();
        let ink = |a: f32| rgba(fg.r, fg.g, fg.b, a);
        let fill = |a: f32| if theme.dark { rgba(1.0, 1.0, 1.0, a) } else { rgba(0.0, 0.0, 0.0, a * 0.6) };
        let fmt = |size: f32, weight: DWRITE_FONT_WEIGHT| -> Result<IDWriteTextFormat> {
            let f = scene.text_format(size, Some(weight))?;
            unsafe {
                f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            }
            Ok(f)
        };
        let title = fmt(22.0, DWRITE_FONT_WEIGHT_BOLD)?;
        let body = fmt(13.0, DWRITE_FONT_WEIGHT_NORMAL)?;
        let label = fmt(14.0, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
        let small = fmt(11.0, DWRITE_FONT_WEIGHT_NORMAL)?;
        let (button, note) = match &self.step {
            Step::Ready => (widgine::tr("Install"), String::new()),
            Step::Done => (widgine::tr("Open"), widgine::tr("Installed").to_string()),
            Step::Failed(e) => (widgine::tr("Install"), format!("{} {e}", widgine::tr("Installation failed:"))),
        };
        let failed = matches!(self.step, Step::Failed(_));
        let hover = self.hover;
        let d2d = scene.d2d().clone();
        for (part, &layer) in self.layers.iter().enumerate() {
            scene.draw(layer, |ctx| unsafe {
                let brush = |c: D2D1_COLOR_F| ctx.CreateSolidColorBrush(&c, None);
                let text = |s: &str, f: &IDWriteTextFormat, l: f32, t: f32, r: f32, b: f32, c: D2D1_COLOR_F| -> Result<()> {
                    let w: Vec<u16> = s.encode_utf16().collect();
                    ctx.DrawText(&w, f, &D2D_RECT_F { left: l, top: t, right: r, bottom: b }, &brush(c)?, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    Ok(())
                };
                match part {
                    LOGO => {
                        // The app logo, light or dark to match the theme.
                        let logo = widgine::load_image(ctx, if theme.dark { include_bytes!("../../manager/assets/logo-dark.png").as_slice() } else { include_bytes!("../../manager/assets/logo.png") })?;
                        let (s, l, t) = (104.0, (SIZE.0 - 104.0) / 2.0, 30.0);
                        ctx.DrawBitmap(&logo, Some(&D2D_RECT_F { left: l, top: t, right: l + s, bottom: t + s }), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
                    }
                    TEXT => {
                        text(APP, &title, 24.0, 142.0, SIZE.0 - 24.0, 174.0, fg)?;
                        text(widgine::tr("Desktop widgets for Windows"), &body, 24.0, 176.0, SIZE.0 - 24.0, 196.0, ink(0.55))?;
                        if !note.is_empty() {
                            let c = if failed { rgba(1.0, 0.35, 0.35, 1.0) } else { ink(0.55) };
                            text(&note, &small, 24.0, 204.0, SIZE.0 - 24.0, 228.0, c)?;
                        }
                    }
                    ACTION => {
                        // A neutral gray button, like the menus.
                        let (l, t, r, b) = BUTTON;
                        let pill = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: l, top: t, right: r, bottom: b }, radiusX: (b - t) / 2.0, radiusY: (b - t) / 2.0 };
                        ctx.FillRoundedRectangle(&pill, &brush(fill(if hover == Some(Hit::Button) { 0.17 } else { 0.11 }))?);
                        text(button, &label, l, t, r, b, fg)?;
                    }
                    _ => {
                        let (cx, cy, r) = CLOSE;
                        let ring = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r };
                        ctx.FillEllipse(&ring, &brush(fill(if hover == Some(Hit::Close) { 0.16 } else { 0.08 }))?);
                        let stroke = d2d.CreateStrokeStyle(&D2D1_STROKE_STYLE_PROPERTIES1 { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, ..Default::default() }, None)?;
                        let (k, xb) = (3.6, brush(ink(0.62))?);
                        ctx.DrawLine(Vector2 { X: cx - k, Y: cy - k }, Vector2 { X: cx + k, Y: cy + k }, &xb, 1.6, &stroke);
                        ctx.DrawLine(Vector2 { X: cx + k, Y: cy - k }, Vector2 { X: cx - k, Y: cy + k }, &xb, 1.6, &stroke);
                    }
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Shows part `i` of the opening animation: it fades in and springs up to full size.
    fn reveal(&self, scene: &mut Scene, i: usize) -> Result<()> {
        let (cx, cy) = CENTERS[i];
        scene.set_opacity(self.layers[i], 1.0, Transition::Ease { ms: 420 })?;
        scene.set_scale(self.layers[i], cx, cy, 1.0, Transition::Spring)
    }
}

impl Widget for Setup {
    fn size(&self) -> (f32, f32) {
        SIZE
    }

    fn customization(&self) -> Customization {
        Customization { card: Some(Card { radius: 30.0, background: Background::Solid, border: true, shadow: true }), ..Default::default() }
    }

    fn interactive(&self, x: f32, y: f32) -> bool {
        self.hit_at(x, y).is_some()
    }

    fn build(&mut self, scene: &mut Scene) -> Result<()> {
        self.layers = (0..CENTERS.len()).map(|_| scene.add_layer(0.0, 0.0, SIZE.0, SIZE.1)).collect::<Result<_>>()?;
        self.draw(scene)?;
        if self.shown == 0 {
            // Opening: the card grows in, then the logo, text, button and close button follow.
            scene.appear(SIZE, 0.94, Transition::Ease { ms: 380 })?;
            for (i, &layer) in self.layers.iter().enumerate() {
                let (cx, cy) = CENTERS[i];
                scene.set_opacity(layer, 0.0, Transition::None)?;
                scene.set_scale(layer, cx, cy, 0.86, Transition::None)?;
            }
            scene.set_timer(110);
        } else {
            // Rebuilt (theme, monitor): parts already shown stay shown.
            for i in 0..self.shown {
                self.reveal(scene, i)?;
            }
        }
        Ok(())
    }

    fn on_event(&mut self, event: Event, scene: &mut Scene) -> Result<()> {
        let before = self.hover;
        match event {
            Event::Timer if self.shown < self.layers.len() => {
                self.reveal(scene, self.shown)?;
                self.shown += 1;
                if self.shown < self.layers.len() {
                    scene.set_timer(70);
                }
            }
            Event::PointerMove { x, y } => self.hover = self.hit_at(x, y),
            Event::PointerLeave => self.hover = None,
            Event::PointerDown { x, y } => self.press = self.hit_at(x, y),
            Event::PointerUp { x, y } => {
                if let Some(h) = self.press.take().filter(|&h| Some(h) == self.hit_at(x, y)) {
                    self.click(h);
                    return self.draw(scene);
                }
            }
            _ => {}
        }
        if self.hover != before {
            self.draw(scene)?;
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let engine = widgine::Engine::new();
    // Middle of the primary monitor.
    let (w, h) = unsafe {
        use windows::Win32::UI::WindowsAndMessaging::*;
        (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN))
    };
    let s = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() } as f32 / 96.0;
    let (x, y) = (w / 2 - (SIZE.0 * s / 2.0) as i32, h / 2 - (SIZE.1 * s / 2.0) as i32);
    widgine::open_panel(ID, Setup { step: Step::Ready, hover: None, press: None, layers: Vec::new(), shown: 0 }, x, y);
    engine.run()
}
