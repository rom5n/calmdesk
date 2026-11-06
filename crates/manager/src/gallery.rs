//! The widget gallery: a panel with the categories on the left and live previews of the
//! widgets on the right. Click a widget to add it, or drag it out onto the desktop.

use std::cell::RefCell;
use std::rc::Rc;

use widgine::windows::Win32::Graphics::Direct2D::Common::*;
use widgine::windows::Win32::Graphics::Direct2D::*;
use widgine::windows::Win32::Graphics::DirectWrite::*;
use widgine::windows::core::Result;
use widgine::windows_numerics::Vector2;
use widgine::{Background, Card, Customization, Event, LayerId, Scene, Widget, rgba};

use crate::catalog::{Entry, Kind};

pub const ID: &str = "gallery";
pub const SIZE: (f32, f32) = (800.0, 540.0);

/// Layout, DIPs.
const SIDE: f32 = 200.0;
const ROW: f32 = 34.0;
const LIST_TOP: f32 = 64.0;
/// Room for a group caption ("Built-in", "Other apps", "Community") above its categories.
const CAPTION: f32 = 28.0;
/// The search field above the categories, as wide as their rows: left, top, right, bottom.
const SEARCH: (f32, f32, f32, f32) = (14.0, 20.0, SIDE - 14.0, 48.0);
const CONTENT_X: f32 = SIDE + 24.0;
const GRID_TOP: f32 = 64.0;
const BOTTOM: f32 = SIZE.1 - 20.0;
const COLUMNS: usize = 3;
const GAP: f32 = 16.0;
const TILE_H: f32 = 170.0;
const HEADER_H: f32 = 34.0;
const CLOSE: (f32, f32, f32) = (SIZE.0 - 30.0, 30.0, 12.0);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Category(usize),
    /// Index into the entries.
    Tile(usize),
    Close,
    Search,
    /// Clears the search.
    Clear,
}

enum Line {
    Header(String),
    Tiles(Vec<usize>),
}

impl Line {
    fn height(&self) -> f32 {
        match self {
            Line::Header(_) => HEADER_H,
            Line::Tiles(_) => TILE_H + GAP,
        }
    }
}

pub struct Gallery {
    entries: Rc<RefCell<Vec<Entry>>>,
    /// Adds a widget: (entry, true = follow the pointer, false = place it).
    add: Rc<dyn Fn(&Entry, bool)>,
    /// 0: all widgets. Else a category, 1-based in `categories()`.
    selected: usize,
    /// First visible line of the grid.
    first: usize,
    hover: Option<Hit>,
    press: Option<(Hit, f32, f32)>,
    hits: Vec<(D2D_RECT_F, Hit)>,
    base: Option<LayerId>,
    /// The last line is visible: no scrolling further down.
    at_end: bool,
    /// Typed into the search field. The panel has the keyboard while it is open.
    query: String,
}

impl Gallery {
    pub fn new(entries: Rc<RefCell<Vec<Entry>>>, add: Rc<dyn Fn(&Entry, bool)>) -> Self {
        Self { entries, add, selected: 0, first: 0, hover: None, press: None, hits: Vec::new(), base: None, at_end: true, query: String::new() }
    }

    /// (group, category, widget count): grouped by where the widgets come from, then by name.
    fn categories(&self) -> Vec<(Kind, String, usize)> {
        let mut cats: Vec<(Kind, String, usize)> = Vec::new();
        for e in self.entries.borrow().iter() {
            let c = e.category();
            match cats.iter_mut().find(|(k, n, _)| *k == e.kind && *n == c) {
                Some(x) => x.2 += 1,
                None => cats.push((e.kind, c, 1)),
            }
        }
        cats.sort_by_key(|(k, n, _)| (*k, n.to_lowercase()));
        cats
    }

