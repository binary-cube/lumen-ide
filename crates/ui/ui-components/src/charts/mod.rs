//! The app's one chart (plan 94): whatever draws a chart — a query's result, the Task
//! Manager, the relax mode's widgets, a profile — draws it with [`Chart`], in the theme's
//! colors ([`ChartLook`]), so every chart looks, reads and answers the mouse the same.
//!
//! The data is prepared once by `charts-model` (on a worker for many rows) and handed as a
//! [`charts_model::Prepared`] with a generation; the chart lays it out only when the data, its
//! size, the series shown or the zoom change, rasterizes its lines and slices on a worker
//! ([`raster`]) and draws the rest from quads. Under the mouse it lights what's there and
//! tells it in a tooltip; a click on the legend hides a series; over a measured X the wheel
//! zooms, a drag moves, a double click shows all. To a screen reader it's an image named by
//! its title with a summary of what it shows.

mod look;
mod motion;
mod raster;
pub mod spark;

use std::sync::Arc;

use charts_model::geometry::{Geometry, Hit, MarkShape, Metrics, View};
use charts_model::prepare::Shape;
use charts_model::scale::number_label;
use charts_model::{Options, Prepared};
use kernel::{Point, Rect};
use render_api::{Layer, Quad};
use text_layout::TextSystem;
use ui_kit::draw_text;

pub use look::{ChartLook, ChartMotion, ChartParts, Enter, Update};

use crate::control::{Control, Press};
use crate::ds::Ctx;
use crate::tooltip::Tip;
use crate::viewer::ViewerCursor;

type Wake = Arc<dyn Fn() + Send + Sync>;

/// How a chart's values are written (its tooltip, its values' axis): series `i`'s value
/// `v` in its unit (`3.4%`, `1.2 MB/s`). Without one: a number of four significant digits.
#[derive(Clone)]
pub struct ValueFormat(pub Arc<dyn Fn(usize, f64) -> String + Send + Sync>);

impl std::fmt::Debug for ValueFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ValueFormat")
    }
}

/// The fonts an exported chart asks for (the system's interface font first).
const SVG_FONTS: &str = "system-ui, sans-serif";

/// A lane's ground: the grid's color, this much of its alpha.
const LANE_GROUND: f32 = 0.35;

/// How far a hovered slice comes out of the pie (logical pixels).
const SLICE_OUT: f32 = 6.0;
/// The wheel's zoom for one line of scroll (a share of the range).
const ZOOM_STEP: f64 = 0.12;
/// The narrowest a zoom goes (a share of the whole range).
const ZOOM_LEAST: f64 = 0.002;

/// What a layout was made for, as one fingerprint (made again when any of it changes):
/// the data's generation, the area, the scale, the series hidden, the zoom, the options, the
/// parts shown, the values' axis — compared each frame without making anything.
fn laid_key(c: &Chart, area: Rect, scale: f32, parts: ChartParts) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    c.generation.hash(&mut h);
    [area.min_x(), area.min_y(), area.size.width, area.size.height, scale]
        .map(f32::to_bits)
        .hash(&mut h);
    c.hidden.hash(&mut h);
    c.zoom.map(|(a, b)| (a.to_bits(), b.to_bits())).hash(&mut h);
    c.options.hash(&mut h);
    parts.hash(&mut h);
    c.y_range.map(|(a, b)| (a.to_bits(), b.to_bits())).hash(&mut h);
    c.lanes.hash(&mut h);
    h.finish()
}

/// A drag along a measured X: where it began and the range then.
#[derive(Debug, Clone, Copy)]
struct Pan {
    from: f32,
    range: (f64, f64),
}

