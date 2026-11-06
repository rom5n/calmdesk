//! The calendar events widget: upcoming events of iCalendar (.ics) links, such as the secret
//! address Google Calendar and Outlook give for a calendar.

use windows::Win32::Foundation::SYSTEMTIME;
use windows::Win32::Globalization::{ENUM_DATE_FORMATS_FLAGS, GetDateFormatEx};
use windows::Win32::System::Time::SystemTimeToTzSpecificLocalTime;
use windows::core::w;

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Local start, "YYYYMMDDHHMM": sorts by time. All-day events start at "0000".
    pub start: String,
    pub all_day: bool,
    pub title: String,
}

/// The events of an iCalendar text.
// ponytail: one start per event; repeating events (RRULE) show only their first date, and a
// TZID start is taken as local time. A proper iCalendar crate if users miss them.
pub fn parse(ics: &str) -> Vec<Event> {
    // Long lines continue on lines that start with a space or tab.
    let text = ics.replace("\r\n", "\n").replace("\n ", "").replace("\n\t", "");
    let mut out = Vec::new();
    let (mut start, mut all_day, mut title, mut inside) = (None, false, String::new(), false);
    for line in text.lines() {
        let Some((name, value)) = line.split_once(':') else { continue };
        let (key, params) = name.split_once(';').unwrap_or((name, ""));
        match key {
            "BEGIN" if value == "VEVENT" => (start, title, inside) = (None, String::new(), true),
            "END" if value == "VEVENT" && inside => {
                inside = false;
                if let Some(s) = start.take() {
                    out.push(Event { start: s, all_day, title: std::mem::take(&mut title) });
                }
            }
            "DTSTART" if inside => {
                all_day = params.contains("VALUE=DATE") && !params.contains("DATE-TIME") || value.len() == 8;
                start = local(value);
            }
            "SUMMARY" if inside => title = value.replace("\\n", " ").replace("\\,", ",").replace("\\;", ";").replace("\\\\", "\\"),
            _ => {}
        }
    }
    out
}

/// "20261106T090000Z" (UTC), "20261106T090000" (local) or "20261106" -> local "YYYYMMDDHHMM".
fn local(v: &str) -> Option<String> {
    let num = |a: usize, b: usize| v.get(a..b).and_then(|s| s.parse::<u16>().ok());
    let (y, mo, d) = (num(0, 4)?, num(4, 6)?, num(6, 8)?);
    if v.len() < 13 {
        return Some(format!("{y:04}{mo:02}{d:02}0000"));
    }
    let (h, mi) = (num(9, 11)?, num(11, 13)?);
    if !v.ends_with('Z') {
        return Some(format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}"));
    }
    let utc = SYSTEMTIME { wYear: y, wMonth: mo, wDay: d, wHour: h, wMinute: mi, ..Default::default() };
    let mut t = SYSTEMTIME::default();
    unsafe { SystemTimeToTzSpecificLocalTime(None, &utc, &mut t) }.ok()?;
    Some(format!("{:04}{:02}{:02}{:02}{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute))
}

/// The next `rows` events from `now` ("YYYYMMDDHHMM"); all-day ones until their day ends.
pub fn upcoming(mut events: Vec<Event>, now: &str, rows: usize) -> Vec<Event> {
    events.retain(|e| if e.all_day { e.start[..8] >= now[..8] } else { e.start.as_str() >= now });
    events.sort_by(|a, b| a.start.cmp(&b.start));
    events.truncate(rows);
    events
}

/// The widget's values: per row `e<i>` title, `d<i>` day ("Fri, 6 Nov" in the user's
/// language), `h<i>` time ("" all day); `any` when there is one.
pub fn values(events: &[Event]) -> Vec<(String, String)> {
    let mut v = vec![("any".to_string(), if events.is_empty() { String::new() } else { "on".into() })];
    for (i, e) in events.iter().enumerate() {
        v.push((format!("e{i}"), e.title.clone()));
        v.push((format!("d{i}"), day(&e.start)));
        v.push((format!("h{i}"), if e.all_day { String::new() } else { format!("{}:{}", &e.start[8..10], &e.start[10..12]) }));
    }
    v
}

fn day(start: &str) -> String {
    let n = |a: usize, b: usize| start[a..b].parse().unwrap_or(1);
    let st = SYSTEMTIME { wYear: n(0, 4), wMonth: n(4, 6), wDay: n(6, 8), ..Default::default() };
    let mut buf = [0u16; 40];
    let len = unsafe { GetDateFormatEx(widgine::locale(), ENUM_DATE_FORMATS_FLAGS(0), Some(&st), w!("ddd, d MMM"), Some(&mut buf), None) };
    String::from_utf16_lossy(&buf[..(len.max(1) - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_events() {
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART;TZID=Europe/Kyiv:20261106T093000\r\nSUMMARY:Stand-up\\, team\r\nEND:VEVENT\r\n\
                   BEGIN:VEVENT\r\nDTSTART;VALUE=DATE:20261107\r\nSUMMARY:Birth\r\n day\r\nEND:VEVENT\r\n\
                   BEGIN:VEVENT\r\nDTSTART:20261101T080000\r\nSUMMARY:Past\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let e = parse(ics);
        assert_eq!(e[0], Event { start: "202611060930".into(), all_day: false, title: "Stand-up, team".into() });
        assert_eq!(e[1], Event { start: "202611070000".into(), all_day: true, title: "Birthday".into() });
        let next = upcoming(e, "202611060800", 4);
        assert_eq!(next.len(), 2);
        let v: std::collections::HashMap<_, _> = values(&next).into_iter().collect();
        assert_eq!((v["h0"].as_str(), v["h1"].as_str(), v["any"].as_str()), ("09:30", "", "on"));
    }
}
