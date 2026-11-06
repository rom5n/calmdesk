//! Live data for the built-in widgets that show this PC: battery, radios, photos, notes, tasks,
//! weather, world clock and screen time. A worker thread reads it; the UI thread copies it into the widgets, because
//! `widgine::set` belongs to the engine's thread.

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::Foundation::{FILETIME, HWND, SYSTEMTIME};
use windows::Win32::Globalization::{GetDateFormatEx, GetUserDefaultGeoName, ENUM_DATE_FORMATS_FLAGS};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::SystemInformation::{GetLocalTime, GetSystemTime, GetTickCount};
use windows::Win32::System::Time::{
    DYNAMIC_TIME_ZONE_INFORMATION, EnumDynamicTimeZoneInformation, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTimeEx,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::SetTimer;
use windows::core::w;

type Values = Vec<(String, String)>;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// Battery, Wi-Fi, Bluetooth, energy saver, airplane mode: every system widget gets all.
    System,
    Photo,
    /// Characters of the note that fit.
    Notes(usize),
    /// Task rows the layout has.
    Tasks(usize),
    Weather,
    World,
    Usage,
    /// Processor, memory and graphics load.
    Monitor,
    /// Battery levels of Bluetooth devices.
    Devices,
    Currency,
    /// Event rows the layout has.
    Events(usize),
    /// Clock, weather and load in turns.
    Stack,
}

fn kind(package_id: &str) -> Option<Kind> {
    let name = package_id.strip_prefix("dev.widget-engine.")?;
    Some(match name {
        "battery" | "battery-wide" | "connectivity" | "wifi" | "bluetooth" | "energy-saver" | "airplane" => Kind::System,
        n if n.starts_with("photo") => Kind::Photo,
        "notes-small" => Kind::Notes(110),
        "notes-medium" => Kind::Notes(260),
        "notes-large" => Kind::Notes(680),
        "tasks-small" => Kind::Tasks(4),
        "tasks-large" => Kind::Tasks(9),
        "weather" | "weather-square" => Kind::Weather,
        "clock-world" => Kind::World,
        "screen-time" | "screen-time-square" => Kind::Usage,
        "system-monitor" => Kind::Monitor,
        "bluetooth-battery" => Kind::Devices,
        "currency" => Kind::Currency,
        "events" => Kind::Events(3),
        "smart-stack" => Kind::Stack,
        _ => return None,
    })
}

enum Msg {
    Start,
    Add(String, Kind),
    Remove(String),
    Action(String, String),
}

static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
/// (widget id, key, value) from the worker, for the UI thread.
static OUT: Mutex<Vec<(String, String, String)>> = Mutex::new(Vec::new());

fn send(m: Msg) {
    if TX.get().is_none() {
        let (tx, rx) = channel();
        let _ = TX.set(Mutex::new(tx));
        std::thread::spawn(move || worker(rx));
        unsafe { SetTimer(None, 0, 250, Some(tick)) };
    }
    let _ = TX.get().unwrap().lock().unwrap().send(m);
}

/// UI thread: hands the new values to the widgets.
extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
    for (id, k, v) in std::mem::take(&mut *OUT.lock().unwrap()) {
        widgine::set(&id, &k, &v);
    }
}

/// Starts the worker: it counts screen time from now on.
pub fn start() {
    send(Msg::Start);
}

/// Feeds the widget `id` if its package is one of ours. Call on the engine's thread.
pub fn add(id: &str, package_id: &str) {
    if crate::timers::add(id, package_id) {
        return;
    }
    if let Some(k) = kind(package_id) {
        send(Msg::Add(id.into(), k));
    }
}

pub fn remove(id: &str) {
    crate::timers::remove(id);
    if TX.get().is_some() {
        send(Msg::Remove(id.into()));
    }
}

/// Clicks in our widgets. False: not ours.
pub fn action(id: &str, action: &str) -> bool {
    if crate::timers::action(id, action) {
        return true;
    }
    let ours = matches!(action, "wifi" | "bluetooth" | "saver" | "airplane" | "edit" | "page") || action.starts_with("task:");
    if ours {
        send(Msg::Action(id.into(), action.into()));
    }
    ours
}

