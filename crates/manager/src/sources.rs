//! `[[source]]` values from widget.toml: a URL (its first line, a JSON path, or an RSS/Atom
//! feed) or a local file, fetched on a worker thread and set as `src.<key>` for `{src.<key>}`.
//! Packages hold no code: there is no command source.
//!
//! The last values are kept in `data_dir()\sources.txt`, so widgets show data at start and
//! offline.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use widgine::Source;

/// (widget id, package id, sources, when each was last fetched).
type Watched = (String, String, Vec<Source>, Vec<Option<Instant>>);
static WATCHED: Mutex<Vec<Watched>> = Mutex::new(Vec::new());
static STARTED: std::sync::Once = std::sync::Once::new();

/// Feeds widget `id` when its package has sources. Call on the engine's thread.
pub fn add(id: &str, info: &widgine::PackageInfo) {
    if info.sources.is_empty() {
        return;
    }
    // The last values, until the first fetch.
    for (key, value) in cached(&info.id) {
        widgine::set(id, &key, &value);
    }
    let n = info.sources.len();
    WATCHED.lock().unwrap().push((id.into(), info.id.clone(), info.sources.clone(), vec![None; n]));
    STARTED.call_once(|| {
        std::thread::spawn(worker);
    });
}

pub fn remove(id: &str) {
    WATCHED.lock().unwrap().retain(|w| w.0 != id);
}

fn worker() {
    loop {
        let now = Instant::now();
        // Due sources, resolved: (widget id, package id, source, url or file).
        let mut due = Vec::new();
        for (id, package, sources, last) in WATCHED.lock().unwrap().iter_mut() {
            for (s, t) in sources.iter().zip(last.iter_mut()) {
                if t.is_none_or(|t| now.duration_since(t) >= every(s)) {
                    *t = Some(now);
                    let target = if s.url.is_empty() { s.file.clone() } else { with_options(&s.url, id) };
                    due.push((id.clone(), package.clone(), s.clone(), target));
                }
            }
        }
        // One fetch per address per round, however many widgets use it.
        let mut bodies: HashMap<String, Option<String>> = HashMap::new();
        for (id, package, s, target) in due {
            let body = bodies.entry(target.clone()).or_insert_with(|| if s.url.is_empty() { std::fs::read_to_string(&target).ok() } else { crate::feeds::curl(&target) });
            let Some(body) = body.as_deref() else { continue };
            for (key, value) in values(&s, body) {
                crate::feeds::publish(&id, &key, &value);
                remember(&package, &key, &value);
            }
        }
        std::thread::sleep(Duration::from_secs(15));
    }
}

/// "5m" -> 5 minutes. Default 15 minutes, at least one minute; files every 15 seconds.
fn every(s: &Source) -> Duration {
    if !s.file.is_empty() && s.every.is_empty() {
        return Duration::from_secs(15);
    }
    let (n, unit) = s.every.split_at(s.every.len().saturating_sub(1));
    let secs = n.parse::<u64>().ok().and_then(|n| Some(n * [("s", 1), ("m", 60), ("h", 3600), ("d", 86400)].iter().find(|(u, _)| *u == unit)?.1)).unwrap_or(900);
    Duration::from_secs(secs.max(if s.file.is_empty() { 60 } else { 15 }))
}

/// `{opt.symbol}` in a URL: the widget's option value, URL-encoded.
fn with_options(url: &str, id: &str) -> String {
    widgine_interpolate(url, &|k| k.strip_prefix("opt.").map(|o| encode(&crate::option(id, o).unwrap_or_default())))
}

/// `{key}` -> lookup(key); other text stays.
fn widgine_interpolate(s: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        out += &rest[..i];
        match rest[i..].find('}') {
            Some(j) => {
                let key = &rest[i + 1..i + j];
                out += &lookup(key).unwrap_or_else(|| format!("{{{key}}}"));
                rest = &rest[i + j + 1..];
            }
            None => {
                out += &rest[i..];
                rest = "";
            }
        }
    }
    out + rest
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The (key, value) pairs a source's answer gives.
fn values(s: &Source, body: &str) -> Vec<(String, String)> {
    let key = format!("src.{}", s.key);
    if s.rss {
        let mut out = Vec::new();
        for (n, (title, link)) in feed_items(body).into_iter().take(10).enumerate() {
            out.push((format!("{key}.{}.title", n + 1), title));
            out.push((format!("{key}.{}.link", n + 1), link));
        }
        return out;
    }
    let value = if s.json.is_empty() {
        body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(200).collect()
    } else {
        match serde_json::from_str::<serde_json::Value>(body).ok().as_ref().and_then(|v| json_path(v, &s.json)) {
            Some(serde_json::Value::String(t)) => t.clone(),
            Some(serde_json::Value::Null) | None => return Vec::new(),
            Some(v) => v.to_string(),
        }
    };
    vec![(key, value)]
}

/// "data.items[0].price" in `v`.
fn json_path<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = v;
    for part in path.split('.').filter(|p| !p.is_empty()) {
        let (name, rest) = part.split_once('[').map_or((part, ""), |(n, r)| (n, r));
        if !name.is_empty() {
            cur = cur.get(name)?;
        }
        for index in rest.split('[') {
            if let Ok(i) = index.trim_end_matches(']').parse::<usize>() {
                cur = cur.get(i)?;
            }
        }
    }
    Some(cur)
}