    /// Top of each sidebar row ("All widgets", then `cats`), the group captions (top, label),
    /// and the row height. No captions while every widget is built-in. Rows get shorter when
    /// they would not fit.
    // ponytail: shrinks instead of scrolling; add sidebar scrolling if rows get too thin.
    fn sidebar(cats: &[(Kind, String, usize)]) -> (Vec<f32>, Vec<(f32, &'static str)>, f32) {
        let grouped = cats.iter().any(|c| c.0 != Kind::Builtin);
        let groups = if grouped { cats.windows(2).filter(|w| w[0].0 != w[1].0).count() + 1 } else { 0 };
        let need = (cats.len() + 1) as f32 * ROW + groups as f32 * CAPTION;
        let k = ((BOTTOM - LIST_TOP) / need).min(1.0);
        let (row, caption) = (ROW * k, CAPTION * k);
        let mut tops = vec![LIST_TOP];
        let mut captions = Vec::new();
        let mut y = LIST_TOP + row;
        for (i, (kind, ..)) in cats.iter().enumerate() {
            if grouped && (i == 0 || cats[i - 1].0 != *kind) {
                captions.push((y, group_label(*kind)));
                y += caption;
            }
            tops.push(y);
            y += row;
        }
        (tops, captions, row)
    }

    /// Shown by the search: the name, category, variant or source contains the query.
    fn matches(&self, e: &Entry) -> bool {
        let q = self.query.to_lowercase();
        [e.info.name.as_str(), &e.category(), &e.info.variant, &e.source].iter().any(|s| s.to_lowercase().contains(&q))
    }

    /// Grid lines for the selected category: per-category headers in "All widgets".
    fn lines(&self) -> Vec<Line> {
        let entries = self.entries.borrow();
        let cats = self.categories();
        let mut lines = Vec::new();
        let grouped = cats.iter().any(|c| c.0 != Kind::Builtin);
        for (k, (kind, cat, _)) in cats.iter().enumerate() {
            if self.selected != 0 && self.selected != k + 1 {
                continue;
            }
            let ids: Vec<usize> = (0..entries.len()).filter(|&i| entries[i].kind == *kind && entries[i].category() == *cat && self.matches(&entries[i])).collect();
            if ids.is_empty() {
                continue;
            }
            if self.selected == 0 {
                lines.push(Line::Header(if grouped && *kind != Kind::Builtin { format!("{} · {cat}", group_label(*kind)) } else { cat.clone() }));
            }
            lines.extend(ids.chunks(COLUMNS).map(|c| Line::Tiles(c.to_vec())));
        }
        lines
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        let (cx, cy, r) = CLOSE;
        if (x - cx).hypot(y - cy) <= r + 4.0 {
            return Some(Hit::Close);
        }
        self.hits.iter().find(|(rc, _)| x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom).map(|(_, h)| *h)
    }

    fn tile_w() -> f32 {
        (SIZE.0 - CONTENT_X - 24.0 - GAP * (COLUMNS - 1) as f32) / COLUMNS as f32
    }

    fn draw_base(&self, scene: &mut Scene) -> Result<()> {
        let Some(layer) = self.base else { return Ok(()) };
        let theme = *scene.theme();
        let fg = theme.foreground();
        let ink = |a: f32| rgba(fg.r, fg.g, fg.b, a);
        let fill = |a: f32| if theme.dark { rgba(1.0, 1.0, 1.0, a) } else { rgba(0.0, 0.0, 0.0, a * 0.6) };
        let cats = self.categories();
        let lines = self.lines();
        let entries = self.entries.borrow();
        let fmt = |size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT| -> Result<IDWriteTextFormat> {
            let f = scene.text_format(size, Some(weight))?;
            unsafe {
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                f.SetTextAlignment(align)?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                let sign = scene.dwrite().CreateEllipsisTrimmingSign(&f)?;
                f.SetTrimming(&DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() }, &sign)?;
            }
            Ok(f)
        };
        let body = fmt(14.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let count = fmt(13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_TRAILING)?;
        let hint = fmt(13.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let header = fmt(15.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let name = fmt(13.0, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let small = fmt(11.0, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_ALIGNMENT_LEADING)?;
        let hover = self.hover;
        let d2d = scene.d2d().clone();
        let scene_dwrite = scene.dwrite().clone();
        scene.draw(layer, |ctx| unsafe {
            let brush = |c: D2D1_COLOR_F| ctx.CreateSolidColorBrush(&c, None);
            let text = |s: &str, f: &IDWriteTextFormat, l: f32, t: f32, r: f32, b: f32, c: D2D1_COLOR_F| -> Result<()> {
                let w: Vec<u16> = s.encode_utf16().collect();
                ctx.DrawText(&w, f, &rect(l, t, r, b), &brush(c)?, D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                Ok(())
            };
            let pill = |l: f32, t: f32, r: f32, b: f32, radius: f32, c: D2D1_COLOR_F| -> Result<()> {
                ctx.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(l, t, r, b), radiusX: radius, radiusY: radius }, &brush(c)?);
                Ok(())
            };

            // Sidebar: categories with icons, a hairline on its edge.
            ctx.FillRectangle(&rect(SIDE, 20.0, SIDE + 1.0, SIZE.1 - 20.0), &brush(ink(0.08))?);

            // Search field: a pill with a magnifier, the query and a caret.
            let (sl, st, sr, sb) = SEARCH;
            let a = if hover == Some(Hit::Search) { 0.1 } else { 0.065 };
            pill(sl, st, sr, sb, (sb - st) / 2.0, fill(a))?;
            let (mx, my) = (sl + 14.0, (st + sb) / 2.0);
            let round = D2D1_STROKE_STYLE_PROPERTIES1 { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, ..Default::default() };
            let stroke = d2d.CreateStrokeStyle(&round, None)?;
            let glass = brush(ink(0.5))?;
            ctx.DrawEllipse(&D2D1_ELLIPSE { point: Vector2 { X: mx - 1.0, Y: my - 1.0 }, radiusX: 4.6, radiusY: 4.6 }, &glass, 1.5, &stroke);
            ctx.DrawLine(Vector2 { X: mx + 2.6, Y: my + 2.6 }, Vector2 { X: mx + 5.8, Y: my + 5.8 }, &glass, 1.5, &stroke);
            let tx = sl + 26.0;
            let tr_ = if self.query.is_empty() { sr - 10.0 } else { sr - 26.0 };
            if self.query.is_empty() {
                text(widgine::tr("Search"), &hint, tx + 4.0, st, tr_, sb, ink(0.42))?;
            } else {
                text(&self.query, &hint, tx, st, tr_, sb, fg)?;
                // The clear button.
                let (cx, cy) = (sr - 14.0, my);
                ctx.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: 7.0, radiusY: 7.0 }, &brush(ink(if hover == Some(Hit::Clear) { 0.45 } else { 0.3 }))?);
                let k = 2.6;
                let xb = brush(if theme.dark { rgba(0.0, 0.0, 0.0, 0.8) } else { rgba(1.0, 1.0, 1.0, 1.0) })?;
                ctx.DrawLine(Vector2 { X: cx - k, Y: cy - k }, Vector2 { X: cx + k, Y: cy + k }, &xb, 1.4, &stroke);
                ctx.DrawLine(Vector2 { X: cx + k, Y: cy - k }, Vector2 { X: cx - k, Y: cy + k }, &xb, 1.4, &stroke);
            }
            let w: Vec<u16> = self.query.encode_utf16().collect();
            let layout = scene_dwrite.CreateTextLayout(&w, &hint, tr_ - tx, sb - st)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            let cx = (tx + m.widthIncludingTrailingWhitespace + 1.0).min(tr_);
            ctx.FillRectangle(&rect(cx, my - 8.0, cx + 1.4, my + 8.0), &brush(theme.accent)?);
            // Featured widgets are copies: count each widget once.
            let total = entries.iter().filter(|e| e.kind != Kind::Featured).count();
            let (tops, captions, row) = Self::sidebar(&cats);
            for (t, label) in &captions {
                let h = row / ROW * CAPTION;
                text(label, &small, 26.0, *t + h - 22.0, SIDE - 26.0, *t + h - 4.0, ink(0.45))?;
            }
            let rows = std::iter::once((widgine::tr("All widgets").to_string(), total)).chain(cats.iter().map(|(_, c, n)| (c.clone(), *n)));
            for (k, (label, n)) in rows.enumerate() {
                let t = tops[k];
                let selected = k == self.selected;
                if selected {
                    pill(14.0, t, SIDE - 14.0, t + row - 2.0, 10.0, theme.accent)?;
                } else if hover == Some(Hit::Category(k)) {
                    pill(14.0, t, SIDE - 14.0, t + row - 2.0, 10.0, fill(0.08))?;
                }
                let c = if selected { rgba(1.0, 1.0, 1.0, 1.0) } else { fg };
                let c2 = if selected { rgba(1.0, 1.0, 1.0, 0.8) } else { ink(0.45) };
                category_icon(ctx, &d2d, if k == 0 { "" } else { &label }, 37.0, t + (row - 2.0) / 2.0, &brush(c)?)?;
                text(&label, &body, 54.0, t, SIDE - 52.0, t + row - 2.0, c)?;
                text(&n.to_string(), &count, SIDE - 60.0, t, SIDE - 26.0, t + row - 2.0, c2)?;
                // A short hairline under the label, not next to a highlighted row.
                let lit = |r: usize| r == self.selected || hover == Some(Hit::Category(r));
                let last_of_group = k < cats.len() && tops[k + 1] > t + row + 0.5;
                if k < cats.len() && !last_of_group && !lit(k) && !lit(k + 1) {
                    ctx.FillRectangle(&rect(54.0, t + row - 1.5, SIDE - 26.0, t + row - 0.5), &brush(ink(0.08))?);
                }
            }

            // Header: hint and the close button.
            text(widgine::tr("Drag a widget to your desktop, or click to add it."), &hint, CONTENT_X, 18.0, SIZE.0 - 60.0, 44.0, ink(0.5))?;
            let (cx, cy, r) = CLOSE;
            let ring = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: r, radiusY: r };
            ctx.FillEllipse(&ring, &brush(fill(if hover == Some(Hit::Close) { 0.16 } else { 0.08 }))?);
            let stroke = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES1 { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, ..Default::default() },
                None,
            )?;
            let k = 3.6;
            let xb = brush(ink(0.62))?;
            ctx.DrawLine(Vector2 { X: cx - k, Y: cy - k }, Vector2 { X: cx + k, Y: cy + k }, &xb, 1.6, &stroke);
            ctx.DrawLine(Vector2 { X: cx + k, Y: cy - k }, Vector2 { X: cx - k, Y: cy + k }, &xb, 1.6, &stroke);

            // Grid: tile backgrounds and labels. The previews go on top, see `build`.
            let tw = Self::tile_w();
            if lines.is_empty() {
                text(widgine::tr("No widgets found"), &hint, CONTENT_X + 2.0, GRID_TOP, SIZE.0 - 24.0, GRID_TOP + 30.0, ink(0.5))?;
            }
            let mut y = GRID_TOP;
            for line in lines.iter().skip(self.first) {
                if y + line.height() - GAP.min(line.height()) > BOTTOM {
                    break;
                }
                match line {
                    Line::Header(h) => text(h, &header, CONTENT_X + 2.0, y, SIZE.0 - 24.0, y + HEADER_H - 6.0, fg)?,
                    Line::Tiles(ids) => {
                        for (c, &i) in ids.iter().enumerate() {
                            let x = CONTENT_X + c as f32 * (tw + GAP);
                            let a = if hover == Some(Hit::Tile(i)) { 0.12 } else { 0.06 };
                            pill(x, y, x + tw, y + TILE_H, 18.0, fill(a))?;
                            let e = &entries[i];
                            text(&e.info.name, &name, x + 14.0, y + TILE_H - 44.0, x + tw - 12.0, y + TILE_H - 24.0, fg)?;
                            // The variant tells similar tiles apart; else where it comes from.
                            // Store widgets show their author and downloads instead.
                            let sub = if e.info.variant.is_empty() || e.store.is_some() { &e.source } else { &e.info.variant };
                            text(sub, &small, x + 14.0, y + TILE_H - 26.0, x + tw - 12.0, y + TILE_H - 10.0, ink(0.5))?;
                        }
                    }
                }
                y += line.height();
            }
            Ok(())
        })
    }
}