/// A chart: its data, what's shown of it, its layout and masks, the mouse over it.
#[derive(Debug, Default)]
pub struct Chart {
    data: Option<Arc<Prepared>>,
    generation: u64,
    options: Options,
    hidden: Vec<bool>,
    zoom: Option<(f64, f64)>,
    geometry: Geometry,
    /// The layout's fingerprint ([`laid_key`]).
    laid: Option<u64>,
    /// The ticks' labels as drawn (formatted, cut to their room), made with the layout.
    y_labels: Vec<String>,
    x_labels: Vec<String>,
    /// Where an X label cut short is drawn, and its tick (its tooltip says it whole).
    cut_labels: Vec<(Rect, usize)>,
    raster: raster::Raster,
    /// Where it was drawn last.
    area: Rect,
    scale: f32,
    mouse: Option<Point>,
    hit: Option<Hit>,
    pan: Option<Pan>,
    parts: ChartParts,
    /// What it's called to a screen reader (its title).
    title: String,
    /// Its values written in their unit.
    format: Option<ValueFormat>,
    /// Its values' axis fixed (a share: 0–100).
    y_range: Option<(f64, f64)>,
    /// Its series in lanes (see `View::lanes`).
    lanes: bool,
    /// Its transitions (appearing, new data, the lit element).
    motion: motion::Motion,
    /// How X's ticks are written (a duration: `1:20`); `None`: as the model writes them.
    x_format: Option<ValueFormat>,
    /// A pie's slice lit by its owner (its row under the mouse in a list beside it), when
    /// the mouse isn't on one.
    lit_by_owner: Option<usize>,
}

impl Chart {
    pub fn new() -> Self {
        Self::default()
    }

    /// Its data, prepared, as of `generation` (a new one is laid out again); `None`: nothing
    /// to show.
    pub fn set_data(&mut self, data: Option<Arc<Prepared>>, generation: u64) {
        if generation == self.generation && data.is_some() == self.data.is_some() {
            return;
        }
        let series = data.as_ref().map_or(0, |d| legend_len(d));
        if self.hidden.len() != series {
            self.hidden = vec![false; series];
        }
        // A transition only for what's drawn (one not drawn would move unseen, with a look
        // it was never given).
        if data.is_some() && self.drawn() {
            let first = self.data.is_none();
            self.motion.data(first, &self.geometry.marks, self.raster.pieces());
        }
        self.data = data;
        self.generation = generation;
        self.hit = None;
    }

    /// Lights a pie's slice `index` from outside (its row in a list under the mouse); `None`:
    /// none. `true` if it changed (a frame wanted).
    pub fn set_lit(&mut self, index: Option<usize>) -> bool {
        if index == self.lit_by_owner {
            return false;
        }
        self.lit_by_owner = index;
        if index.is_some() {
            self.motion.lit();
        }
        true
    }

    /// The pie's slice at `p` (as last drawn), if any: what a click on it is about.
    pub fn slice_at(&self, p: Point) -> Option<usize> {
        let pie = self.data.as_ref().is_some_and(|d| d.kind == charts_model::ChartKind::Pie);
        match self.geometry.hit(p)? {
            Hit::Mark { index, .. } if pie => Some(index),
            _ => None,
        }
    }

    /// The spec's options (in steps, dots, to 100%, a log scale).
    pub fn set_options(&mut self, options: Options) {
        self.options = options;
    }

    /// How its values are written (`None`: as numbers).
    pub fn set_format(&mut self, format: Option<ValueFormat>) {
        self.format = format;
        // Its labels written and their room measured anew (set with new data, not per frame).
        self.laid = None;
    }

    /// Its values' axis from `lo` to `hi` whatever the values (`None`: from them).
    pub fn set_y_range(&mut self, range: Option<(f64, f64)>) {
        self.y_range = range;
    }

    /// Its series in bands of their own along one X, each on its own scale.
    pub fn set_lanes(&mut self, lanes: bool) {
        self.lanes = lanes;
    }

    /// How X's ticks are written (`None`: as numbers, dates or categories); set with its
    /// data (its labels are written when it's laid out).
    pub fn set_x_format(&mut self, format: Option<ValueFormat>) {
        self.x_format = format;
    }

    /// Series `i`'s value as it's written.
    fn written(&self, i: usize, v: f64) -> String {
        match &self.format {
            Some(f) => (f.0)(i, v),
            None => value_label(v),
        }
    }

    /// What it's called to a screen reader.
    pub fn set_title(&mut self, title: &str) {
        title.clone_into(&mut self.title);
    }