/// What the gallery previews show.
pub fn sample(package_id: &str) -> Values {
    let own = |v: &[(&str, &str)]| v.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Values>();
    let tr = widgine::tr;
    match package_id.strip_prefix("dev.widget-engine.").unwrap_or("") {
        "timer" => return own(&[("time", "05:00"), ("dash", "263.9"), ("color", "#FF9500"), ("focus", "on")]),
        "pomodoro" => return own(&[("time", "25:00"), ("dash", "263.9"), ("color", "#FF6347"), ("focus", "on")]),
        "stopwatch" => return own(&[("time", "00:00.0")]),
        _ => {}
    }
    match kind(package_id) {
        Some(Kind::System) => own(&[
            ("battery", "76"),
            ("battery_dash", "200.6"),
            ("battery_w", "38"),
            ("battery_color", "#34C759"),
            ("percent", "76%"),
            ("unplugged", "on"),
            ("wifi", "on"),
            ("bt", "on"),
        ]),
        Some(Kind::Notes(_)) => vec![("text".into(), ["Pick up flowers", "Call Anna at six", "Book tickets for Friday"].map(tr).join("\n"))],
        Some(Kind::Tasks(_)) => own(&[
            ("t0", tr("Buy groceries")),
            ("t1", tr("Reply to emails")),
            ("d1", "on"),
            ("t2", tr("Water the plants")),
            ("t3", tr("Go for a run")),
            ("count", "3"),
        ]),
        Some(Kind::Weather) => {
            let mut v = own(&[
            ("city", tr("Paris")),
            ("ready", "on"),
            ("temp", "18°"),
            ("icon", "partly"),
            ("k_partly", "on"),
            ("hi", "21°"),
            ("lo", "12°"),
            ("i1", "sun"),
            ("h1", "22°"),
            ("i2", "cloud"),
            ("h2", "19°"),
            ("i3", "rain"),
            ("h3", "16°"),
            ("i4", "partly"),
            ("h4", "18°"),
            ]);
            // Friday to Monday in the user's language.
            for (i, d) in ["2026-11-06", "2026-11-07", "2026-11-08", "2026-11-09"].iter().enumerate() {
                v.push((format!("w{}", i + 1), weekday(d)));
            }
            v
        }
        Some(Kind::World) => world_values(""),
        Some(Kind::Monitor) => own(&[("cpu", "23"), ("cpu_t", "23%"), ("ram", "58"), ("ram_t", "58%"), ("gpu", "12"), ("gpu_t", "12%")]),
        Some(Kind::Devices) => own(&[("n0", "Headphones"), ("p0", "80"), ("b0", "80"), ("n1", "Mouse"), ("p1", "45"), ("b1", "45"), ("n2", "Keyboard"), ("p2", "100"), ("b2", "100")]),
        Some(Kind::Currency) => own(&[("base", "USD"), ("c0", "EUR"), ("r0", "0.8910"), ("c1", "RUB"), ("r1", "81.8"), ("c2", "GBP"), ("r2", "0.7430"), ("c3", "CNY"), ("r3", "6.67")]),
        Some(Kind::Events(_)) => {
            let at = |day: &str, h: &str, t: &str| crate::events::Event { start: format!("{day}{h}"), all_day: h == "0000", title: tr(t).into() };
            let mut v = crate::events::values(&[
                at("20261106", "0930", "Team stand-up"),
                at("20261106", "1300", "Lunch with Anna"),
                at("20261107", "0000", "Mom's birthday"),
            ]);
            v.push(("linked".into(), "on".into()));
            v
        }
        Some(Kind::Stack) => own(&[("p0", "on")]),
        Some(Kind::Usage) => {
            let minutes = [0, 0, 0, 0, 0, 0, 0, 0, 12, 38, 52, 45, 20, 34, 58, 41, 30, 0, 0, 0, 0, 0, 0, 0];
            usage_values(&minutes.map(|m| m * 60_000))
        }
        _ => Vec::new(),
    }
}

fn worker(rx: Receiver<Msg>) {
    // WinRT (radios) needs an apartment.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let mut w = Worker::default();
    loop {
        w.refresh();
        let first = match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        w.handle(first);
        while let Ok(m) = rx.try_recv() {
            w.handle(m);
        }
    }
}

#[derive(Default)]
struct Worker {
    widgets: Vec<(String, Kind)>,
    /// What each widget already has: only changes go out.
    sent: HashMap<(String, String), String>,
    /// Photos of each folder, and when they were found.
    photos: HashMap<PathBuf, (Vec<PathBuf>, Instant)>,
    /// Photo shown by each widget, and since when.
    shown: HashMap<String, (PathBuf, Instant)>,
    /// Forecast of each city ("auto": the IP address's), when it was fetched, and whether the
    /// last try failed (it is then the one saved on disk, or none).
    weather: HashMap<String, (Values, Instant, bool)>,
    usage: Option<Usage>,
    load: crate::sensors::Load,
    /// Bluetooth battery levels, and when they were read.
    devices: Option<(Values, Instant)>,
    /// Rates for each base currency, and when they were fetched.
    rates: HashMap<String, (Values, Instant)>,
    /// Each events widget's events, when fetched, and from which links.
    events: HashMap<String, (Vec<crate::events::Event>, Instant, String)>,
    /// Each smart stack's page, and since when.
    pages: HashMap<String, (usize, Instant)>,
}

const PHOTO_EVERY: Duration = Duration::from_secs(15 * 60);
const WEATHER_EVERY: Duration = Duration::from_secs(30 * 60);
const DEVICES_EVERY: Duration = Duration::from_secs(60);
const RATES_EVERY: Duration = Duration::from_secs(60 * 60);
const EVENTS_EVERY: Duration = Duration::from_secs(30 * 60);
const PAGE_EVERY: Duration = Duration::from_secs(15);
const PAGES: usize = 3;

impl Worker {
    fn handle(&mut self, m: Msg) {
        match m {
            Msg::Start => {}
            Msg::Add(id, k) => {
                self.remove(&id);
                self.widgets.push((id, k));
            }
            Msg::Remove(id) => self.remove(&id),
            Msg::Action(id, a) => self.action(&id, &a),
        }
    }

    fn remove(&mut self, id: &str) {
        self.widgets.retain(|(i, _)| i != id);
        self.sent.retain(|(i, _), _| i != id);
        self.shown.remove(id);
        self.events.remove(id);
        self.pages.remove(id);
    }