impl Widget for Gallery {
    fn size(&self) -> (f32, f32) {
        SIZE
    }

    fn customization(&self) -> Customization {
        Customization {
            card: Some(Card { radius: 30.0, background: Background::Frosted, border: true, shadow: true }),
            ..Default::default()
        }
    }

    fn interactive(&self, x: f32, y: f32) -> bool {
        self.hit_at(x, y).is_some()
    }

    fn build(&mut self, scene: &mut Scene) -> Result<()> {
        self.base = Some(scene.add_layer(0.0, 0.0, SIZE.0, SIZE.1)?);
        let cats = self.categories();
        self.selected = self.selected.min(cats.len());
        // Hit areas and previews of what is visible.
        let (tops, _, row) = Self::sidebar(&cats);
        self.hits = tops.into_iter().enumerate().map(|(k, t)| (rect(14.0, t, SIDE - 14.0, t + row - 2.0), Hit::Category(k))).collect();
        let (sl, st, sr, sb) = SEARCH;
        if !self.query.is_empty() {
            self.hits.push((rect(sr - 24.0, st, sr, sb), Hit::Clear));
        }
        self.hits.push((rect(sl, st, sr, sb), Hit::Search));
        let lines = self.lines();
        self.first = self.first.min(lines.len().saturating_sub(1));
        let entries = self.entries.borrow().clone();
        let tw = Self::tile_w();
        let mut y = GRID_TOP;
        let mut last = self.first;
        for (n, line) in lines.iter().enumerate().skip(self.first) {
            if y + line.height() - GAP.min(line.height()) > BOTTOM {
                break;
            }
            last = n;
            if let Line::Tiles(ids) = line {
                for (c, &i) in ids.iter().enumerate() {
                    let x = CONTENT_X + c as f32 * (tw + GAP);
                    self.hits.push((rect(x, y, x + tw, y + TILE_H), Hit::Tile(i)));
                    // A store widget not downloaded yet: fetch it, the gallery rebuilds once it is here.
                    if let Some(item) = entries[i].store.as_ref().filter(|_| !entries[i].dir.is_dir()) {
                        crate::store::fetch(item, crate::post_scanned);
                        continue;
                    }
                    let id = &entries[i].info.id;
                    let sample = crate::feeds::sample(id);
                    let sample: Vec<(&str, &str)> = sample.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                    let values = if id.starts_with("dev.widget-engine.media") { media::SAMPLE } else { &sample };
                    scene.add_preview(&entries[i].dir, x + 12.0, y + 12.0, tw - 24.0, TILE_H - 64.0, values);
                }
            }
            y += line.height();
        }
        self.at_end = last + 1 >= lines.len();
        self.draw_base(scene)
    }