    /// How the app is woken when its lines are rasterized.
    pub fn set_waker(&mut self, waker: &Option<Wake>) {
        self.raster.set_waker(waker.clone());
    }

    /// Its lines rasterized meanwhile taken: `true` to draw again.
    pub fn poll(&mut self) -> bool {
        let came = self.raster.poll();
        if came {
            self.motion.pieces_came();
        }
        came
    }

    /// Something moves (a transition, the lit element lighting up): a frame is wanted.
    /// Something moves in it, as drawn: never while it isn't drawn (not yet, or let go with
    /// [`Self::forget`]) — frames asked for nothing seen would spin.
    pub fn animating(&self) -> bool {
        self.drawn() && self.motion.moving()
    }

    /// It's drawn (its area as last painted, not forgotten since).
    fn drawn(&self) -> bool {
        self.area.size.width > 0.0 && self.area.size.height > 0.0
    }

    /// Its lines are being rasterized.
    pub fn busy(&self) -> bool {
        self.raster.busy()
    }

    /// The series hidden from its legend.
    pub fn hidden(&self) -> &[bool] {
        &self.hidden
    }

    /// Series (or a pie's slice) `i` shown or hidden.
    pub fn toggle(&mut self, i: usize) {
        if let Some(h) = self.hidden.get_mut(i) {
            *h = !*h;
        }
    }

    /// All of X shown again.
    pub fn reset_zoom(&mut self) {
        self.zoom = None;
    }

    /// What it holds let go while it's not shown (its masks).
    pub fn forget(&mut self) {
        self.raster.clear();
        self.laid = None;
        self.area = Rect::default();
    }

    /// The layout it was drawn with last.
    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// It as drawn last, written as SVG in `look`'s colors (`None`: nothing drawn).
    pub fn svg(&self, look: &ChartLook) -> Option<String> {
        let data = self.data.as_ref()?;
        if self.geometry.area.size.width <= 0.0 {
            return None;
        }
        let svg_look = charts_model::svg::SvgLook {
            colors: look.palette().to_vec(),
            text: look.text,
            muted: look.muted,
            grid: look.grid,
            zero: look.zero,
            background: look.background,
            font_family: SVG_FONTS.to_owned(),
            font_size: look.label_font.size,
            line_width: look.line_width * self.scale.max(1.0),
            area_alpha: look::AREA_ALPHA,
        };
        Some(charts_model::svg::svg(data, &self.geometry, &svg_look))
    }

    /// Draws it in `rect` with `look` (its masks asked of the worker when its layout changed).
    pub fn paint(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        look: &ChartLook,
        rect: Rect,
    ) {
        self.area = rect;
        self.scale = ctx.scale;
        self.parts = look.parts;
        self.motion.asked = look.motion.effective();
        self.motion.settle();
        let Some(data) = self.data.clone() else {
            self.geometry = Geometry::default();
            return;
        };
        self.lay_out(&data, text, ctx, look);
        let key = self.laid.unwrap_or(0) ^ look.stamp();
        self.raster.ask(key, &self.geometry, look, data.kind.stacked());
        ui_kit::access::node(|| {
            use ui_kit::access::{Described, Role};
            Described::new(Role::Image, self.title.as_str(), rect).value(summary(&data))
        });
        let g = &self.geometry;
        if look.parts.grid {
            for t in &g.y_ticks {
                let line = Rect::new(g.plot.min_x(), t.at.round(), g.plot.size.width, ctx.line());
                layer.quad(Quad::filled(line, ctx.fade(look.grid)));
            }
        }
        if let Some(zero) = g.zero.filter(|_| look.parts.y_axis) {
            let line = Rect::new(g.plot.min_x(), zero.round(), g.plot.size.width, ctx.line());
            layer.quad(Quad::filled(line, ctx.fade(look.zero)));
        }
        self.paint_lanes(layer, text, ctx, look, &data);
        self.paint_masks(layer, ctx);
        self.paint_marks(layer, ctx, look);
        self.paint_axes(layer, text, ctx, look);
        self.paint_legend(layer, text, ctx, look);
        if look.parts.hover {
            self.paint_hover(layer, ctx, look);
        }
    }

