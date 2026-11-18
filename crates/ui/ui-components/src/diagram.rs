//! What a diagram view is made of, whatever its boxes stand for (a type and its relatives, a
//! table and its keys): where its cards and edges go once laid out ([`Layout`], in diagram
//! space: logical pixels before pan and zoom), how it's looked at ([`Viewport`]: pan, zoom,
//! fit, center, a press told from a drag), and how its pieces are drawn — a card's frame,
//! its header with a badge, its rows, the axis-aligned edges with their arrows, a legend's
//! sample. Each view lays out and fills its own cards; drawing is quads and text only.

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

use crate::Palette;
use crate::ds::{self, Ctx, Pen};

pub mod layered;
pub mod tidy;

pub use tidy::{Flow, Gaps, links, tidy};

/// A card's header and its rows, in logical pixels (scaled with the zoom).
pub const HEADER_H: f32 = 46.0;
pub const ROW_H: f32 = 21.0;
/// The space under a card's last row.
pub const PAD: f32 = 8.0;

/// The way an edge ends at its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Right,
    Left,
}

/// A box of the diagram: node `node` of the view's model, at `rect`, listing `shown` of its
/// rows (and saying how many `more` there are).
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub node: usize,
    pub rect: Rect,
    pub shown: usize,
    pub more: usize,
}

/// A line between two cards: axis-aligned, ending at the target's border (where its arrow's
/// tip is), of the view's `kind`.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge<K> {
    pub points: Vec<Point>,
    pub dir: Dir,
    pub kind: K,
}

impl<K> Edge<K> {
    /// The rectangle holding its points.
    pub fn bounds(&self) -> Rect {
        bounds_of(self.points.iter().map(|p| Rect::new(p.x, p.y, 0.0, 0.0)))
    }
}

/// Where a diagram's cards and edges go, and the rectangle holding them all.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout<K> {
    pub cards: Vec<Card>,
    pub edges: Vec<Edge<K>>,
    pub bounds: Rect,
}

impl<K> Default for Layout<K> {
    fn default() -> Self {
        Self { cards: Vec::new(), edges: Vec::new(), bounds: Rect::default() }
    }
}

impl<K> Layout<K> {
    /// `cards` and `edges`, their bounds those of the cards.
    pub fn new(cards: Vec<Card>, edges: Vec<Edge<K>>) -> Self {
        let bounds = bounds_of(cards.iter().map(|c| c.rect));
        Self { cards, edges, bounds }
    }

    /// The card of node `i`.
    pub fn card(&self, i: usize) -> Option<&Card> {
        self.cards.iter().find(|c| c.node == i)
    }
}

/// A card's height: its header, then `shown` rows and one saying how many `more` there are.
pub fn card_height(shown: usize, more: usize) -> f32 {
    if shown == 0 && more == 0 {
        HEADER_H
    } else {
        HEADER_H + (shown + usize::from(more > 0)) as f32 * ROW_H + PAD
    }
}

/// The rectangle holding all of `rects` (empty: the default one).
pub fn bounds_of(rects: impl IntoIterator<Item = Rect>) -> Rect {
    let mut rects = rects.into_iter();
    let Some(first) = rects.next() else { return Rect::default() };
    rects.fold(first, |acc, r| {
        let (x0, y0) = (acc.min_x().min(r.min_x()), acc.min_y().min(r.min_y()));
        let (x1, y1) = (acc.max_x().max(r.max_x()), acc.max_y().max(r.max_y()));
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    })
}

/// What a release of the mouse was.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Release {
    /// No press began in the view.
    None,
    /// The end of a pan.
    Panned,
    /// A click (where it was pressed).
    Click(Point),
}