    fn refresh(&mut self) {
        self.count_usage();
        let mut system = None;
        let mut load = None;
        let mut out = Vec::new();
        for (id, kind) in self.widgets.clone() {
            let values = match kind {
                Kind::System => system.get_or_insert_with(system_values).clone(),
                Kind::Photo => self.photo(&id),
                Kind::Notes(limit) => vec![("text".into(), note_text(&file(&id, "notes"), limit))],
                Kind::Tasks(rows) => task_values(&std::fs::read_to_string(file(&id, "tasks")).unwrap_or_default(), rows),
                Kind::Weather => self.weather(&id),
                Kind::World => world_values(&id),
                Kind::Usage => usage_values(&self.usage.as_ref().map_or([0; 24], |u| u.ms)),
                Kind::Monitor => load.get_or_insert_with(|| self.load_values()).clone(),
                Kind::Devices => self.devices(),
                Kind::Currency => self.rates(&id),
                Kind::Events(rows) => self.events(&id, rows),
                Kind::Stack => {
                    let mut v = self.page(&id);
                    v.extend(self.weather(&id));
                    v.extend(load.get_or_insert_with(|| self.load_values()).clone());
                    v
                }
            };
            for (k, v) in values {
                let key = (id.clone(), k);
                if self.sent.get(&key) != Some(&v) {
                    out.push((id.clone(), key.1.clone(), v.clone()));
                    self.sent.insert(key, v);
                }
            }
        }
        if !out.is_empty() {
            OUT.lock().unwrap().extend(out);
        }
    }

    /// A photo from the folder picked in the widget's menu; none until one is picked (the
    /// widget then says how).
    fn photo(&mut self, id: &str) -> Values {
        let Some(dir) = crate::option(id, "folder").map(PathBuf::from) else {
            self.shown.remove(id);
            return vec![("photo".into(), String::new())];
        };
        if self.photos.get(&dir).is_none_or(|(_, t)| t.elapsed() > PHOTO_EVERY) {
            self.photos.insert(dir.clone(), (find_photos(&dir), Instant::now()));
        }
        let list = &self.photos[&dir].0;
        if list.is_empty() {
            self.shown.remove(id);
        } else if self.shown.get(id).is_none_or(|(p, t)| t.elapsed() > PHOTO_EVERY || !p.starts_with(&dir)) {
            let n = std::collections::hash_map::RandomState::new().hash_one(id) as usize;
            self.shown.insert(id.into(), (list[n % list.len()].clone(), Instant::now()));
        }
        let path = self.shown.get(id).map(|(p, _)| p.display().to_string()).unwrap_or_default();
        vec![("photo".into(), path)]
    }

    fn weather(&mut self, id: &str) -> Values {
        let city = crate::option(id, "city").unwrap_or_else(|| "auto".into());
        let due = self.weather.get(&city).is_none_or(|(_, t, _)| t.elapsed() > WEATHER_EVERY);
        if due {
            match fetch_weather(&city) {
                Some(v) => {
                    save_weather(&city, &v);
                    self.weather.insert(city.clone(), (v, Instant::now(), false));
                }
                // Offline: the last forecast, from disk after a restart. Try again in two minutes,
                // or in 15 seconds with nothing to show.
                None => {
                    let mut old = self.weather.remove(&city).map(|(v, ..)| v).filter(|v| !v.is_empty()).unwrap_or_else(|| load_weather(&city));
                    // Saved in the language of that time: the city's name in today's.
                    let name = city.rsplit('/').next().unwrap_or(&city);
                    if let Some(&(known, ..)) = WEATHER_CITIES.iter().find(|c| c.0 == name) {
                        old.iter_mut().filter(|(k, _)| k == "city").for_each(|(_, v)| *v = widgine::tr(known).to_string());
                    }
                    let wait = Duration::from_secs(if old.is_empty() { 15 } else { 120 });
                    let at = Instant::now().checked_sub(WEATHER_EVERY - wait).unwrap_or_else(Instant::now);
                    self.weather.insert(city.clone(), (old, at, true));
                }
            }
        }
        let (v, _, offline) = &self.weather[&city];
        let mut v = v.clone();
        let some = !v.is_empty();
        v.push(("ready".into(), on(true)));
        v.push(("nonet".into(), on(!some && *offline)));
        v.push(("stale".into(), on(some && *offline)));
        v
    }

    fn load_values(&mut self) -> Values {
        let (cpu, ram, gpu, _) = self.load.read();
        let pct = |p: f32| format!("{}%", p.round() as u32);
        let mut v = vec![
            ("cpu".into(), format!("{cpu:.0}")),
            ("cpu_t".into(), pct(cpu)),
            ("ram".into(), format!("{ram:.0}")),
            ("ram_t".into(), pct(ram)),
        ];
        // No counter (old drivers, virtual machines): the row hides.
        v.push(("gpu".into(), gpu.map_or(String::new(), |g| format!("{g:.0}"))));
        v.push(("gpu_t".into(), gpu.map_or(String::new(), pct)));
        v
    }

    fn devices(&mut self) -> Values {
        if self.devices.as_ref().is_none_or(|(_, t)| t.elapsed() > DEVICES_EVERY) {
            let found = crate::sensors::bluetooth_batteries();
            let mut v = Values::new();
            for i in 0..3 {
                let (name, level) = found.get(i).map_or((String::new(), String::new()), |(n, l)| (n.clone(), l.to_string()));
                v.push((format!("n{i}"), name));
                v.push((format!("p{i}"), level.clone()));
                v.push((format!("b{i}"), level));
            }
            self.devices = Some((v, Instant::now()));
        }
        self.devices.as_ref().unwrap().0.clone()
    }

    /// Rates of the widget's base currency; the last ones while offline.
    fn rates(&mut self, id: &str) -> Values {
        let base = crate::option(id, "base").unwrap_or_else(|| "USD".into());
        if self.rates.get(&base).is_none_or(|(_, t)| t.elapsed() > RATES_EVERY) {
            let fresh = fetch_rates(&base);
            let old = self.rates.remove(&base).map(|(v, _)| v).unwrap_or_default();
            // Offline: try again in five minutes.
            let at = if fresh.is_some() { Instant::now() } else { Instant::now().checked_sub(RATES_EVERY - Duration::from_secs(300)).unwrap_or_else(Instant::now) };
            self.rates.insert(base.clone(), (fresh.unwrap_or(old), at));
        }
        let mut v = vec![("base".to_string(), base.clone())];
        v.extend(self.rates[&base].0.clone());
        v
    }