    fn on_event(&mut self, event: Event, scene: &mut Scene) -> Result<()> {
        let before = self.hover;
        match event {
            Event::PointerMove { x, y } => {
                self.hover = self.hit_at(x, y);
                // A drag on a tile takes the widget out of the gallery.
                if let Some((Hit::Tile(i), px, py)) = self.press {
                    if (x - px).hypot(y - py) > 6.0 {
                        self.press = None;
                        self.hover = None;
                        let e = self.entries.borrow()[i].clone();
                        (self.add)(&e, true);
                    }
                }
            }
            Event::PointerLeave => self.hover = None,
            Event::PointerDown { x, y } => self.press = self.hit_at(x, y).map(|h| (h, x, y)),
            Event::PointerUp { x, y } => {
                let hit = self.hit_at(x, y);
                match self.press.take() {
                    Some((h, ..)) if Some(h) == hit => match h {
                        Hit::Close => widgine::close(ID),
                        Hit::Search => {}
                        Hit::Clear => {
                            self.query.clear();
                            self.first = 0;
                            scene.request_rebuild();
                        }
                        Hit::Category(k) => {
                            (self.selected, self.first) = (k, 0);
                            scene.request_rebuild();
                        }
                        Hit::Tile(i) => {
                            let e = self.entries.borrow()[i].clone();
                            (self.add)(&e, false);
                            // The new widget lands in the middle of the screen, under this panel.
                            widgine::close(ID);
                        }
                    },
                    _ => {}
                }
            }
            Event::Wheel { delta, .. } => {
                if delta > 0.0 && self.first > 0 {
                    self.first -= 1;
                    scene.request_rebuild();
                } else if delta < 0.0 && !self.at_end {
                    self.first += 1;
                    scene.request_rebuild();
                }
            }
            // Typing goes to the search field. A search looks through all widgets.
            Event::Char(c) => {
                match c {
                    '\u{8}' => {
                        self.query.pop();
                    }
                    '\r' => return Ok(()),
                    c => self.query.push(c),
                }
                (self.selected, self.first) = (0, 0);
                scene.request_rebuild();
            }
            // The catalog changed (scan finished).
            Event::DataChanged => scene.request_rebuild(),
            _ => {}
        }
        if self.hover != before {
            self.draw_base(scene)?;
        }
        Ok(())
    }
}

