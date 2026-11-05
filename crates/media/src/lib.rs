//! Now Playing widget in the style of macOS: artwork, title, artist, progress, and transport
//! buttons for whatever app plays media (Spotify, browsers, Media Player, ...), through the
//! Windows media session API (GlobalSystemMediaTransportControls). widgets/media draws it; this
//! app feeds it `{app.*}` values and handles its buttons.
//!
//! The media session calls block, and some (artwork) need the calling thread free to finish,
//! so they run on a worker thread. The UI thread only copies the latest snapshot into the widgets.
//!
//! A library, so Calmdesk shows it too: [`start`], then [`add`] each widget. The `media`
//! binary is the standalone app.

use std::cell::RefCell;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Media::Control::*;
use windows::Storage::Streams::DataReader;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::SetTimer;
use windows::core::Result;

/// What the widget shows: `{app.*}` values.
#[derive(Clone, Default, PartialEq)]
struct Snapshot {
    values: Vec<(&'static str, String)>,
}

/// Latest snapshot from the worker, taken by the UI thread.
static LATEST: Mutex<Option<Snapshot>> = Mutex::new(None);
/// Button presses and seeks ("seek:0.42") for the worker.
static COMMANDS: OnceLock<Mutex<Sender<String>>> = OnceLock::new();
/// When the user last moved the bar (UI thread). The app reports the old place for a moment
/// after a seek: snapshots leave the bar and times alone for a while.
static SEEKED: Mutex<Option<Instant>> = Mutex::new(None);
/// Play state the widget shows, and the state a click asked for with when (UI thread).
/// The icon flips on click; the app's reported state may lag behind for a moment, so
/// snapshots that disagree with the click are ignored for a while.
static PLAYING: Mutex<(bool, Option<(bool, Instant)>)> = Mutex::new((false, None));
const HOLD: Duration = Duration::from_secs(2);

/// "Spotify.exe", "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify", "MSEdge", "chrome" -> a name.
fn app_name(aumid: &str) -> String {
    let id = aumid.rsplit('!').next().unwrap_or(aumid).trim_end_matches(".exe");
    let lower = id.to_lowercase();
    match () {
        _ if lower.contains("spotify") => "Spotify".into(),
        _ if lower.contains("chrome") => "Google Chrome".into(),
        _ if lower.contains("msedge") => "Microsoft Edge".into(),
        _ if lower.contains("firefox") || id == "308046B0AF4A39CB" => "Firefox".into(),
        _ if lower.contains("zunemusic") || lower.contains("media.player") => "Media Player".into(),
        _ => {
            let mut c = id.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

/// 100 ns ticks to "m:ss" (or "h:mm:ss").
fn clock(ticks: i64) -> String {
    let s = (ticks.max(0) / 10_000_000) as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// Now in WinRT DateTime ticks (100 ns since 1601).
fn now_ticks() -> i64 {
    let unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    (unix.as_nanos() / 100) as i64 + 116_444_736_000_000_000
}

/// Saves the track's thumbnail to a temp file. None when the app gives none (yet).
fn save_art(props: &GlobalSystemMediaTransportControlsSessionMediaProperties, key: &str) -> Option<String> {
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = stream.Size().ok()? as u32;
    if size == 0 {
        return None;
    }
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    let dir = std::env::temp_dir().join("widget-media");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("art-{:016x}.img", h.finish()));
    std::fs::write(&path, bytes).ok()?;
    Some(path.to_string_lossy().into_owned())
}

fn nothing_playing() -> Snapshot {
    let v = [("title", widget_engine::tr("Not Playing")), ("artist", ""), ("source", ""), ("art", ""), ("playing", ""), ("timed", "0"), ("elapsed", ""), ("remaining", ""), ("position", "0"), ("bar", "0"), ("controls", "0.35")];
    Snapshot { values: v.iter().map(|(k, v)| (*k, v.to_string())).collect() }
}

/// Worker thread state: the session manager and the artwork of the current track.
struct Reader {
    manager: Option<GlobalSystemMediaTransportControlsSessionManager>,
    /// ("title\u{1}artist", artwork file or "").
    track: (String, String),
    /// Time played in this track by our own count (100 ns ticks), for apps that report no
    /// timeline, and when we counted last.
    counted: i64,
    last: Instant,
    /// Last known length of this track (100 ns ticks), 0 = never reported.
    length: i64,
}

impl Reader {
    fn session(&mut self) -> Option<GlobalSystemMediaTransportControlsSession> {
        if self.manager.is_none() {
            self.manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|op| op.join()).ok();
        }
        self.manager.as_ref()?.GetCurrentSession().ok()
    }

    fn command(&mut self, action: &str) {
        let Some(s) = self.session() else { return };
        if let Some(f) = action.strip_prefix("seek:").and_then(|f| f.parse::<f64>().ok()) {
            // Browsers sometimes drop the timeline: then the last length we saw.
            let (start, end) = s
                .GetTimelineProperties()
                .and_then(|t| Ok((t.StartTime()?.Duration, t.EndTime()?.Duration)))
                .ok()
                .filter(|(a, b)| b > a)
                .unwrap_or((0, self.length));
            if end > start {
                let at = start + ((end - start) as f64 * f.clamp(0.0, 1.0)) as i64;
                self.counted = at - start;
                let _ = s.TryChangePlaybackPositionAsync(at).and_then(|op| op.join());
            }
            return;
        }
        let op = match action {
            "previous" => s.TrySkipPreviousAsync(),
            "playpause" => s.TryTogglePlayPauseAsync(),
            _ => s.TrySkipNextAsync(),
        };
        let _ = op.and_then(|op| op.join());
    }

    fn read(&mut self) -> Result<Snapshot> {
        let Some(s) = self.session() else { return Ok(nothing_playing()) };
        let props = s.TryGetMediaPropertiesAsync()?.join()?;
        let (title, artist) = (props.Title()?.to_string(), props.Artist()?.to_string());
        let key = format!("{title}\u{1}{artist}");
        if self.track.0 != key {
            if !self.track.1.is_empty() {
                let _ = std::fs::remove_file(&self.track.1);
            }
            self.track = (key.clone(), String::new());
            (self.counted, self.length) = (0, 0);
        }
        // Apps often send the artwork a moment after the title: retry until it arrives.
        if self.track.1.is_empty() {
            self.track.1 = save_art(&props, &key).unwrap_or_default();
        }
        let playing = s.GetPlaybackInfo()?.PlaybackStatus()? == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing;
        let timeline = s.GetTimelineProperties()?;
        let end = timeline.EndTime()?.Duration - timeline.StartTime()?.Duration;
        let mut pos = timeline.Position()?.Duration;
        // Apps report the position only now and then: extrapolate while playing.
        if playing {
            pos += now_ticks() - timeline.LastUpdatedTime()?.UniversalTime;
        }
        // Browsers often drop the timeline (length 0) mid-track: count the time ourselves from
        // the last real position, and keep the last known length of this track.
        let now = Instant::now();
        if playing {
            self.counted += (now - self.last).as_nanos() as i64 / 100;
        }
        self.last = now;
        if end > 0 {
            (self.counted, self.length) = (pos.clamp(0, end), end);
        }
        let end = self.length;
        let pos = if end > 0 { self.counted.min(end) } else { self.counted };
        // -1 = length never known for this track: no bar.
        let progress = if end > 0 { pos as f32 / end as f32 } else { -1.0 };
        let values = vec![
            ("title", if title.is_empty() { widget_engine::tr("Unknown|track").into() } else { title }),
            ("artist", artist),
            ("source", app_name(&s.SourceAppUserModelId()?.to_string())),
            ("art", self.track.1.clone()),
            ("playing", if playing { "on" } else { "" }.into()),
            ("controls", "1".into()),
            ("timed", "1".into()),
            ("elapsed", clock(pos)),
            ("remaining", if end > 0 { format!("-{}", clock(end - pos)) } else { String::new() }),
            ("position", format!("{progress:.4}")),
            // Percent played for assets/progress.svg; an empty bar without a track length.
            ("bar", if end > 0 { format!("{:.2}", progress * 100.0) } else { "0".into() }),
        ];
        Ok(Snapshot { values })
    }
}

/// Reads the media session once a second, and right after a button press.
// ponytail: polls. The session has change events too; polling also drives the progress clock.
fn worker(commands: Receiver<String>) {
    let mut reader = Reader { manager: None, track: Default::default(), counted: 0, last: Instant::now(), length: 0 };
    loop {
        let snap = reader.read().unwrap_or_else(|e| {
            eprintln!("media: {e}");
            // The session went away mid-read: ask for a fresh manager next time.
            reader.manager = None;
            nothing_playing()
        });
        *LATEST.lock().unwrap() = Some(snap);
        match commands.recv_timeout(Duration::from_secs(1)) {
            Ok(action) => reader.command(&action),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

thread_local! {
    /// Widgets showing the media session (UI thread).
    static IDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Values the widgets show now, for widgets added later.
    static SHOWN: RefCell<Vec<(&'static str, String)>> = const { RefCell::new(Vec::new()) };
}

fn set_all(key: &'static str, value: &str) {
    IDS.with_borrow(|ids| ids.iter().for_each(|id| widget_engine::set(id, key, value)));
    SHOWN.with_borrow_mut(|s| match s.iter_mut().find(|(k, _)| *k == key) {
        Some(kv) => kv.1 = value.into(),
        None => s.push((key, value.into())),
    });
}

/// UI thread: copies a new snapshot into the widgets.
extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
    let Some(snap) = LATEST.lock().unwrap().take() else { return };
    let mut playing = PLAYING.lock().unwrap();
    let seeking = SEEKED.lock().unwrap().is_some_and(|at| at.elapsed() < HOLD);
    for (k, v) in &snap.values {
        if seeking && matches!(*k, "bar" | "elapsed" | "remaining") {
            continue;
        }
        if *k == "playing" {
            let on = !v.is_empty();
            match playing.1 {
                Some((want, at)) if want != on && at.elapsed() < HOLD => continue,
                _ => *playing = (on, None),
            }
        }
        set_all(k, v);
    }
}

/// UI thread: flips the play/pause icon right away.
fn toggle_icon() {
    let mut playing = PLAYING.lock().unwrap();
    let on = !playing.0;
    *playing = (on, Some((on, Instant::now())));
    set_all("playing", if on { "on" } else { "" });
}

/// Starts reading the media session. Call once, on the engine's thread.
pub fn start() {
    if COMMANDS.get().is_some() {
        return;
    }
    let (tx, rx) = channel();
    let _ = COMMANDS.set(Mutex::new(tx));
    std::thread::spawn(move || worker(rx));
    SHOWN.set(nothing_playing().values);
    // Short period: button presses show up quickly. Nothing redraws unless a value changed.
    unsafe { SetTimer(None, 0, 200, Some(tick)) };
}

/// Feeds the widget `id` (a widgets/media package) from now on.
pub fn add(id: &str) {
    IDS.with_borrow_mut(|ids| ids.push(id.into()));
    SHOWN.with_borrow(|s| s.iter().for_each(|(k, v)| widget_engine::set(id, k, v)));
}

pub fn remove(id: &str) {
    IDS.with_borrow_mut(|ids| ids.retain(|i| i != id));
}

/// Handles the widget's buttons and its bar (`seek:<fraction>`). False: not a media action.
pub fn action(action: &str) -> bool {
    match action {
        "previous" | "next" => {}
        "playpause" => toggle_icon(),
        _ => {
            let Some(f) = action.strip_prefix("seek:").and_then(|f| f.parse::<f32>().ok()) else { return false };
            // The bar stays where it was let go until the app catches up.
            set_all("bar", &format!("{:.2}", f.clamp(0.0, 1.0) * 100.0));
            *SEEKED.lock().unwrap() = Some(Instant::now());
        }
    }
    if let Some(c) = COMMANDS.get() {
        let _ = c.lock().unwrap().send(action.to_string());
    }
    true
}

/// Made-up values for a gallery preview.
pub const SAMPLE: &[(&str, &str)] = &[
    ("title", "Midnight City Lights"),
    ("artist", "The Quiet Test Band"),
    ("playing", "on"),
    ("controls", "1"),
    ("timed", "1"),
    ("elapsed", "1:18"),
    ("remaining", "-2:15"),
    ("position", "0.37"),
    ("bar", "37"),
    ("art", "assets/sample-art.svg"),
];