    /// The next events of the links in the widget's text file.
    fn events(&mut self, id: &str, rows: usize) -> Values {
        let links = std::fs::read_to_string(file(id, "events")).unwrap_or_default();
        let stale = self.events.get(id).is_none_or(|(_, t, l)| *l != links || t.elapsed() > EVENTS_EVERY);
        if stale {
            let mut all = Vec::new();
            let mut failed = false;
            for url in links.lines().map(str::trim).filter(|l| l.contains("://")) {
                let url = url.replacen("webcal://", "https://", 1);
                match curl(&url) {
                    Some(text) if text.contains("BEGIN:VCALENDAR") => all.extend(crate::events::parse(&text)),
                    _ => failed = true,
                }
            }
            // Offline: keep the events from before.
            if failed && all.is_empty() {
                if let Some((old, _, _)) = self.events.remove(id) {
                    all = old;
                }
            }
            self.events.insert(id.into(), (all, Instant::now(), links.clone()));
        }
        let t = unsafe { GetLocalTime() };
        let now = format!("{:04}{:02}{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute);
        let next = crate::events::upcoming(self.events[id].0.clone(), &now, rows);
        let linked = links.contains("://");
        let mut v = crate::events::values(&next);
        // Rows that are empty now: cleared, not left from before.
        for i in next.len()..rows {
            v.push((format!("e{i}"), String::new()));
        }
        v.push(("linked".into(), on(linked)));
        v.push(("empty".into(), on(linked && next.is_empty())));
        v
    }

    /// The smart stack's page flags `p0`..: the next page every 15 seconds, unless turned off.
    fn page(&mut self, id: &str) -> Values {
        let rotate = crate::option(id, "rotate").as_deref() != Some("off");
        let (page, since) = self.pages.entry(id.into()).or_insert((0, Instant::now()));
        if rotate && since.elapsed() > PAGE_EVERY {
            *page = (*page + 1) % PAGES;
            *since = Instant::now();
        }
        (0..PAGES).map(|i| (format!("p{i}"), on(i == *page))).collect()
    }

    /// Adds the time since the last call to this hour, if someone used the PC lately.
    fn count_usage(&mut self) {
        let now = unsafe { GetLocalTime() };
        let today = (now.wYear, now.wMonth, now.wDay);
        let u = self.usage.get_or_insert_with(|| Usage::load(today));
        if u.day != today {
            *u = Usage::new(today);
        }
        // Sleep and hibernation are not use: count at most a few seconds a step.
        let step = u.last.elapsed().min(Duration::from_secs(10));
        u.last = Instant::now();
        if idle() < IDLE {
            u.ms[now.wHour as usize % 24] += step.as_millis() as u64;
        }
        if u.saved.elapsed() > Duration::from_secs(60) {
            u.save();
        }
    }

    fn action(&mut self, id: &str, action: &str) {
        match action {
            "wifi" if !toggle_radio(RadioKind::WiFi) => open("ms-settings:network-wifi"),
            "bluetooth" if !toggle_radio(RadioKind::Bluetooth) => open("ms-settings:bluetooth"),
            // No public API switches these: their Settings pages do.
            "saver" => open("ms-settings:batterysaver"),
            "airplane" => open("ms-settings:network-airplanemode"),
            "page" => {
                let (page, since) = self.pages.entry(id.into()).or_insert((0, Instant::now()));
                *page = (*page + 1) % PAGES;
                *since = Instant::now();
            }
            "edit" => {
                let dir = match self.widgets.iter().find(|(i, _)| i == id).map(|(_, k)| *k) {
                    Some(Kind::Tasks(_)) => "tasks",
                    Some(Kind::Events(_)) => "events",
                    _ => "notes",
                };
                let path = file(id, dir);
                if !path.exists() {
                    let _ = std::fs::create_dir_all(path.parent().unwrap());
                    let _ = std::fs::write(&path, "");
                }
                let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
            }
            a => {
                if let Some(n) = a.strip_prefix("task:").and_then(|n| n.parse().ok()) {
                    let path = file(id, "tasks");
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let _ = std::fs::write(&path, toggle_task(&text, n));
                }
            }
        }
    }
}

/// No input for this long: the PC is not in use.
const IDLE: Duration = Duration::from_secs(5 * 60);

fn idle() -> Duration {
    let mut li = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut li) }.as_bool() {
        return Duration::ZERO;
    }
    Duration::from_millis(unsafe { GetTickCount() }.wrapping_sub(li.dwTime) as u64)
}

/// Today's screen time by hour, kept in `usage.txt` as "2026-11-05 ms0 ms1 .. ms23".
struct Usage {
    day: (u16, u16, u16),
    ms: [u64; 24],
    last: Instant,
    saved: Instant,
}

impl Usage {
    fn new(day: (u16, u16, u16)) -> Self {
        Usage { day, ms: [0; 24], last: Instant::now(), saved: Instant::now() }
    }

    fn path() -> PathBuf {
        crate::data_dir().join("usage.txt")
    }

    fn load(day: (u16, u16, u16)) -> Self {
        let mut u = Usage::new(day);
        let text = std::fs::read_to_string(Self::path()).unwrap_or_default();
        let mut parts = text.split_whitespace();
        if parts.next() == Some(&format!("{:04}-{:02}-{:02}", day.0, day.1, day.2)) {
            for (slot, v) in u.ms.iter_mut().zip(parts) {
                *slot = v.parse().unwrap_or(0);
            }
        }
        u
    }