    fn lay_out(&mut self, data: &Prepared, text: &mut TextSystem, ctx: &Ctx<'_>, look: &ChartLook) {
        let r = self.area;
        let key = laid_key(self, r, ctx.scale, look.parts);
        if self.laid == Some(key) {
            return;
        }
        let font = &look.label_font;
        let mut metrics = Metrics::at(ctx.scale);
        metrics.label_h = line_height(&text.metrics(font));
        // The values' labels measured as they're written (the owner's format).
        let format = self.format.clone();
        let write = move |v: f64| format.as_ref().map_or_else(String::new, |f| (f.0)(0, v));
        let view = View {
            area: r,
            options: self.options,
            metrics,
            hidden: &self.hidden,
            zoom: self.zoom,
            axes: look.parts.x_axis || look.parts.y_axis,
            legend: look.parts.legend,
            y_range: self.y_range,
            lanes: self.lanes,
            y_label: self.format.is_some().then_some(charts_model::geometry::Written(&write)),
        };
        self.geometry =
            charts_model::lay_out(data, &view, &mut |s: &str| text.shape_line(s, font).width);
        self.laid = Some(key);
        // The ticks' labels as they'll be drawn: formatted, cut to their room (once here).
        let g = &self.geometry;
        self.y_labels = g
            .y_ticks
            .iter()
            .map(|t| match &self.format {
                Some(f) => (f.0)(0, t.value),
                None => t.label.clone(),
            })
            .collect();
        let pad = ctx.px(4.0);
        let room = g.x_ticks.windows(2).map(|w| w[1].at - w[0].at).fold(f32::INFINITY, f32::min);
        let room = if room.is_finite() { room - pad } else { g.plot.size.width };
        self.x_labels = g
            .x_ticks
            .iter()
            .map(|t| {
                let label = match &self.x_format {
                    Some(f) => (f.0)(0, t.value),
                    None => t.label.clone(),
                };
                crate::ds::ellipsize(text, &label, font, room.max(ctx.px(24.0)))
            })
            .collect();
        // The labels cut short, and where they're drawn (as `paint_axes` places them).
        let m = text.metrics(font);
        let lh = line_height(&m);
        let top = g.plot.max_y() + pad;
        self.cut_labels.clear();
        for (i, (t, shown)) in g.x_ticks.iter().zip(&self.x_labels).enumerate() {
            if shown.ends_with('…') && self.x_format.is_none() {
                let w = text.shape_line(shown, font).width;
                let x = (t.at - w / 2.0).clamp(r.min_x(), (r.max_x() - w).max(r.min_x()));
                self.cut_labels.push((Rect::new(x, top, w, lh), i));
            }
        }
    }

    /// In lanes: each band's faint ground and its series' name on its left.
    fn paint_lanes(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        look: &ChartLook,
        data: &Prepared,
    ) {
        let font = &look.label_font;
        let m = text.metrics(font);
        for &(s, band) in &self.geometry.bands {
            layer.quad(
                Quad::filled(band, ctx.fade(look.grid.with_alpha(look.grid.a * LANE_GROUND)))
                    .rounded(ctx.px(look.bar_radius)),
            );
            let shaped = text.shape_line(data.series_name(s), font);
            let x = self.area.min_x();
            let base =
                ui_kit::centered_baseline(band.min_y(), line_height(&m).min(band.size.height), &m);
            draw_text(layer, &shaped, x, base, ctx.fade(look.muted));
        }
    }

