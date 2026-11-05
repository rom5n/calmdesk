//! Timer, stopwatch and pomodoro widgets, counted on the UI thread ten times a second.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONASTERISK, SetTimer};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Timer,
    Stopwatch,
    Pomodoro,
}

struct Clock {
    id: String,
    kind: Kind,
    /// Running since; None: paused.
    since: Option<Instant>,
    /// Counted before `since`.
    before: Duration,
    /// Countdown length (timer, pomodoro).
    total: Duration,
    /// Pomodoro: in a break.
    rest: bool,
}

impl Clock {
    fn elapsed(&self) -> Duration {
        self.before + self.since.map_or(Duration::ZERO, |t| t.elapsed())
    }

    fn pause(&mut self) {
        self.before = self.elapsed();
        self.since = None;
    }

    /// Back to the start, with the length the user picks in the menu now.
    fn reset(&mut self) {
        self.since = None;
        self.before = Duration::ZERO;
        let (key, default) = match (self.kind, self.rest) {
            (Kind::Timer, _) => ("minutes", 5),
            (Kind::Pomodoro, false) => ("focus", 25),
            (Kind::Pomodoro, true) => ("break", 5),
            (Kind::Stopwatch, _) => return,
        };
        let m = crate::option(&self.id, key).and_then(|v| v.parse().ok()).unwrap_or(default);
        self.total = Duration::from_secs(m * 60);
    }

    fn values(&self) -> Vec<(&'static str, String)> {
        let running = ("running", if self.since.is_some() { "on" } else { "" }.to_string());
        if self.kind == Kind::Stopwatch {
            return vec![("time", stopwatch_text(self.elapsed())), running];
        }
        let left = self.total.saturating_sub(self.elapsed());
        let part = if self.total.is_zero() { 0.0 } else { left.as_secs_f32() / self.total.as_secs_f32() };
        let color = match (self.kind, self.rest) {
            (Kind::Timer, _) => "#FF9500",
            (_, false) => "#FF6347",
            (_, true) => "#34C759",
        };
        vec![
            // Rounded up: 4:59.2 left shows 5:00 until a whole second has gone.
            ("time", countdown_text(left.as_millis().div_ceil(1000) as u64)),
            // The ring in assets/ring.svg has r = 42: circumference 263.9.
            ("dash", format!("{:.1}", part * 263.9)),
            ("color", color.into()),
            ("focus", if self.rest { "" } else { "on" }.into()),
            running,
        ]
    }
}

fn countdown_text(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

fn stopwatch_text(d: Duration) -> String {
    let t = d.as_millis() as u64 / 100;
    let (h, m, s, tenth) = (t / 36000, t / 600 % 60, t / 10 % 60, t % 10);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}.{tenth}") }
}

thread_local! {
    static CLOCKS: RefCell<Vec<Clock>> = const { RefCell::new(Vec::new()) };
    /// What each widget already shows: only changes go out.
    static SENT: RefCell<HashMap<(String, &'static str), String>> = RefCell::new(HashMap::new());
    static TICKS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Counts for the widget `id` if its package is one of ours. True: ours.
pub fn add(id: &str, package_id: &str) -> bool {
    let kind = match package_id {
        "dev.widget-engine.timer" => Kind::Timer,
        "dev.widget-engine.stopwatch" => Kind::Stopwatch,
        "dev.widget-engine.pomodoro" => Kind::Pomodoro,
        _ => return false,
    };
    remove(id);
    let mut c = Clock { id: id.into(), kind, since: None, before: Duration::ZERO, total: Duration::ZERO, rest: false };
    c.reset();
    show(&c);
    CLOCKS.with_borrow_mut(|v| {
        if v.is_empty() {
            unsafe { SetTimer(None, 0, 100, Some(tick)) };
        }
        v.push(c)
    });
    true
}

pub fn remove(id: &str) {
    CLOCKS.with_borrow_mut(|v| v.retain(|c| c.id != id));
    SENT.with_borrow_mut(|s| s.retain(|(i, _), _| i != id));
}

/// `timer:toggle`, `timer:reset`, `timer:skip`. False: not ours.
pub fn action(id: &str, action: &str) -> bool {
    let Some(a) = action.strip_prefix("timer:") else { return false };
    CLOCKS.with_borrow_mut(|v| {
        let Some(c) = v.iter_mut().find(|c| c.id == id) else { return };
        match a {
            "toggle" if c.since.is_some() => c.pause(),
            "toggle" => {
                // A finished countdown starts over; one never started takes the length picked now.
                if c.before.is_zero() || (c.kind != Kind::Stopwatch && c.elapsed() >= c.total) {
                    c.reset();
                }
                c.since = Some(Instant::now());
            }
            "reset" => c.reset(),
            "skip" => {
                c.rest = !c.rest;
                c.reset();
            }
            _ => {}
        }
        show(c);
    });
    true
}

fn show(c: &Clock) {
    SENT.with_borrow_mut(|sent| {
        for (k, v) in c.values() {
            if sent.get(&(c.id.clone(), k)) != Some(&v) {
                widget_engine::set(&c.id, k, &v);
                sent.insert((c.id.clone(), k), v);
            }
        }
    });
}

extern "system" fn tick(_: HWND, _: u32, _: usize, _: u32) {
    // Once a second, idle countdowns pick up a new length from the menu.
    let n = TICKS.get().wrapping_add(1);
    TICKS.set(n);
    CLOCKS.with_borrow_mut(|v| {
        for c in v.iter_mut() {
            if c.since.is_some() && c.kind != Kind::Stopwatch && c.elapsed() >= c.total {
                unsafe { _ = MessageBeep(MB_ICONASTERISK) };
                if c.kind == Kind::Pomodoro {
                    // Focus done: a break is next (and the other way round), started by the user.
                    c.rest = !c.rest;
                    c.reset();
                } else {
                    c.pause();
                }
            } else if n % 10 == 0 && c.since.is_none() && c.before.is_zero() {
                c.reset();
            }
            show(c);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts() {
        assert_eq!(countdown_text(300), "05:00");
        assert_eq!(countdown_text(3725), "1:02:05");
        assert_eq!(stopwatch_text(Duration::from_millis(83_456)), "01:23.4");
        assert_eq!(stopwatch_text(Duration::from_secs(3601)), "1:00:01");
    }
}