    fn save(&mut self) {
        self.saved = Instant::now();
        let ms: Vec<String> = self.ms.iter().map(u64::to_string).collect();
        let _ = std::fs::create_dir_all(crate::data_dir());
        let _ = std::fs::write(Self::path(), format!("{:04}-{:02}-{:02} {}\n", self.day.0, self.day.1, self.day.2, ms.join(" ")));
    }
}

/// `hours` ("" under an hour), `mins`, and bar heights `b0`..`b23` with their tops `y0`..
/// (assets/bars.svg: 56 units for a full hour).
fn usage_values(ms: &[u64; 24]) -> Values {
    let total = ms.iter().sum::<u64>() / 60_000;
    let mut v: Values = vec![
        ("hours".into(), if total >= 60 { (total / 60).to_string() } else { String::new() }),
        ("mins".into(), (total % 60).to_string()),
    ];
    for (i, m) in ms.iter().enumerate() {
        let h = (*m as f32 / 60_000.0).min(60.0) * 56.0 / 60.0;
        v.push((format!("b{i}"), format!("{h:.1}")));
        v.push((format!("y{i}"), format!("{:.1}", 56.0 - h)));
    }
    v
}

/// Cities of the world clock (its widget.toml lists the same) and their Windows time zones.
const CITIES: [(&str, &str); 15] = [
    ("Los Angeles", "Pacific Standard Time"),
    ("New York", "Eastern Standard Time"),
    ("São Paulo", "E. South America Standard Time"),
    ("London", "GMT Standard Time"),
    ("Paris", "Romance Standard Time"),
    ("Berlin", "W. Europe Standard Time"),
    ("Kyiv", "FLE Standard Time"),
    ("Istanbul", "Turkey Standard Time"),
    ("Moscow", "Russian Standard Time"),
    ("Dubai", "Arabian Standard Time"),
    ("New Delhi", "India Standard Time"),
    ("Bangkok", "SE Asia Standard Time"),
    ("Beijing", "China Standard Time"),
    ("Tokyo", "Tokyo Standard Time"),
    ("Sydney", "AUS Eastern Standard Time"),
];
const DEFAULT_CITIES: [&str; 4] = ["New Delhi", "Dubai", "London", "Paris"];

fn zone(key: &str) -> Option<DYNAMIC_TIME_ZONE_INFORMATION> {
    thread_local! {
        static ZONES: std::cell::RefCell<HashMap<String, Option<DYNAMIC_TIME_ZONE_INFORMATION>>> = Default::default();
    }
    ZONES.with_borrow_mut(|zones| {
        *zones.entry(key.into()).or_insert_with(|| {
            (0..).map_while(|i| {
                let mut z = DYNAMIC_TIME_ZONE_INFORMATION::default();
                (unsafe { EnumDynamicTimeZoneInformation(i, &mut z) } == 0).then_some(z)
            })
            .find(|z| String::from_utf16_lossy(&z.TimeZoneKeyName).split('\0').next() == Some(key))
        })
    })
}