/// What covers a diagram's area along its sides (screen pixels): a bar of controls along
/// its bottom, a panel over its right. Fitting and centering use what's left of it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// How a diagram is looked at: the diagram-space point at the area's top left (`pan`), the
/// zoom, the screen's scale, where the view is (a toolbar `top` logical pixels high over
/// the diagram's area, what covers its sides), and a press being told from a drag.
#[derive(Debug, Clone, Default)]
pub struct Viewport {
    pub pan: Point,
    pub zoom: f32,
    pub scale: f32,
    pub bounds: Rect,
    /// The toolbar's height (logical pixels).
    pub top: f32,
    /// What the view draws over the area's sides (none by default).
    pub insets: Insets,
    /// Fitted at the next paint (a new layout).
    pub fit_pending: bool,
    pub press: Option<Point>,
    pub moved: bool,
}

impl Viewport {
    /// A viewport under a toolbar `top` logical pixels high.
    pub fn new(top: f32) -> Self {
        Self { top, zoom: 1.0, ..Self::default() }
    }

    /// Diagram → screen pixels.
    pub fn k(&self) -> f32 {
        self.zoom.max(0.05) * self.scale
    }

    /// The area under the toolbar.
    pub fn area(&self) -> Rect {
        let top = (self.top * self.scale).round();
        Rect::new(
            self.bounds.min_x(),
            self.bounds.min_y() + top,
            self.bounds.size.width,
            (self.bounds.size.height - top).max(0.0),
        )
    }

    /// The area left in view by its [`Insets`] (the whole area when they'd leave too
    /// little of it).
    pub fn visible(&self) -> Rect {
        let a = self.area();
        let i = self.insets;
        let (w, h) = (a.size.width - i.left - i.right, a.size.height - i.top - i.bottom);
        if w < 120.0 * self.scale || h < 80.0 * self.scale {
            return a;
        }
        Rect::new(a.min_x() + i.left, a.min_y() + i.top, w, h)
    }

    pub fn to_screen(&self, p: Point) -> Point {
        let a = self.area();
        let k = self.k();
        Point::new(a.min_x() + (p.x - self.pan.x) * k, a.min_y() + (p.y - self.pan.y) * k)
    }