    /// The rasterized lines, areas and slices (the hovered slice moved out).
    fn paint_masks(&self, layer: &mut Layer, ctx: &Ctx<'_>) {
        let Some(pieces) = self.raster.pieces() else { return };
        let o = self.area.origin;
        let (fade_in, fade_out) = self.motion.masks();
        // The old picture fading out under the new one (new data).
        if let Some(old) = self.motion.old_pieces.as_ref().filter(|_| fade_out > 0.0) {
            for piece in old.iter() {
                let at = Rect::new(
                    o.x + piece.at.min_x(),
                    o.y + piece.at.min_y(),
                    piece.at.size.width,
                    piece.at.size.height,
                );
                layer.image_faded(&piece.image, at, fade_out);
            }
        }
        let lit_amount = self.motion.lit_amount();
        let pie = self.data.as_ref().is_some_and(|d| d.kind == charts_model::ChartKind::Pie);
        let lit = match self.hit {
            Some(Hit::Mark { index, .. }) if pie => Some(index),
            _ if pie => self.lit_by_owner,
            _ => None,
        };
        let out = |slice: usize| -> Point {
            let found = self.geometry.marks.iter().find(|m| m.index == slice);
            match found.map(|m| m.shape) {
                // The whole pie (a single slice) stays where it is: moved, it would only
                // look off its place.
                Some(MarkShape::Slice { start, end, .. }) if !whole_turn(start, end) => {
                    let mid = (start + end) / 2.0;
                    let d = ctx.px(SLICE_OUT) * lit_amount;
                    Point::new(mid.sin() * d, -mid.cos() * d)
                }
                _ => Point::default(),
            }
        };
        for piece in pieces.iter() {
            let shift = match (piece.slice, lit) {
                (Some(s), Some(l)) if s == l => out(s),
                _ => Point::default(),
            };
            let at = Rect::new(
                o.x + piece.at.min_x() + shift.x,
                o.y + piece.at.min_y() + shift.y,
                piece.at.size.width,
                piece.at.size.height,
            );
            layer.image_faded(&piece.image, at, fade_in);
        }
    }

    /// Bars and points, from quads (the one under the mouse lit, lighting up smoothly),
    /// grown or moved while a transition runs.
    fn paint_marks(&self, layer: &mut Layer, ctx: &Ctx<'_>, look: &ChartLook) {
        let lit = match self.hit {
            Some(Hit::Mark { series, index }) => Some((series, index)),
            _ => None,
        };
        let slot = match self.hit {
            Some(Hit::Slot(i)) => Some(i),
            _ => None,
        };
        let pie = self.data.as_ref().is_some_and(|d| d.kind == charts_model::ChartKind::Pie);
        let amount = self.motion.lit_amount();
        let base = self.geometry.zero.unwrap_or(self.geometry.plot.max_y());
        let paint = |layer: &mut Layer,
                     m: &charts_model::geometry::Mark,
                     shape: MarkShape,
                     alpha: f32,
                     on: bool| {
            let color = if pie { look.series(m.index) } else { look.series(m.series) };
            let color = if on { color.mix(look.text, 0.18 * amount) } else { color };
            let color = color.with_alpha(color.a * alpha);
            match shape {
                MarkShape::Bar(r) => {
                    let radius = ctx.px(look.bar_radius).min(r.size.width / 2.0);
                    layer.quad(Quad::filled(r, ctx.fade(color)).rounded(radius));
                }
                MarkShape::Point { center, radius } => {
                    let r = if on { radius * (1.0 + 0.5 * amount) } else { radius };
                    let dot = Rect::new(center.x - r, center.y - r, 2.0 * r, 2.0 * r);
                    let alpha = if on { 1.0 } else { 0.8 };
                    layer.quad(
                        Quad::filled(dot, ctx.fade(color.with_alpha(color.a * alpha)))
                            .rounded(r)
                            .bordered(
                                ctx.line(),
                                ctx.fade(look.background.with_alpha(look.background.a * color.a)),
                            ),
                    );
                }
                MarkShape::Slice { .. } => {}
            }
        };
        for (m, alpha) in self.motion.leaving() {
            paint(layer, m, m.shape, alpha, false);
        }
        for m in &self.geometry.marks {
            let on = lit == Some((m.series, m.index)) || slot == Some(m.index);
            let (shape, alpha) = self.motion.mark(m, base);
            paint(layer, m, shape, alpha, on);
        }
    }