/// Minutes since 1601, for differences.
fn minutes(st: &SYSTEMTIME) -> i64 {
    let mut ft = FILETIME::default();
    let _ = unsafe { SystemTimeToFileTime(st, &mut ft) };
    ((((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64) / 600_000_000) as i64
}

/// Per city `i` of the four: hand angles `a<i>`, `m<i>`, name `n<i>`, day `d<i>`, offset `o<i>`.
fn world_values(id: &str) -> Values {
    let utc = unsafe { GetSystemTime() };
    let local = unsafe { GetLocalTime() };
    let mut v = Values::new();
    for (i, default) in DEFAULT_CITIES.iter().enumerate() {
        let picked = crate::option(id, &format!("city{i}"));
        let (city, key) = CITIES.iter().find(|(c, _)| Some(*c) == picked.as_deref()).or_else(|| CITIES.iter().find(|(c, _)| c == default)).unwrap();
        let mut t = local;
        if let Some(z) = zone(key) {
            let _ = unsafe { SystemTimeToTzSpecificLocalTimeEx(Some(&z), &utc, &mut t) };
        }
        // Both clocks were read a moment apart: zones differ by whole quarter hours.
        let off = ((minutes(&t) - minutes(&local)) as f32 / 15.0).round() as i64 * 15;
        let (h, m, s) = (t.wHour as f32, t.wMinute as f32, t.wSecond as f32 + t.wMilliseconds as f32 / 1000.0);
        let day = match ((t.wYear, t.wMonth, t.wDay).cmp(&(local.wYear, local.wMonth, local.wDay)), off > 0) {
            (std::cmp::Ordering::Equal, _) => "Today",
            (_, true) => "Tomorrow",
            _ => "Yesterday",
        };
        let sign = if off < 0 { '−' } else { '+' };
        v.push((format!("a{i}"), format!("{:.1}", (h % 12.0) * 30.0 + m * 0.5 + s / 120.0)));
        v.push((format!("m{i}"), format!("{:.1}", m * 6.0 + s * 0.1)));
        v.push((format!("n{i}"), widgine::tr(city).into()));
        v.push((format!("d{i}"), widgine::tr(day).into()));
        v.push((format!("o{i}"), format!("{sign}{}:{:02}", off.abs() / 60, off.abs() % 60)));
    }
    v
}

/// `%LOCALAPPDATA%\calmdesk\<dir>\<widget id>.txt`.
fn file(id: &str, dir: &str) -> PathBuf {
    crate::data_dir().join(dir).join(format!("{id}.txt"))
}

pub(crate) fn open(target: &str) {
    // explorer opens files with their default app and ms-settings: links in Settings.
    let _ = std::process::Command::new("explorer.exe").arg(target).spawn();
}

fn on(b: bool) -> String {
    if b { "on" } else { "" }.into()
}

fn system_values() -> Values {
    let mut s = SYSTEM_POWER_STATUS::default();
    let _ = unsafe { GetSystemPowerStatus(&mut s) };
    // 128: no battery (a desktop PC); 255: unknown.
    let battery = s.BatteryFlag != 128 && s.BatteryLifePercent <= 100;
    let level = if battery { s.BatteryLifePercent as f32 } else { 100.0 };
    let plugged = s.ACLineStatus == 1;
    let charging = battery && plugged && s.BatteryFlag & 8 != 0;
    let (wifi, bt) = radio_states();
    let v = [
        ("battery", format!("{level}")),
        // The ring in assets/ring.svg has r = 42: circumference 263.9.
        ("battery_dash", format!("{:.1}", level * 2.639)),
        // The bar in assets/battery.svg is 50 units wide.
        ("battery_w", format!("{:.1}", level * 0.5)),
        ("battery_color", if level <= 20.0 && !plugged { "#FF3B30" } else { "#34C759" }.into()),
        ("percent", format!("{level}%")),
        ("charging", on(charging)),
        ("plugged", on(plugged && !charging)),
        ("unplugged", on(!plugged)),
        // SystemStatusFlag 1: energy saver is on.
        ("saver", on(s.SystemStatusFlag == 1)),
        ("plane", on(airplane_mode())),
        ("wifi", on(wifi)),
        ("bt", on(bt)),
    ];
    v.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn airplane_mode() -> bool {
    let mut value = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!(r"SYSTEM\CurrentControlSet\Control\RadioManagement\SystemRadioState"),
            None,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    r.is_ok() && value == 1
}

fn radios() -> Vec<Radio> {
    let Ok(list) = Radio::GetRadiosAsync().and_then(|op| op.join()) else { return Vec::new() };
    (0..list.Size().unwrap_or(0)).filter_map(|i| list.GetAt(i).ok()).collect()
}

/// (Wi-Fi on, Bluetooth on).
fn radio_states() -> (bool, bool) {
    let mut st = (false, false);
    for r in radios() {
        let on = r.State().is_ok_and(|s| s == RadioState::On);
        match r.Kind() {
            Ok(RadioKind::WiFi) => st.0 |= on,
            Ok(RadioKind::Bluetooth) => st.1 |= on,
            _ => {}
        }
    }
    st
}

/// Turns the radio on or off. False when Windows does not let us (or there is none).
fn toggle_radio(kind: RadioKind) -> bool {
    let _ = Radio::RequestAccessAsync().and_then(|op| op.join());
    let Some(r) = radios().into_iter().find(|r| r.Kind().is_ok_and(|k| k == kind)) else { return false };
    let next = if r.State().is_ok_and(|s| s == RadioState::On) { RadioState::Off } else { RadioState::On };
    r.SetStateAsync(next).and_then(|op| op.join()).is_ok_and(|s| s == windows::Devices::Radios::RadioAccessStatus::Allowed)
}

/// Photos in `dir`, a few folders deep.
fn find_photos(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if out.len() >= 2000 {
                return;
            }
            if p.is_dir() {
                if depth > 0 {
                    walk(&p, depth - 1, out);
                }
            } else if p.extension().is_some_and(|x| matches!(x.to_ascii_lowercase().to_str(), Some("jpg" | "jpeg" | "png" | "bmp" | "webp")))
                && e.metadata().is_ok_and(|m| m.len() < 40 << 20)
            {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, 3, &mut out);
    out
}

/// The note, cut to what fits the widget.
fn note_text(path: &Path, limit: usize) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let text = text.trim();
    match text.char_indices().nth(limit) {
        Some((i, _)) => format!("{}…", text[..i].trim_end()),
        None => text.into(),
    }
}

/// A task line: "[x] done", "[ ] open" or just "open"; "- " in front is fine too.
fn parse_task(line: &str) -> Option<(bool, &str)> {
    let l = line.trim();
    let l = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).unwrap_or(l).trim();
    if l.is_empty() {
        return None;
    }
    for (p, done) in [("[x]", true), ("[X]", true), ("[ ]", false), ("[]", false)] {
        if let Some(rest) = l.strip_prefix(p) {
            return Some((done, rest.trim()));
        }
    }
    Some((false, l))
}

/// `t0`.. task texts, `d0`.. done flags, `count`: open tasks.
fn task_values(text: &str, rows: usize) -> Values {
    let tasks: Vec<(bool, &str)> = text.lines().filter_map(parse_task).collect();
    let mut v = Values::new();
    for i in 0..rows {
        let (done, t) = tasks.get(i).copied().unwrap_or((false, ""));
        v.push((format!("t{i}"), t.into()));
        v.push((format!("d{i}"), on(done)));
    }
    v.push(("count".into(), tasks.iter().filter(|(d, _)| !d).count().to_string()));
    v
}

/// Marks the `n`-th task done, or open again.
fn toggle_task(text: &str, n: usize) -> String {
    let mut k = 0;
    let lines: Vec<String> = text
        .lines()
        .map(|line| match parse_task(line) {
            Some((done, t)) => {
                k += 1;
                if k - 1 == n { format!("[{}] {t}", if done { ' ' } else { 'x' }) } else { line.into() }
            }
            None => line.into(),
        })
        .collect();
    lines.join("\r\n") + "\r\n"
}

/// The value after `"key":` in a JSON text: a string's content, an array's items, or a number.
// ponytail: no JSON parser for two known replies; switch to serde_json if more appear.
pub(crate) fn field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("\"{key}\":");
    let rest = json[json.find(&pat)? + pat.len()..].trim_start();
    if let Some(r) = rest.strip_prefix('"') {
        return r.split('"').next();
    }
    if let Some(r) = rest.strip_prefix('[') {
        return r.split(']').next();
    }
    rest.split([',', '}']).next().map(str::trim)
}