    /// A screen point in diagram space.
    pub fn to_diagram(&self, p: Point) -> Point {
        let a = self.area();
        let k = self.k();
        Point::new(self.pan.x + (p.x - a.min_x()) / k, self.pan.y + (p.y - a.min_y()) / k)
    }

    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        let a = self.to_screen(Point::new(r.min_x(), r.min_y()));
        let k = self.k();
        Rect::new(a.x.round(), a.y.round(), (r.size.width * k).round(), (r.size.height * k).round())
    }

    /// Fits `bounds` in what's [`visible`](Self::visible), zoomed between `min` and `max`;
    /// when even `min` is too small to keep it readable, it's shown around `focus` instead
    /// (if given).
    pub fn fit(&mut self, bounds: Rect, focus: Option<Rect>, min: f32, max: f32) {
        let b = bounds;
        let (a, v) = (self.area(), self.visible());
        if b.size.width <= 0.0 || v.size.width <= 0.0 || self.scale <= 0.0 {
            return;
        }
        let (view_w, view_h) = (v.size.width / self.scale, v.size.height / self.scale);
        let fit = ((view_w - 80.0) / b.size.width).min((view_h - 60.0) / b.size.height);
        self.zoom = fit.clamp(min, max);
        let center = match focus.filter(|_| fit < min) {
            Some(focus) => focus,
            None => b,
        };
        // How far the visible part starts into the area (diagram pixels at zoom 1).
        let (off_x, off_y) =
            ((v.min_x() - a.min_x()) / self.scale, (v.min_y() - a.min_y()) / self.scale);
        self.pan = Point::new(
            center.min_x() + center.size.width / 2.0 - view_w / self.zoom / 2.0 - off_x / self.zoom,
            center.min_y() + center.size.height / 2.0
                - view_h / self.zoom / 2.0
                - off_y / self.zoom,
        );
        self.fit_pending = false;
    }

    /// Pans so `r` is in the middle of what's [`visible`](Self::visible) (the zoom kept).
    pub fn center_on(&mut self, r: Rect) {
        let (a, v) = (self.area(), self.visible());
        if self.scale <= 0.0 {
            return;
        }
        let k = self.k();
        let (view_w, view_h) = (v.size.width / k, v.size.height / k);
        let (off_x, off_y) = ((v.min_x() - a.min_x()) / k, (v.min_y() - a.min_y()) / k);
        self.pan = Point::new(
            r.min_x() + r.size.width / 2.0 - view_w / 2.0 - off_x,
            r.min_y() + r.size.height / 2.0 - view_h / 2.0 - off_y,
        );
    }

    /// Zooms by `factor` around screen point `at`, within `min`..`max`. `false`: no change.
    pub fn zoom_at(&mut self, factor: f32, at: Point, min: f32, max: f32) -> bool {
        let before = self.zoom;
        let zoom = (self.zoom * factor).clamp(min, max);
        if (zoom - before).abs() < f32::EPSILON {
            return false;
        }
        let a = self.area();
        let (k0, k1) = (before * self.scale, zoom * self.scale);
        let gx = self.pan.x + (at.x - a.min_x()) / k0;
        let gy = self.pan.y + (at.y - a.min_y()) / k0;
        self.pan = Point::new(gx - (at.x - a.min_x()) / k1, gy - (at.y - a.min_y()) / k1);
        self.zoom = zoom;
        true
    }

    /// Back to 100%, around the middle of what's visible.
    pub fn reset_zoom(&mut self, min: f32, max: f32) -> bool {
        let a = self.visible();
        let center = Point::new(a.min_x() + a.size.width / 2.0, a.min_y() + a.size.height / 2.0);
        self.zoom_at(1.0 / self.zoom.max(0.05), center, min, max)
    }

    /// Scrolled by screen pixels.
    pub fn scroll(&mut self, dx: f32, dy: f32) {
        let k = self.k();
        self.pan = Point::new(self.pan.x + dx / k, self.pan.y + dy / k);
    }

    /// The mouse pressed at `p`: a click at release, unless it moves (a pan).
    pub fn press(&mut self, p: Point) {
        self.press = Some(p);
        self.moved = false;
    }

    /// The mouse dragged to `p`: past a few pixels, the diagram follows it. `true` if it
    /// moved.
    pub fn drag(&mut self, p: Point) -> bool {
        let Some(from) = self.press else { return false };
        if !self.moved && (p.x - from.x).hypot(p.y - from.y) < 4.0 * self.scale {
            return false;
        }
        self.moved = true;
        let k = self.k();
        self.pan = Point::new(self.pan.x - (p.x - from.x) / k, self.pan.y - (p.y - from.y) / k);
        self.press = Some(p);
        true
    }

    /// The mouse released: a click, the end of a pan, or nothing pressed.
    pub fn release(&mut self) -> Release {
        let Some(at) = self.press.take() else { return Release::None };
        if std::mem::take(&mut self.moved) {
            return Release::Panned;
        }
        Release::Click(at)
    }
}

/// What was looked at before (← goes back to it).
#[derive(Debug, Clone)]
pub struct History<T>(Vec<T>);

impl<T> Default for History<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> History<T> {
    pub fn push(&mut self, item: T) {
        self.0.push(item);
    }

    pub fn pop(&mut self) -> Option<T> {
        self.0.pop()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn items(&self) -> &[T] {
        &self.0
    }
}

/// The cards' fonts at scale `k`: the title, the line under it (and "more"), the rows.
#[derive(Debug, Clone)]
pub struct CardFonts {
    pub title: Font,
    pub sub: Font,
    pub row: Font,
}

impl CardFonts {
    pub fn new(p: &Palette, k: f32) -> Self {
        Self {
            title: Font::new(p.ui_font.clone(), (p.ui_font_size * k).round().max(6.0))
                .with_weight(Font::SEMIBOLD),
            sub: Font::new(p.ui_font.clone(), ((p.ui_font_size - 2.5) * k).round().max(5.0)),
            row: Font::new(p.code_font.clone(), ((p.code_font_size - 1.5) * k).round().max(5.0)),
        }
    }
}

/// How a card looks: the one the diagram is about (raised, outlined in the accent),
/// under the mouse, or muted (something the diagram only names: not indexed, not read).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CardLook {
    pub focus: bool,
    pub hovered: bool,
    pub muted: bool,
}