/// A 16 DIP line icon centered at (cx, cy): "" for all widgets, else by category name.
unsafe fn category_icon(ctx: &ID2D1DeviceContext, d2d: &ID2D1Factory1, category: &str, cx: f32, cy: f32, b: &ID2D1SolidColorBrush) -> Result<()> {
    let tr = widgine::tr;
    let p = |x: f32, y: f32| Vector2 { X: cx + x, Y: cy + y };
    let round = |l: f32, t: f32, r: f32, bt: f32, rad: f32| D2D1_ROUNDED_RECT { rect: rect(cx + l, cy + t, cx + r, cy + bt), radiusX: rad, radiusY: rad };
    let dot = |x: f32, y: f32, r: f32| D2D1_ELLIPSE { point: p(x, y), radiusX: r, radiusY: r };
    unsafe {
        let stroke = d2d.CreateStrokeStyle(
            &D2D1_STROKE_STYLE_PROPERTIES1 { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() },
            None,
        )?;
        let w = 1.5;
        if category.is_empty() {
            // Four tiles.
            for (x, y) in [(-7.0, -7.0), (1.0, -7.0), (-7.0, 1.0), (1.0, 1.0)] {
                ctx.DrawRoundedRectangle(&round(x, y, x + 6.0, y + 6.0, 1.8), b, w, &stroke);
            }
        } else if category == tr("Time") {
            ctx.DrawEllipse(&dot(0.0, 0.0, 7.2), b, w, &stroke);
            ctx.DrawLine(p(0.0, 0.0), p(0.0, -4.2), b, w, &stroke);
            ctx.DrawLine(p(0.0, 0.0), p(3.2, 1.8), b, w, &stroke);
        } else if category == tr("Media") {
            // A note: two heads joined by a beam.
            ctx.FillEllipse(&D2D1_ELLIPSE { point: p(-4.2, 4.6), radiusX: 2.6, radiusY: 2.1 }, b);
            ctx.FillEllipse(&D2D1_ELLIPSE { point: p(4.0, 3.0), radiusX: 2.6, radiusY: 2.1 }, b);
            ctx.DrawLine(p(-1.8, 4.4), p(-1.8, -5.0), b, w, &stroke);
            ctx.DrawLine(p(6.4, 2.8), p(6.4, -6.6), b, w, &stroke);
            ctx.DrawLine(p(-1.8, -5.0), p(6.4, -6.6), b, w + 0.6, &stroke);
        } else if category == tr("System") {
            // A switch.
            ctx.DrawRoundedRectangle(&round(-7.5, -4.5, 7.5, 4.5, 4.5), b, w, &stroke);
            ctx.FillEllipse(&dot(3.0, 0.0, 2.4), b);
        } else if category == tr("Photos") {
            // A picture: frame, mountain, sun.
            ctx.DrawRoundedRectangle(&round(-7.5, -6.0, 7.5, 6.0, 2.2), b, w, &stroke);
            let m = d2d.CreatePathGeometry()?;
            let sink = m.Open()?;
            sink.BeginFigure(p(-7.0, 5.0), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddLines(&[p(-2.5, -0.5), p(1.0, 3.0), p(3.0, 1.0), p(7.0, 5.0)]);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            ctx.DrawGeometry(&m, b, w, &stroke);
            ctx.FillEllipse(&dot(3.2, -2.6, 1.4), b);
        } else if category == tr("Productivity") {
            // A ticked box.
            ctx.DrawRoundedRectangle(&round(-7.0, -7.0, 7.0, 7.0, 3.0), b, w, &stroke);
            ctx.DrawLine(p(-3.4, 0.2), p(-0.8, 2.8), b, w, &stroke);
            ctx.DrawLine(p(-0.8, 2.8), p(3.8, -2.6), b, w, &stroke);
        } else if category == tr("Weather") {
            // A sun rising behind a cloud.
            let (sx, sy, r) = (-2.6, -2.2, 3.4);
            for deg in [180.0f32, 225.0, 270.0, 315.0] {
                let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
                ctx.DrawLine(p(sx + c * (r + 1.8), sy + s * (r + 1.8)), p(sx + c * (r + 3.0), sy + s * (r + 3.0)), b, w, &stroke);
            }
            let arc = d2d.CreatePathGeometry()?;
            let sink = arc.Open()?;
            let a = 135.0f32.to_radians();
            sink.BeginFigure(p(sx + a.cos() * r, sy + a.sin() * r), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT { point: p(sx + r, sy), size: D2D_SIZE_F { width: r, height: r }, rotationAngle: 0.0, sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE, arcSize: D2D1_ARC_SIZE_LARGE });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            ctx.DrawGeometry(&arc, b, w, &stroke);
            ctx.DrawRoundedRectangle(&round(-7.0, 0.5, 7.5, 7.0, 3.25), b, w, &stroke);
        } else if category == tr("Other") {
            for x in [-5.5, 0.0, 5.5] {
                ctx.FillEllipse(&dot(x, 0.0, 1.6), b);
            }
        } else {
            // A widget card with a line of content.
            ctx.DrawRoundedRectangle(&round(-7.0, -5.5, 7.0, 5.5, 2.5), b, w, &stroke);
            ctx.DrawLine(p(-3.5, 0.0), p(3.5, 0.0), b, w, &stroke);
        }
    }
    Ok(())
}

fn group_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Featured => widgine::tr("Featured"),
        Kind::Builtin => widgine::tr("Built-in"),
        Kind::App => widgine::tr("Other apps"),
        Kind::Community => widgine::tr("Community"),
    }
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F { left, top, right, bottom }
}