/// The text from the object `"name":{` on.
fn section<'a>(json: &'a str, name: &str) -> &'a str {
    json.find(&format!("\"{name}\":{{")).map_or("", |i| &json[i..])
}

pub(crate) fn curl(url: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("curl.exe").args(["-sL", "-m", "15", url]).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// One unit of `base` in the first four other currencies of the widget's list. Rates By
/// Exchange Rate API (open.er-api.com, no key, daily rates).
fn fetch_rates(base: &str) -> Option<Values> {
    const CODES: [&str; 9] = ["USD", "EUR", "RUB", "GBP", "CNY", "JPY", "CHF", "PLN", "CAD"];
    let to: Vec<&str> = CODES.iter().copied().filter(|c| *c != base).take(4).collect();
    let json = curl(&format!("https://open.er-api.com/v6/latest/{base}"))?;
    let rates = section(&json, "rates");
    let mut v = Values::new();
    for (i, code) in to.iter().enumerate() {
        let r: f64 = field(rates, code)?.parse().ok()?;
        v.push((format!("c{i}"), code.to_string()));
        v.push((format!("r{i}"), if r >= 100.0 { format!("{r:.1}") } else if r >= 1.0 { format!("{r:.2}") } else { format!("{r:.4}") }));
    }
    Some(v)
}

/// Icon file name in the weather package for a WMO weather code.
fn icon(code: u32, day: bool) -> &'static str {
    match code {
        0 | 1 if day => "sun",
        0 | 1 => "moon",
        2 => "partly",
        3 => "cloud",
        45 | 48 => "fog",
        71..=77 | 85 | 86 => "snow",
        95..=99 => "storm",
        _ if code >= 51 => "rain",
        _ => "cloud",
    }
}

/// Condition flags `k_<name>` the layout shows a translated label for.
const CONDITIONS: [&str; 7] = ["clear", "partly", "cloud", "fog", "rain", "snow", "storm"];

fn condition(icon: &str) -> &str {
    if matches!(icon, "sun" | "moon") { "clear" } else { icon }
}

fn fahrenheit() -> bool {
    let mut geo = [0u16; 8];
    let n = unsafe { GetUserDefaultGeoName(&mut geo) };
    String::from_utf16_lossy(&geo[..(n.max(1) - 1) as usize]) == "US"
}

/// "2026-11-06" -> short weekday name in the user's language.
fn weekday(date: &str) -> String {
    let mut parts = date.split('-').map(|p| p.parse::<u16>().unwrap_or(1));
    let st = SYSTEMTIME { wYear: parts.next().unwrap_or(2000), wMonth: parts.next().unwrap_or(1), wDay: parts.next().unwrap_or(1), ..Default::default() };
    let mut buf = [0u16; 32];
    let n = unsafe { GetDateFormatEx(widgine::locale(), ENUM_DATE_FORMATS_FLAGS(0), Some(&st), w!("ddd"), Some(&mut buf), None) };
    String::from_utf16_lossy(&buf[..(n.max(1) - 1) as usize])
}

/// Cities the weather widget offers (its widget.toml lists the same): name, latitude, longitude.
const WEATHER_CITIES: [(&str, f32, f32); 34] = [
    ("Los Angeles", 34.05, -118.24),
    ("Chicago", 41.88, -87.63),
    ("New York", 40.71, -74.01),
    ("Toronto", 43.65, -79.38),
    ("São Paulo", -23.55, -46.63),
    ("London", 51.51, -0.13),
    ("Amsterdam", 52.37, 4.90),
    ("Paris", 48.86, 2.35),
    ("Madrid", 40.42, -3.70),
    ("Rome", 41.90, 12.50),
    ("Berlin", 52.52, 13.40),
    ("Prague", 50.08, 14.44),
    ("Vienna", 48.21, 16.37),
    ("Warsaw", 52.23, 21.01),
    ("Lviv", 49.84, 24.03),
    ("Kyiv", 50.45, 30.52),
    ("Kharkiv", 49.99, 36.23),
    ("Odesa", 46.48, 30.72),
    ("Minsk", 53.90, 27.56),
    ("Istanbul", 41.01, 28.98),
    ("Saint Petersburg", 59.94, 30.31),
    ("Moscow", 55.76, 37.62),
    ("Kazan", 55.79, 49.12),
    ("Yekaterinburg", 56.84, 60.61),
    ("Novosibirsk", 55.03, 82.92),
    ("Almaty", 43.24, 76.89),
    ("Dubai", 25.20, 55.27),
    ("New Delhi", 28.61, 77.21),
    ("Bangkok", 13.76, 100.50),
    ("Singapore", 1.35, 103.82),
    ("Beijing", 39.90, 116.40),
    ("Seoul", 37.57, 126.98),
    ("Tokyo", 35.68, 139.69),
    ("Sydney", -33.87, 151.21),
];