/// A card's frame at screen rectangle `r` (`k`: diagram → screen; `zoom` and `scale` for
/// the focused one's shadow and border).
pub fn card_frame(
    layer: &mut Layer,
    ds: &Ctx<'_>,
    r: Rect,
    look: CardLook,
    k: f32,
    zoom: f32,
    scale: f32,
) {
    let p = ds.palette;
    let rad = (10.0 * k).round();
    if look.focus {
        ds::surface(layer, ds, r, ds::Elevation::Medium, 10.0 * zoom);
    }
    let fill = if look.muted { p.text.with_alpha(0.025) } else { p.editor_background };
    let border = if look.focus {
        p.accent
    } else if look.hovered {
        p.accent.with_alpha(0.6)
    } else {
        p.card_border
    };
    let bw = if look.focus { (2.0 * scale).round() } else { ds.line() };
    layer.quad(Quad::filled(r, fill).rounded(rad).bordered(bw, border));
}

/// What a card's header shows: a badge (its letter and color), the title, and the line
/// under it (none: the title alone, centered).
#[derive(Debug, Clone, Copy)]
pub struct Header<'a> {
    pub glyph: &'a str,
    pub color: Rgba,
    pub title: &'a str,
    pub subtitle: &'a str,
    /// The title muted (something only named).
    pub muted: bool,
}

/// A card's header at the top of screen rectangle `r`: its badge, title and subtitle.
/// Returns its rectangle.
pub fn card_header(
    pen: Pen<'_, '_>,
    fonts: &CardFonts,
    header: Header<'_>,
    r: Rect,
    k: f32,
) -> Rect {
    let Pen { layer, text, ds } = pen;
    let p = ds.palette;
    let rect = Rect::new(r.min_x(), r.min_y(), r.size.width, (HEADER_H * k).round());
    let bs = (20.0 * k).round();
    let badge =
        Rect::new(r.min_x() + 10.0 * k, rect.min_y() + (rect.size.height - bs) / 2.0, bs, bs);
    layer.quad(Quad::filled(badge, header.color.with_alpha(0.16)).rounded((5.0 * k).round()));
    let glyph_font =
        Font::new(p.ui_font.clone(), (bs * 0.6).round().max(5.0)).with_weight(Font::BOLD);
    let g = text.shape_line(header.glyph, &glyph_font);
    draw_text(
        layer,
        &g,
        (badge.min_x() + (badge.size.width - g.width) / 2.0).round(),
        ds::optical_baseline(badge.min_y(), badge.size.height, &glyph_font),
        header.color,
    );
    let tx = badge.max_x() + 8.0 * k;
    let max = r.max_x() - tx - 10.0 * k;
    let name = ds::ellipsize(text, header.title, &fonts.title, max);
    let shaped = text.shape_line(&name, &fonts.title);
    let (name_y, sub_y) = if header.subtitle.is_empty() {
        (ds::optical_baseline(rect.min_y(), rect.size.height, &fonts.title), 0.0)
    } else {
        (rect.min_y() + rect.size.height * 0.48, rect.min_y() + rect.size.height * 0.82)
    };
    let name_color = if header.muted { p.text_muted } else { p.text };
    draw_text(layer, &shaped, tx.round(), name_y.round(), name_color);
    if !header.subtitle.is_empty() {
        let sub = ds::ellipsize_start(text, header.subtitle, &fonts.sub, max);
        let shaped = text.shape_line(&sub, &fonts.sub);
        draw_text(layer, &shaped, tx.round(), sub_y.round(), p.text_muted);
    }
    rect
}