    /// The ticks' labels: the values' on the left, the categories' (or X's) under the plot.
    fn paint_axes(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        look: &ChartLook,
    ) {
        let g = &self.geometry;
        let font = &look.label_font;
        let m = text.metrics(font);
        let lh = line_height(&m);
        let pad = ctx.px(4.0);
        if look.parts.y_axis {
            for (t, label) in g.y_ticks.iter().zip(&self.y_labels) {
                let shaped = text.shape_line(label, font);
                let x = g.plot.min_x() - pad * 2.0 - shaped.width;
                let base = ui_kit::centered_baseline(t.at - lh / 2.0, lh, &m);
                draw_text(layer, &shaped, x, base, ctx.fade(look.muted));
            }
        }
        if look.parts.x_axis {
            let room =
                g.x_ticks.windows(2).map(|w| w[1].at - w[0].at).fold(f32::INFINITY, f32::min);
            let room = if room.is_finite() { room - pad } else { g.plot.size.width };
            let top = g.plot.max_y() + pad;
            for t in &g.x_ticks {
                let label = crate::ds::ellipsize(text, &t.label, font, room.max(ctx.px(24.0)));
                let shaped = text.shape_line(&label, font);
                let x = (t.at - shaped.width / 2.0).clamp(
                    self.area.min_x(),
                    (self.area.max_x() - shaped.width).max(self.area.min_x()),
                );
                let base = ui_kit::centered_baseline(top, lh, &m);
                draw_text(layer, &shaped, x, base, ctx.fade(look.muted));
            }
        }
    }

    /// The legend: a swatch and a name per series (a hidden one hollow and muted).
    fn paint_legend(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ctx: &Ctx<'_>,
        look: &ChartLook,
    ) {
        let font = &look.label_font;
        let m = text.metrics(font);
        for item in &self.geometry.legend {
            let color = look.series(item.index);
            let swatch = Quad::filled(
                item.swatch,
                ctx.fade(if item.hidden { color.with_alpha(0.0) } else { color }),
            )
            .rounded(ctx.px(2.0))
            .bordered(ctx.line(), ctx.fade(color));
            layer.quad(swatch);
            let shaped = text.shape_line(&item.name, font);
            let base = ui_kit::centered_baseline(item.area.min_y(), item.area.size.height, &m);
            let ink =
                if item.hidden { look.muted.with_alpha(look.muted.a * 0.6) } else { look.text };
            draw_text(layer, &shaped, item.label.x, base, ctx.fade(ink));
        }
    }

    /// Over a category's column: a guide down the plot and a dot on each line there.
    fn paint_hover(&self, layer: &mut Layer, ctx: &Ctx<'_>, look: &ChartLook) {
        let Some(Hit::Slot(i)) = self.hit else { return };
        let g = &self.geometry;
        let Some(&x) = g.slots.get(i) else { return };
        if g.levels.is_empty() {
            return;
        }
        let amount = self.motion.lit_amount();
        let guide = Rect::new(x.round(), g.plot.min_y(), ctx.line(), g.plot.size.height);
        layer.quad(Quad::filled(guide, ctx.fade(look.zero.with_alpha(look.zero.a * amount))));
        let r = ctx.px(3.5) * (0.6 + 0.4 * amount);
        for level in &g.levels {
            let Some(Some(y)) = level.ys.get(i) else { continue };
            let dot = Rect::new(x - r, y - r, 2.0 * r, 2.0 * r);
            layer.quad(
                Quad::filled(dot, ctx.fade(look.series(level.series)))
                    .rounded(r)
                    .bordered(ctx.px(1.5), ctx.fade(look.background)),
            );
        }
    }