/// Current weather and four more days in `city`, or ("auto") at this PC's IP address.
// ponytail: "auto" is the IP address's city (ipinfo.io); Windows location services if it is too coarse.
fn fetch_weather(city: &str) -> Option<Values> {
    // Values are "Europe/Paris": the menu groups them by region.
    let name = city.rsplit('/').next().unwrap_or(city);
    let (name, lat, lon) = match WEATHER_CITIES.iter().find(|c| c.0 == name) {
        Some(&(c, lat, lon)) => (widgine::tr(c).to_string(), lat.to_string(), lon.to_string()),
        None => {
            let geo = curl("https://ipinfo.io/json")?;
            let (lat, lon) = field(&geo, "loc")?.split_once(',')?;
            (field(&geo, "city").unwrap_or("").to_string(), lat.to_string(), lon.to_string())
        }
    };
    let unit = if fahrenheit() { "&temperature_unit=fahrenheit" } else { "" };
    let json = curl(&format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m,weather_code,is_day\
         &daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=auto&forecast_days=5{unit}"
    ))?;
    weather_values(&name, &json)
}

/// The last forecast of `city`, kept for when the PC is offline: `key<TAB>value` lines.
fn weather_file(city: &str) -> PathBuf {
    let name: String = city.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    crate::data_dir().join("weather").join(format!("{name}.txt"))
}

fn save_weather(city: &str, v: &Values) {
    let path = weather_file(city);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(path, v.iter().map(|(k, x)| format!("{k}\t{x}\n")).collect::<String>());
}

fn load_weather(city: &str) -> Values {
    let text = std::fs::read_to_string(weather_file(city)).unwrap_or_default();
    text.lines().filter_map(|l| l.split_once('\t')).map(|(k, v)| (k.into(), v.into())).collect()
}

fn weather_values(city: &str, json: &str) -> Option<Values> {
    let deg = |s: &str| s.trim().parse::<f32>().map(|t| format!("{}°", t.round() as i32)).unwrap_or_default();
    let cur = section(json, "current");
    let code: u32 = field(cur, "weather_code")?.parse().ok()?;
    let now = icon(code, field(cur, "is_day") != Some("0"));
    let daily = section(json, "daily");
    let list = |k: &str| field(daily, k).map(|s| s.split(',').map(|x| x.trim().trim_matches('"').to_string()).collect()).unwrap_or_else(Vec::<String>::new);
    let (dates, codes, max, min) = (list("time"), list("weather_code"), list("temperature_2m_max"), list("temperature_2m_min"));
    let at = |l: &[String], i: usize| l.get(i).cloned().unwrap_or_default();
    let mut v: Values = vec![
        ("city".into(), city.into()),
        ("temp".into(), deg(field(cur, "temperature_2m")?)),
        ("icon".into(), now.into()),
        ("hi".into(), deg(&at(&max, 0))),
        ("lo".into(), deg(&at(&min, 0))),
    ];
    for c in CONDITIONS {
        v.push((format!("k_{c}"), on(condition(now) == c)));
    }
    for i in 1..5 {
        let code = at(&codes, i).parse().unwrap_or(3);
        v.push((format!("w{i}"), weekday(&at(&dates, i))));
        v.push((format!("i{i}"), icon(code, true).into()));
        v.push((format!("h{i}"), deg(&at(&max, i))));
        v.push((format!("l{i}"), deg(&at(&min, i))));
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_and_usage() {
        let v: HashMap<String, String> = world_values("").into_iter().collect();
        assert!(v["o0"].ends_with(":30"), "New Delhi: {}", v["o0"]);
        let mut ms = [0; 24];
        ms[9] = 80 * 60_000;
        ms[10] = 30 * 60_000;
        let u: HashMap<String, String> = usage_values(&ms).into_iter().collect();
        assert_eq!((u["hours"].as_str(), u["mins"].as_str()), ("1", "50"));
        assert_eq!((u["b9"].as_str(), u["y9"].as_str(), u["b10"].as_str()), ("56.0", "0.0", "28.0"));
    }

    #[test]
    fn tasks() {
        let text = "- [ ] milk\n\n[x] mail\nplants\n";
        let v = task_values(text, 4);
        assert_eq!(v[0], ("t0".into(), "milk".into()));
        assert_eq!(v[3], ("d1".into(), "on".into()));
        assert_eq!(v[4], ("t2".into(), "plants".into()));
        assert_eq!(v.last().unwrap(), &("count".to_string(), "2".to_string()));
        assert_eq!(toggle_task(text, 2), "- [ ] milk\r\n\r\n[x] mail\r\n[x] plants\r\n");
        assert_eq!(toggle_task(text, 1), "- [ ] milk\r\n\r\n[ ] mail\r\nplants\r\n");
    }

    #[test]
    fn weather_reply() {
        let json = r#"{"current_units":{"temperature_2m":"°C"},"current":{"time":"2026-11-05T18:00","temperature_2m":11.6,"weather_code":61,"is_day":0},
            "daily_units":{"time":"iso8601"},"daily":{"time":["2026-11-05","2026-11-06","2026-11-07","2026-11-08","2026-11-09"],
            "weather_code":[61,0,3,71,95],"temperature_2m_max":[13.2,14.0,9.5,2.1,-0.4],"temperature_2m_min":[7.1,6.0,4.4,-3.6,-5.0]}}"#;
        let v: HashMap<String, String> = weather_values("Kyiv", json).unwrap().into_iter().collect();
        assert_eq!(v["temp"], "12°");
        assert_eq!(v["icon"], "rain");
        assert_eq!(v["k_rain"], "on");
        assert_eq!(v["k_clear"], "");
        assert_eq!((v["hi"].as_str(), v["lo"].as_str()), ("13°", "7°"));
        assert_eq!((v["i1"].as_str(), v["i3"].as_str(), v["i4"].as_str()), ("sun", "snow", "storm"));
        assert_eq!(v["h4"], "0°");
        assert!(!v["w1"].is_empty());
        assert_eq!(field(r#"{"city":"Kyiv","loc":"50.45,30.52"}"#, "loc"), Some("50.45,30.52"));
    }
}