/// The line between a card's header and its rows.
pub fn header_divider(layer: &mut Layer, ds: &Ctx<'_>, r: Rect, header: Rect) {
    layer.quad(Quad::filled(
        Rect::new(r.min_x(), header.max_y(), r.size.width, ds.line()),
        ds.palette.card_border.with_alpha(0.7),
    ));
}

/// What a card's row shows: a mark before it (`+`, a key), its label, and a detail on its
/// right (a type, what a key points to).
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    pub mark: &'a str,
    pub mark_color: Rgba,
    pub label: &'a str,
    pub color: Rgba,
    pub detail: Option<(&'a str, Rgba)>,
    pub hovered: bool,
}

/// A row of the card at screen rectangle `r`, its top at `y`, `row_h` high. Returns its
/// rectangle (where a click on it lands) and where its label starts.
pub fn card_row(
    pen: Pen<'_, '_>,
    font: &Font,
    r: Rect,
    y: f32,
    row_h: f32,
    k: f32,
    row: Row<'_>,
) -> (Rect, f32) {
    let Pen { layer, text, ds } = pen;
    let p = ds.palette;
    let rect = Rect::new(r.min_x() + ds.line(), y, r.size.width - 2.0 * ds.line(), row_h);
    if row.hovered {
        layer.quad(Quad::filled(rect, p.text.with_alpha(0.06)));
    }
    let base = ds::optical_baseline(rect.min_y(), rect.size.height, font);
    let mx = r.min_x() + 12.0 * k;
    if !row.mark.is_empty() {
        let mark = text.shape_line(row.mark, font);
        draw_text(layer, &mark, mx.round(), base, row.mark_color);
    }
    let lx = mx + 14.0 * k;
    let mut max = r.max_x() - lx - 10.0 * k;
    if let Some((detail, color)) = row.detail {
        // The detail gives way to the label when the card is narrow.
        let cut = ds::ellipsize(text, detail, font, max * 0.45);
        let shaped = text.shape_line(&cut, font);
        let dx = r.max_x() - 10.0 * k - shaped.width;
        draw_text(layer, &shaped, dx.round(), base, color);
        max -= shaped.width + 8.0 * k;
    }
    let label = ds::ellipsize(text, row.label, font, max);
    let shaped = text.shape_line(&label, font);
    draw_text(layer, &shaped, lx.round(), base, row.color);
    (rect, lx)
}

/// The last row of a card listing only some: how many more there are.
pub fn more_row(pen: Pen<'_, '_>, font: &Font, r: Rect, y: f32, row_h: f32, k: f32, label: &str) {
    let Pen { layer, text, ds } = pen;
    let shaped = text.shape_line(label, font);
    let base = ds::optical_baseline(y, row_h, font);
    draw_text(layer, &shaped, (r.min_x() + 26.0 * k).round(), base, ds.palette.text_muted);
}

/// How an edge ends at its target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Head {
    /// Nothing: the line meets the card.
    None,
    /// A triangle; hollow (its inside `fill`) or filled.
    Arrow { hollow: Option<Rgba> },
}

/// How an edge is drawn: its color, dashed or not, its head, its thickness (logical
/// pixels at 100 %; [`EdgeStyle::LINE`] the usual one).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeStyle {
    pub color: Rgba,
    pub dashed: bool,
    pub head: Head,
    pub width: f32,
}

impl EdgeStyle {
    /// The usual thickness.
    pub const LINE: f32 = 1.5;
}