/// (title, link) of each RSS `<item>` or Atom `<entry>`.
// ponytail: tag scanning, not an XML parser; fine for the feeds people subscribe to.
fn feed_items(xml: &str) -> Vec<(String, String)> {
    let tag = |s: &str, name: &str| -> Option<String> {
        let start = s.find(&format!("<{name}"))?;
        let open_end = start + s[start..].find('>')?;
        let head = &s[start..open_end];
        // Atom: <link href="..."/>.
        if let Some(h) = head.find("href=\"") {
            let v = &head[h + 6..];
            return Some(v[..v.find('"')?].to_string());
        }
        let body = &s[open_end + 1..];
        let end = body.find(&format!("</{name}>"))?;
        Some(text(&body[..end]))
    };
    let mut out = Vec::new();
    for (open, close) in [("<item", "</item>"), ("<entry", "</entry>")] {
        let mut rest = xml;
        while let Some(i) = rest.find(open) {
            let Some(j) = rest[i..].find(close) else { break };
            let item = &rest[i..i + j];
            out.push((tag(item, "title").unwrap_or_default(), tag(item, "link").unwrap_or_default()));
            rest = &rest[i + j..];
        }
    }
    out
}

/// Element text: CDATA unwrapped, basic entities decoded, tags dropped.
fn text(s: &str) -> String {
    let s = s.trim();
    let s = s.strip_prefix("<![CDATA[").and_then(|x| x.strip_suffix("]]>")).unwrap_or(s);
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode(&out).trim().to_string()
}

/// `&amp;`, `&#39;`, `&#x27;` and the other XML entities.
fn decode(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out += &rest[..i];
        let after = &rest[i + 1..];
        let end = after.find(';').filter(|&e| e <= 8);
        let c = end.and_then(|e| match &after[..e] {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            n => n.strip_prefix("#x").or_else(|| n.strip_prefix("#X")).map(|h| u32::from_str_radix(h, 16)).or_else(|| n.strip_prefix('#').map(|d| d.parse::<u32>())).and_then(|r| r.ok()).and_then(char::from_u32),
        });
        match (c, end) {
            (Some(c), Some(e)) => {
                out.push(c);
                rest = &after[e + 1..];
            }
            _ => {
                out.push('&');
                rest = after;
            }
        }
    }
    out + rest
}

fn cache_file() -> PathBuf {
    crate::data_dir().join("sources.txt")
}

/// Last values: lines of package id, key, value, tab-separated.
static CACHE: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);

fn with_cache<R>(f: impl FnOnce(&mut HashMap<(String, String), String>) -> R) -> R {
    let mut c = CACHE.lock().unwrap();
    let map = c.get_or_insert_with(|| {
        std::fs::read_to_string(cache_file())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                Some(((f.next()?.to_string(), f.next()?.to_string()), f.next()?.to_string()))
            })
            .collect()
    });
    f(map)
}

fn cached(package: &str) -> Vec<(String, String)> {
    with_cache(|m| m.iter().filter(|((p, _), _)| p == package).map(|((_, k), v)| (k.clone(), v.clone())).collect())
}

fn remember(package: &str, key: &str, value: &str) {
    let changed = with_cache(|m| m.insert((package.into(), key.into()), value.into()).as_deref() != Some(value));
    if changed {
        let text: String = with_cache(|m| m.iter().map(|((p, k), v)| format!("{p}\t{k}\t{}\n", v.replace(['\t', '\n', '\r'], " "))).collect());
        let _ = std::fs::write(cache_file(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(json: &str, rss: bool) -> Source {
        Source { key: "k".into(), url: "https://x".into(), file: String::new(), json: json.into(), rss, every: String::new() }
    }

    #[test]
    fn json_values() {
        let body = r#"{"data":{"amount":"97000.5","items":[{"n":1},{"n":2}]},"stars":42}"#;
        assert_eq!(values(&src("data.amount", false), body), vec![("src.k".into(), "97000.5".into())]);
        assert_eq!(values(&src("data.items[1].n", false), body), vec![("src.k".into(), "2".into())]);
        assert_eq!(values(&src("stars", false), body), vec![("src.k".into(), "42".into())]);
        assert!(values(&src("nope", false), body).is_empty());
        assert_eq!(values(&src("", false), "\n  hello \nworld"), vec![("src.k".into(), "hello".into())]);
    }

    #[test]
    fn feeds() {
        let rss = "<rss><channel><title>T</title><item><title><![CDATA[First & one]]></title><link>https://a</link></item><item><title>Second &amp; two</title><link>https://b</link></item></channel></rss>";
        let v = values(&src("", true), rss);
        assert_eq!(v[0], ("src.k.1.title".into(), "First & one".into()));
        assert_eq!(v[3], ("src.k.2.link".into(), "https://b".into()));
        let atom = r#"<feed><entry><title>A</title><link href="https://c"/></entry></feed>"#;
        assert_eq!(feed_items(atom), vec![("A".into(), "https://c".into())]);
        assert_eq!(decode("We&#x27;ve &#39;x&#39; &amp; a & b &bogus;"), "We've 'x' & a & b &bogus;");
    }

    #[test]
    fn intervals_and_urls() {
        let mut s = src("", false);
        assert_eq!(every(&s), Duration::from_secs(900));
        s.every = "5m".into();
        assert_eq!(every(&s), Duration::from_secs(300));
        s.every = "10s".into();
        assert_eq!(every(&s), Duration::from_secs(60));
        assert_eq!(encode("BTC USD"), "BTC%20USD");
        assert_eq!(widgine_interpolate("a{x}b{y}", &|k| (k == "x").then(|| "1".into())), "a1b{y}");
    }
}