    /// What the tooltip says of `hit`.
    fn told(&self, hit: Hit) -> Option<String> {
        let data = self.data.as_ref()?;
        let shown = |s: usize| !self.hidden.get(s).copied().unwrap_or(false);
        match (hit, &data.shape) {
            (Hit::Legend(_), _) => None,
            (Hit::Slot(i), Shape::Categories { labels, series, .. }) => {
                let mut lines = vec![labels.get(i)?.clone()];
                for (s, v) in series.iter().enumerate().filter(|(s, _)| shown(*s)) {
                    if let Some(Some(value)) = v.values.get(i) {
                        lines.push(format!("{}: {}", v.name, self.written(s, *value)));
                    }
                }
                Some(lines.join("\n"))
            }
            (Hit::Mark { series, index }, Shape::Categories { labels, series: all, .. }) => {
                let s = all.get(series)?;
                let v = (*s.values.get(index)?)?;
                Some(format!("{}\n{}: {}", labels.get(index)?, s.name, self.written(series, v)))
            }
            (Hit::Mark { series, index }, Shape::Points(points)) => {
                let s = points.get(series)?;
                let [x, y, size] = *s.points.get(index)?;
                let x =
                    if data.x_time { charts_model::scale::moment_label(x) } else { value_label(x) };
                let size = if data.kind == charts_model::ChartKind::Bubble {
                    format!(" · {}", value_label(size))
                } else {
                    String::new()
                };
                Some(format!("{}\n{x}, {}{size}", s.name, value_label(y)))
            }
            (Hit::Mark { index, .. }, Shape::Slices { slices, rest }) => {
                let total: f64 =
                    slices.iter().map(|s| s.value).sum::<f64>() + rest.map_or(0.0, |r| r.1);
                let (name, value) = match slices.get(index) {
                    Some(s) => (s.name.clone(), s.value),
                    None => {
                        let (n, sum) = (*rest)?;
                        (format!("+{n}"), sum)
                    }
                };
                let share = if total > 0.0 { value / total * 100.0 } else { 0.0 };
                // In its unit (the owner's format), its share as the language writes it.
                let share = i18n::decimal(format!("{share:.1}%"));
                Some(format!("{name}\n{} · {share}", self.written(0, value)))
            }
            (Hit::Mark { index, .. } | Hit::Slot(index), Shape::Bins { name, bins }) => {
                let b = bins.get(index)?;
                Some(format!(
                    "{name}: {} – {}\n{}",
                    value_label(b.from),
                    value_label(b.to),
                    value_label(b.count)
                ))
            }
            _ => None,
        }
    }

    /// The zoom's range now (all of X when not zoomed).
    fn range(&self) -> Option<(f64, f64)> {
        let s = self.geometry.x_scale?;
        Some(self.zoom.unwrap_or((s.lo, s.hi)))
    }
}

/// A line of text's height in `m`'s font.
fn line_height(m: &text_layout::FontMetrics) -> f32 {
    (m.ascent + m.descent).ceil()
}

/// How many entries its legend has: its series (a pie: its slices, the rest one more).
fn legend_len(d: &Prepared) -> usize {
    match &d.shape {
        Shape::Slices { slices, rest } => slices.len() + usize::from(rest.is_some()),
        _ => d.series_count(),
    }
}

/// A value as a tooltip says it: up to four significant digits, thousands as k.
fn value_label(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".to_owned();
    }
    let step = 10f64.powf(v.abs().log10().floor() - 3.0);
    number_label(v, step.max(1e-6))
}

/// What a screen reader is told of a chart: its kind, how many places and series, the
/// largest value.
fn summary(d: &Prepared) -> String {
    let (places, top) = match &d.shape {
        Shape::Categories { labels, series, .. } => (
            labels.len(),
            series.iter().flat_map(|s| s.values.iter().flatten()).copied().fold(f64::NAN, f64::max),
        ),
        Shape::Points(p) => (p.iter().map(|s| s.points.len()).sum(), f64::NAN),
        Shape::Slices { slices, .. } => {
            (slices.len(), slices.first().map_or(f64::NAN, |s| s.value))
        }
        Shape::Bins { bins, .. } => {
            (bins.len(), bins.iter().map(|b| b.count).fold(f64::NAN, f64::max))
        }
    };
    let kind = format!("{:?}", d.kind).to_lowercase();
    let top = if top.is_finite() { format!(", max {}", value_label(top)) } else { String::new() };
    format!("{kind}, {places} × {}{top}", d.series_count())
}

impl Control for Chart {
    fn area(&self) -> Option<Rect> {
        Some(self.area).filter(|a| a.size.width > 0.0)
    }