/// An edge through screen `points` (its tip the last one, pointing `dir`), at scale `k`:
/// its segments, then its head.
pub fn edge(layer: &mut Layer, points: &[Point], dir: Dir, k: f32, style: EdgeStyle) {
    let Some(&tip) = points.last() else { return };
    let w = (style.width * k).max(1.0);
    // A thick line's head grows with it (the usual one's stays as it was).
    let head = (10.0 * k).max(5.0).max(w * 2.2);
    let head_w = (11.0 * k).max(6.0).max(w * 2.6);
    // The line stops where the arrow begins.
    let mut line = points.to_vec();
    if let (Some(last), Head::Arrow { .. }) = (line.last_mut(), style.head) {
        match dir {
            Dir::Up => last.y += head,
            Dir::Down => last.y -= head,
            Dir::Right => last.x -= head,
            Dir::Left => last.x += head,
        }
    }
    for pair in line.windows(2) {
        segment(layer, pair[0], pair[1], w, style.dashed, style.color);
    }
    // A thick line's bends filled (a thin one's are a pixel).
    if style.width > EdgeStyle::LINE && !style.dashed && line.len() > 2 {
        for p in &line[1..line.len() - 1] {
            layer.quad(Quad::filled(Rect::new(p.x - w / 2.0, p.y - w / 2.0, w, w), style.color));
        }
    }
    if let Head::Arrow { hollow } = style.head {
        arrow(layer, tip, dir, head, head_w, style.color, hollow);
    }
}

/// A legend's sample of `style`: a short line from `x0` to `x1` at `mid`, ending in its
/// arrow (logical sizes scaled by `ds`).
pub fn legend_sample(
    layer: &mut Layer,
    ds: &Ctx<'_>,
    x0: f32,
    x1: f32,
    mid: f32,
    style: EdgeStyle,
) {
    let px = |v: f32| ds.px(v);
    let w = px(style.width).max(1.0);
    match style.head {
        Head::Arrow { hollow } => {
            segment(
                layer,
                Point::new(x0, mid),
                Point::new(x1 - px(7.0), mid),
                w,
                style.dashed,
                style.color,
            );
            arrow(layer, Point::new(x1, mid), Dir::Right, px(8.0), px(9.0), style.color, hollow);
        }
        Head::None => {
            segment(layer, Point::new(x0, mid), Point::new(x1, mid), w, style.dashed, style.color);
        }
    }
}

/// The dot grid's spacing (logical pixels, on screen: it doesn't follow the zoom).
pub const DOT_GRID_STEP: f32 = 24.0;

/// A canvas's dot grid over `area`: the dots' spacing and size stay the same at any zoom
/// (only the cards zoom); the grid moves with the pan, anchored at `origin` (where the
/// diagram's (0, 0) is on screen). Faint, but readable in both themes.
pub fn dot_grid(layer: &mut Layer, p: &Palette, area: Rect, origin: Point, scale: f32) {
    let step = DOT_GRID_STEP * scale;
    let dot = (1.6 * scale).round().max(1.0);
    let color = p.text.with_alpha(0.14);
    let (x0, y0) = dot_grid_start(area, origin, step);
    let mut y = y0;
    while y < area.max_y() {
        let mut x = x0;
        while x < area.max_x() {
            layer.quad(
                Quad::filled(Rect::new(x.round(), y.round(), dot, dot), color).rounded(dot / 2.0),
            );
            x += step;
        }
        y += step;
    }
}

/// The grid's first dot in `area` (its top left), `step` screen pixels apart from `origin`.
pub fn dot_grid_start(area: Rect, origin: Point, step: f32) -> (f32, f32) {
    (
        area.min_x() + (origin.x - area.min_x()).rem_euclid(step),
        area.min_y() + (origin.y - area.min_y()).rem_euclid(step),
    )
}

/// An axis-aligned segment `a → b`, `w` thick, solid or dashed.
pub fn segment(layer: &mut Layer, a: Point, b: Point, w: f32, dashed: bool, color: Rgba) {
    let (len, horizontal) = if (a.y - b.y).abs() < 0.5 {
        ((b.x - a.x).abs(), true)
    } else {
        ((b.y - a.y).abs(), false)
    };
    let (dash, gap) = if dashed { (w * 4.0, w * 3.0) } else { (len, 0.0) };
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let mut at = 0.0;
    while at < len {
        let piece = dash.min(len - at);
        let r = if horizontal {
            Rect::new(x0 + at, a.y - w / 2.0, piece, w)
        } else {
            Rect::new(a.x - w / 2.0, y0 + at, w, piece)
        };
        layer.quad(Quad::filled(r, color));
        at += dash + gap.max(0.0);
        if gap == 0.0 {
            break;
        }
    }
}

/// How far `p` is from the polyline through `points` (in their units).
pub fn distance_to_polyline(p: Point, points: &[Point]) -> f32 {
    let segment = |a: Point, b: Point| {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 {
            (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
    };
    match points {
        [] => f32::INFINITY,
        [one] => (p.x - one.x).hypot(p.y - one.y),
        _ => points.windows(2).map(|w| segment(w[0], w[1])).fold(f32::INFINITY, f32::min),
    }
}

impl<K> Edge<K> {
    /// How far `p` is from it (diagram space).
    pub fn distance(&self, p: Point) -> f32 {
        distance_to_polyline(p, &self.points)
    }
}

/// `r` grown by `by` on every side.
pub fn inflate(r: Rect, by: f32) -> Rect {
    Rect::new(r.min_x() - by, r.min_y() - by, r.size.width + 2.0 * by, r.size.height + 2.0 * by)
}

/// `inner` wholly inside `outer`.
pub fn contains_rect(outer: Rect, inner: Rect) -> bool {
    inner.min_x() >= outer.min_x()
        && inner.min_y() >= outer.min_y()
        && inner.max_x() <= outer.max_x()
        && inner.max_y() <= outer.max_y()
}

/// `r` moved (not resized) inside `within` as far as it goes; a side that doesn't fit
/// starts where `within` does.
pub fn clamp_into(r: Rect, within: Rect) -> Rect {
    let x = r.min_x().min(within.max_x() - r.size.width).max(within.min_x());
    let y = r.min_y().min(within.max_y() - r.size.height).max(within.min_y());
    Rect::new(x, y, r.size.width, r.size.height)
}

/// Where a label `size` (width, height) may go along an edge through `points`, best first
/// (the edge's own space): beside its segments, those that hold it first and the longest
/// first — over or under a horizontal one, right or left of a vertical one, `gap` off the
/// line so the line stays in view — at their middle, then nearer their ends.
pub fn label_spots(points: &[Point], size: (f32, f32), gap: f32) -> Vec<Rect> {
    let (w, h) = size;
    let mut segments: Vec<(Point, Point, bool, f32)> = points
        .windows(2)
        .filter(|s| s[0] != s[1])
        .map(|s| {
            let horizontal = (s[0].y - s[1].y).abs() < 0.5;
            let len = (s[1].x - s[0].x).abs() + (s[1].y - s[0].y).abs();
            // What the label leaves of it (negative: it overhangs).
            (s[0], s[1], horizontal, len - if horizontal { w } else { h })
        })
        .collect();
    segments.sort_by(|a, b| (b.3 >= 0.0).cmp(&(a.3 >= 0.0)).then(b.3.total_cmp(&a.3)));
    let mut spots = Vec::with_capacity(segments.len() * 10);
    for (a, b, horizontal, _) in segments {
        for t in [0.5, 0.3, 0.7, 0.15, 0.85] {
            let (cx, cy) = (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            if horizontal {
                spots.push(Rect::new(cx - w / 2.0, cy - gap - h, w, h));
                spots.push(Rect::new(cx - w / 2.0, cy + gap, w, h));
            } else {
                spots.push(Rect::new(cx + gap, cy - h / 2.0, w, h));
                spots.push(Rect::new(cx - gap - w, cy - h / 2.0, w, h));
            }
        }
    }
    spots
}

/// `r` overlaps none of `obstacles`.
pub fn clear_of(r: Rect, obstacles: &[Rect]) -> bool {
    obstacles.iter().all(|o| r.intersect(o).is_none())
}

/// Labels placed in their order (the most wanted first): each at its first spot clear of
/// `obstacles` (the cards) and of the labels placed before it; `None` for one without such
/// a spot (shown only when asked for: its edge under the mouse, or selected).
pub fn place_labels(spots: &[Vec<Rect>], obstacles: &[Rect]) -> Vec<Option<usize>> {
    let mut placed: Vec<Rect> = Vec::new();
    spots
        .iter()
        .map(|spots| {
            let chosen = spots.iter().position(|&r| clear_of(r, obstacles) && clear_of(r, &placed));
            if let Some(i) = chosen {
                placed.push(spots[i]);
            }
            chosen
        })
        .collect()
}

/// A triangle pointing `dir` with its tip at `tip`: `len` long, `width` at its base, drawn
/// as one-pixel strips (crisp at any zoom). Hollow: its inside is `fill`.
pub fn arrow(
    layer: &mut Layer,
    tip: Point,
    dir: Dir,
    len: f32,
    width: f32,
    color: Rgba,
    hollow: Option<Rgba>,
) {
    let mut strips = |len: f32, width: f32, offset: f32, color: Rgba| {
        let n = len.round().max(1.0) as usize;
        for i in 0..n {
            let t = (i as f32 + 0.5) / n as f32;
            let w = (width * t).max(1.0);
            let d = offset + i as f32;
            let r = match dir {
                Dir::Up => Rect::new(tip.x - w / 2.0, tip.y + d, w, 1.0),
                Dir::Down => Rect::new(tip.x - w / 2.0, tip.y - d - 1.0, w, 1.0),
                Dir::Right => Rect::new(tip.x - d - 1.0, tip.y - w / 2.0, 1.0, w),
                Dir::Left => Rect::new(tip.x + d, tip.y - w / 2.0, 1.0, w),
            };
            layer.quad(Quad::filled(r, color));
        }
    };
    strips(len, width, 0.0, color);
    if let Some(fill) = hollow {
        let border = (len * 0.2).max(2.0);
        strips(len - border * 1.6, width - border * 2.2, border * 1.3, fill);
    }
}

/// The toolbar's ← at `r`: its hover, its arrow.
pub fn back_button(pen: Pen<'_, '_>, r: Rect, hovered: bool) {
    let Pen { layer, text, ds } = pen;
    let p = ds.palette;
    if hovered {
        layer.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(ds.px(crate::ds::radius::MD)));
    }
    let font = ds.ui_font(2.0);
    let shaped = text.shape_line("←", &font);
    let tx = (r.min_x() + (r.size.width - shaped.width) / 2.0).round();
    draw_text(layer, &shaped, tx, ds::optical_baseline(r.min_y(), r.size.height, &font), p.text);
}

/// A diagram's toolbar band across the top of `b`, `h` logical pixels high, with the line
/// under it. Returns its rectangle.
pub fn toolbar(layer: &mut Layer, ds: &Ctx<'_>, b: Rect, h: f32) -> Rect {
    let p = ds.palette;
    let bar = Rect::new(b.min_x(), b.min_y(), b.size.width, ds.px(h));
    layer.quad(Quad::filled(bar, p.editor_background).rounded(ds.px(crate::ds::radius::CARD)));
    layer.quad(Quad::filled(
        Rect::new(bar.min_x(), bar.max_y() - ds.line(), bar.size.width, ds.line()),
        p.card_border,
    ));
    bar
}

/// The diagram's title in the toolbar, at `x`: returns where what follows it starts.
pub fn toolbar_title(
    text: &mut TextSystem,
    layer: &mut Layer,
    ds: &Ctx<'_>,
    bar: Rect,
    x: f32,
    title: &str,
) -> f32 {
    let title_font = ds.ui_font(2.0).with_weight(Font::SEMIBOLD);
    let shaped = text.shape_line(title, &title_font);
    draw_text(
        layer,
        &shaped,
        x,
        ds::optical_baseline(bar.min_y(), bar.size.height, &title_font),
        ds.palette.text,
    );
    x + shaped.width
}

#[cfg(test)]
#[path = "../tests/unit/diagram.rs"]
mod tests;