    /// The legend toggles its series; a double click shows all of X; a press on a measured X
    /// begins a drag along it.
    fn press(&mut self, p: Point, clicks: u32) -> Press {
        if let Some(Hit::Legend(i)) = self.geometry.hit(p) {
            self.toggle(i);
            return Press::Took;
        }
        if !self.parts.zoom || !self.geometry.plot.contains(p) {
            return Press::Ignored;
        }
        if clicks >= 2 && self.zoom.is_some() {
            self.reset_zoom();
            return Press::Took;
        }
        match self.range() {
            Some(range) => {
                self.pan = Some(Pan { from: p.x, range });
                Press::Held
            }
            None => Press::Ignored,
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        let (Some(pan), Some(scale)) = (self.pan, self.geometry.x_scale) else { return false };
        let width = f64::from(self.geometry.plot.size.width.max(1.0));
        let span = pan.range.1 - pan.range.0;
        let moved = f64::from(p.x - pan.from) / width * span;
        let (whole_lo, whole_hi) = (scale.lo.min(pan.range.0), scale.hi.max(pan.range.1));
        let lo = (pan.range.0 - moved).clamp(whole_lo, whole_hi - span);
        self.zoom = Some((lo, lo + span));
        true
    }

    fn release(&mut self) -> bool {
        self.pan.take().is_some()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.mouse = p;
        let hit = if self.parts.hover { p.and_then(|p| self.geometry.hit(p)) } else { None };
        let changed = hit != self.hit;
        if changed && hit.is_some_and(|h| !matches!(h, Hit::Legend(_))) {
            self.motion.lit();
        }
        self.hit = hit;
        changed
    }

    fn animating(&self) -> bool {
        self.motion.moving()
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        matches!(self.geometry.hit(p), Some(Hit::Legend(_))).then_some(ViewerCursor::Pointer)
    }

    /// The wheel zooms around the mouse along a measured X.
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if !self.parts.zoom || dy == 0.0 {
            return false;
        }
        let (Some(mouse), Some((lo, hi)), Some(whole)) =
            (self.mouse, self.range(), self.geometry.x_scale)
        else {
            return false;
        };
        let plot = self.geometry.plot;
        if !plot.contains(mouse) {
            return false;
        }
        let t = f64::from((mouse.x - plot.min_x()) / plot.size.width.max(1.0));
        let at = lo + t * (hi - lo);
        let factor = (1.0 + ZOOM_STEP).powf(f64::from(dy.signum()));
        let whole_span = (whole.hi - whole.lo).max(f64::MIN_POSITIVE);
        let span = ((hi - lo) * factor).clamp(whole_span * ZOOM_LEAST, whole_span);
        let lo = (at - t * span).clamp(whole.lo, whole.hi - span);
        self.zoom = (span < whole_span).then_some((lo, lo + span));
        true
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        // An X label cut short: said whole.
        if let Some((r, i)) = self.cut_labels.iter().find(|(r, _)| r.contains(p)) {
            return Some(Tip::new(*r, self.geometry.x_ticks.get(*i)?.label.clone()));
        }
        // The legend: its series' name, and what a press does.
        if let Some(item) = self.geometry.legend.iter().find(|l| l.area.contains(p)) {
            let key = if item.hidden { "charts.legend_show" } else { "charts.legend_hide" };
            return Some(Tip::new(item.area, format!("{}\n{}", item.name, i18n::t(key))));
        }
        if !self.parts.hover {
            return None;
        }
        let hit = self.geometry.hit(p)?;
        Some(Tip::new(mouse_anchor(p), self.told(hit)?))
    }
}

/// A tooltip's anchor at the mouse: wide, so the tip follows it.
fn mouse_anchor(p: Point) -> Rect {
    let half = 400.0;
    Rect::new(p.x - half, p.y, 2.0 * half, 1.0)
}

/// A slice from `start` to `end` is the whole turn (the only one).
pub(crate) fn whole_turn(start: f32, end: f32) -> bool {
    end - start >= std::f32::consts::TAU - 1e-3
}

#[cfg(test)]
#[path = "../../tests/unit/charts/mod.rs"]
mod tests;
