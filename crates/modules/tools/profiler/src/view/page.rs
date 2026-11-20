//! The recording's page, drawn from our components: a header (what it is, where it ran,
//! its actions), "What stands out" (each finding with its severity's icon), the figures as
//! metric cards with their charts, the timeline (the CPU and the frames over the recording,
//! a time axis, the values under the mouse), then tables in the look every table has
//! (`ui_components::table`: the header band with subtitles, alternating rows, the lines
//! between columns), a share drawn as a bar in its cell.

use i18n::t;
use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::chart::{Plot, Reduce, Samples, Style, sparkline};
use ui_components::ds::{self, Ctx, radius};
use ui_components::metric_card::{CardForm, CardSeries, MetricCard, metric_card};
use ui_components::table;

/// The page's widest column, its padding, the gap between its blocks (logical pixels).
pub const MAX_W: f32 = 1240.0;
pub const PAD: f32 = 28.0;
const GAP: f32 = 22.0;
/// A table's row, a finding's row, a metric card, the timeline (logical pixels).
const ROW_H: f32 = 30.0;
const FINDING_H: f32 = 34.0;
const CARD_H: f32 = 118.0;
const TIMELINE_H: f32 = 150.0;
/// The flame graph's band and axis, and each of its levels (logical pixels).
const FLAME_TOP: f32 = 66.0;
const FLAME_ROW: f32 = 25.0;
/// The rows a table shows at most.
pub const ROWS: usize = 12;

/// What the page's controls are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Copy,
    Reveal,
}

/// A table's column: its name, its subtitle, its share of the width, numbers on the right.
pub struct Col<'a> {
    pub name: &'a str,
    pub sub: &'a str,
    pub share: f32,
    pub right: bool,
}

/// A table's cell.
pub enum Cell {
    Text(String),
    /// Muted (a detail beside the name).
    Muted(String),
    /// A share (0..=1) drawn as a bar in its tone, its value said after it.
    Bar(f32, String, ds::Tone),
}

/// A metric card: its label, value, line under it, chart, color, chart's top.
pub type Card<'a> = (&'a str, String, String, Option<&'a Plot>, Rgba, f32);

/// A timeline's series: its label, its plot, color, top, values per sample, unit.
pub type Series<'a> = (&'a str, &'a Plot, Rgba, f32, &'a [f32], &'a str);

/// Where the page is drawn and what it found.
pub struct Page<'a, 'c> {
    pub scene: &'a mut Scene,
    pub text: &'a mut TextSystem,
    pub icons: &'a Icons,
    pub ds: &'a Ctx<'c>,
    /// Its column, the next block's top.
    pub x: f32,
    pub width: f32,
    pub y: f32,
    /// The visible area (blocks wholly outside it are skipped).
    pub clip: Rect,
    pub mouse: Option<Point>,
    pub hits: Vec<(Rect, Hit)>,
    pub hover: Option<Hit>,
    /// The timeline's chart area, for the mouse's reading.
    pub timeline: Option<Rect>,
    /// The flame graph's area (its clicks, its hover).
    pub flame: Option<Rect>,
    /// The tables' rows as drawn: where, and their place in the rows given.
    pub rows: Vec<(Rect, usize)>,
    /// The row lit (under the mouse) in a table whose rows are clickable.
    pub lit_row: Option<usize>,
    /// The rows a table shows at most.
    pub max_rows: usize,
}

/// The page's column in `bounds`: its left and its width (centered, at most [`MAX_W`],
/// the scrollbar's lane kept free).
pub fn column(ds: &Ctx<'_>, bounds: Rect) -> (f32, f32) {
    let lane = ui_components::Scrollbar::track_thickness(ds.scale) + ds.px(8.0);
    let room = (bounds.size.width - 2.0 * ds.px(PAD) - lane).max(0.0);
    let width = room.min(ds.px(MAX_W));
    (bounds.min_x() + ds.px(PAD) + ((room - width) / 2.0).max(0.0), width)
}

impl<'a, 'c> Page<'a, 'c> {
    /// A page whose column is `(x, width)` and first block's top `y`, drawn within `clip`.
    pub fn new(
        scene: &'a mut Scene,
        text: &'a mut TextSystem,
        icons: &'a Icons,
        ds: &'a Ctx<'c>,
        (x, width, y): (f32, f32, f32),
        clip: Rect,
    ) -> Self {
        Self {
            scene,
            text,
            icons,
            ds,
            x,
            width,
            y,
            clip,
            mouse: None,
            hits: Vec::new(),
            hover: None,
            timeline: None,
            flame: None,
            rows: Vec::new(),
            lit_row: None,
            max_rows: ROWS,
        }
    }
}

impl Page<'_, '_> {
    fn px(&self, v: f32) -> f32 {
        self.ds.px(v)
    }

    /// The file still being read: a loader where the page starts.
    pub fn loading(&mut self) {
        let reading = t("profiler.reading");
        let loader = ds::Loader::with_text(&reading);
        let top = self.clip.min_y() + self.px(PAD);
        let layer = self.scene.push_layer(Some(self.clip));
        ds::loader(layer, self.text, self.ds, self.x, top, &loader);
    }

    /// The header: its title, where it ran, its actions on the right.
    pub fn header(&mut self, title: &str, meta: &str) {
        let ds = self.ds;
        let p = ds.palette;
        let h = self.px(56.0);
        let (copy, reveal) = (t("profiler.copy_summary"), t("profiler.reveal"));
        let bw = ds::primary_width(self.text, ds, &[&copy, &reveal]);
        let bh = self.px(32.0);
        let reveal_r = Rect::new(self.x + self.width - bw, self.y + self.px(6.0), bw, bh);
        let copy_r = Rect::new(reveal_r.min_x() - self.px(8.0) - bw, reveal_r.min_y(), bw, bh);
        let (hover_copy, hover_reveal) =
            (self.hover == Some(Hit::Copy), self.hover == Some(Hit::Reveal));
        let (x, y, room) = (self.x, self.y, copy_r.min_x() - self.x - self.px(16.0));
        let text = &mut *self.text;
        let layer = self.scene.push_layer(Some(self.clip));
        let big = ds.ui_font(7.0).with_weight(Font::SEMIBOLD);
        ds::draw_ellipsized(layer, text, title, &big, (x, y + ds.px(24.0)), room, p.text);
        let small = ds.ui_font(-1.0);
        ds::draw_ellipsized(layer, text, meta, &small, (x, y + ds.px(46.0)), room, p.text_muted);
        ds::secondary_button(layer, text, ds, copy_r, &copy, true, hover_copy);
        ds::secondary_button(layer, text, ds, reveal_r, &reveal, true, hover_reveal);
        self.hits.push((copy_r, Hit::Copy));
        self.hits.push((reveal_r, Hit::Reveal));
        self.y += h + self.px(GAP);
    }

    /// A block's title over it.
    pub fn title(&mut self, title: &str, detail: &str) {
        let ds = self.ds;
        let (x, y, width) = (self.x, self.y, self.width);
        let text = &mut *self.text;
        let layer = self.scene.push_layer(Some(self.clip));
        let font = ds.ui_font(1.0).with_weight(Font::SEMIBOLD);
        let (drawn, _) = ds::draw_ellipsized(
            layer,
            text,
            title,
            &font,
            (x, y + ds.px(16.0)),
            width,
            ds.palette.text,
        );
        if !detail.is_empty() {
            let small = ds.ui_font(-1.5);
            let dx = x + drawn + ds.px(10.0);
            ds::draw_ellipsized(
                layer,
                text,
                detail,
                &small,
                (dx, y + ds.px(16.0)),
                (width - drawn).max(0.0),
                ds.palette.text_muted,
            );
        }
        self.y += self.px(28.0);
    }

    /// A card's surface (as the page's others: rounded, its edge).
    fn card(layer: &mut Layer, ds: &Ctx<'_>, r: Rect) {
        layer.quad(
            Quad::filled(r, ds.palette.text.with_alpha(0.025))
                .rounded(ds.px(radius::MD))
                .bordered(ds.line(), ds.palette.card_border),
        );
    }

    /// What stands out: each finding on its row, its severity's icon and color before it,
    /// what to do under it (muted); none: said so, in the success's tone.
    pub fn findings(&mut self, rows: &[(ui_components::severity::Level, String, String)]) {
        let ds = self.ds;
        let p = ds.palette;
        let row_h = ds.px(if rows.is_empty() { FINDING_H } else { FINDING_H + 18.0 });
        let h = row_h * rows.len().max(1) as f32 + ds.px(12.0);
        let r = Rect::new(self.x, self.y, self.width, h);
        let (icons, text) = (self.icons, &mut *self.text);
        let layer = self.scene.push_layer(Some(self.clip));
        Self::card(layer, ds, r);
        let font = ds.ui_font(-0.5);
        let small = ds.ui_font(-1.5);
        let s = ds.px(16.0);
        let mut y = r.min_y() + ds.px(6.0);
        let calm = [(
            ui_components::severity::Level::Info,
            t("profiler.nothing_stands_out"),
            String::new(),
        )];
        let shown: &[(ui_components::severity::Level, String, String)] =
            if rows.is_empty() { &calm } else { rows };
        for (i, (level, sentence, advice)) in shown.iter().enumerate() {
            if i > 0 {
                ds::divider(layer, ds, r.min_x() + ds.px(14.0), r.max_x() - ds.px(14.0), y);
            }
            let color = if rows.is_empty() { ds::Tone::Success.color(p) } else { level.color(p) };
            let name = if rows.is_empty() { "check" } else { level.icon() };
            let first = y + ds.px(FINDING_H) / 2.0;
            if let Some(id) = icons.id(name) {
                let at = Rect::new(r.min_x() + ds.px(14.0), (first - s / 2.0).round(), s, s);
                ui_kit::draw_icon(layer, icons, id, at, color);
            }
            let tx = r.min_x() + ds.px(42.0);
            let room = r.max_x() - tx - ds.px(14.0);
            let base = ds::optical_baseline(y, ds.px(FINDING_H), &font);
            ds::draw_ellipsized(layer, text, sentence, &font, (tx, base), room, p.text);
            if !advice.is_empty() {
                let base = base + ds.px(19.0);
                let line = format!("→ {advice}");
                ds::draw_ellipsized(layer, text, &line, &small, (tx, base), room, p.text_muted);
            }
            y += row_h;
        }
        self.y += h + self.px(GAP);
    }

    /// The figures: a metric card each, its chart under its value.
    pub fn cards(&mut self, cards: &[Card<'_>]) {
        let ds = self.ds;
        let gap = self.px(12.0);
        let n = cards.len().max(1) as f32;
        let w = ((self.width - gap * (n - 1.0)) / n).floor();
        let h = self.px(CARD_H);
        let (icons, text) = (self.icons, &mut *self.text);
        let layer = self.scene.push_layer(Some(self.clip));
        for (i, (label, value, sub, plot, color, max)) in cards.iter().enumerate() {
            let r = Rect::new(self.x + i as f32 * (w + gap), self.y, w, h);
            let series: Vec<CardSeries<'_>> =
                plot.iter().map(|plot| CardSeries { plot, color: *color, label }).collect();
            let card = MetricCard {
                label,
                value,
                sub: Some(sub.as_str()).filter(|s| !s.is_empty()),
                warning: false,
                series: &series,
                max: *max,
                hovered: false,
                active: false,
                focused: false,
            };
            metric_card(layer, text, icons, ds, r, &card, CardForm::Card);
        }
        self.y += h + self.px(GAP);
    }

    /// The timeline: the CPU (an area) and the frames (a second one under it) over the
    /// recording, a time axis under both, and under the mouse a line with what was then.
    pub fn timeline(&mut self, series: &[Series<'_>], seconds: f64) {
        let ds = self.ds;
        let p = ds.palette;
        let lane = self.px(TIMELINE_H) / series.len().max(1) as f32;
        let axis_h = self.px(24.0);
        let h = lane * series.len() as f32 + axis_h + self.px(20.0);
        let r = Rect::new(self.x, self.y, self.width, h);
        let label_w = self.px(140.0);
        let chart = Rect::new(
            r.min_x() + label_w,
            r.min_y() + self.px(10.0),
            r.size.width - label_w - self.px(14.0),
            lane * series.len() as f32,
        );
        let (text, mouse) = (&mut *self.text, self.mouse);
        let layer = self.scene.push_layer(Some(self.clip));
        Self::card(layer, ds, r);
        let small = ds.ui_font(-1.5);
        for (i, (label, plot, color, max, _, _)) in series.iter().enumerate() {
            let lane_r = Rect::new(
                chart.min_x(),
                chart.min_y() + i as f32 * lane,
                chart.size.width,
                lane - ds.px(6.0),
            );
            layer.quad(Quad::filled(lane_r, p.text.with_alpha(0.02)).rounded(ds.px(radius::SM)));
            sparkline(layer, ds, lane_r, plot, *color, Style::Steps, *max);
            let base = ds::optical_baseline(lane_r.min_y(), ds.px(20.0), &small);
            ds::draw_ellipsized(
                layer,
                text,
                label,
                &small,
                (r.min_x() + ds.px(14.0), base),
                label_w - ds.px(20.0),
                p.text_muted,
            );
        }
        // The time axis: a mark every few seconds (about eight along it).
        let step = nice_step(seconds / 8.0);
        // Too narrow for a chart: no axis (its marks would have nowhere to go).
        let step = if chart.size.width > 0.0 { step } else { 0.0 };
        let mut at = 0.0;
        let axis_y = chart.max_y() + ds.px(4.0);
        while at <= seconds + 1e-6 && step > 0.0 {
            let x = chart.min_x() + (at / seconds.max(1e-9)) as f32 * chart.size.width;
            layer.quad(Quad::filled(
                Rect::new(x.round(), chart.min_y(), ds.line(), chart.size.height),
                p.text.with_alpha(0.06),
            ));
            let label = super::words::length(at);
            let shaped = text.shape_line(&label, &small);
            let lx = (x - shaped.width / 2.0).min(chart.max_x() - shaped.width).max(chart.min_x());
            ui_kit::draw_text(layer, &shaped, lx, axis_y + ds.px(14.0), p.text_muted);
            at += step;
        }
        // The mouse over it: a line, and what each series was then.
        if let Some(m) = mouse.filter(|m| chart.contains(*m)) {
            let x = m.x.round();
            layer.quad(Quad::filled(
                Rect::new(x, chart.min_y(), ds.line().max(1.0), chart.size.height),
                p.accent,
            ));
            let share = ((m.x - chart.min_x()) / chart.size.width).clamp(0.0, 1.0);
            let mut lines = vec![super::words::length(seconds * f64::from(share))];
            for (label, _, _, _, values, unit) in series {
                if values.is_empty() {
                    continue;
                }
                let i = ((values.len() - 1) as f32 * share).round() as usize;
                lines.push(format!("{label}: {:.1}{unit}", values[i.min(values.len() - 1)]));
            }
            let font = ds.ui_font(-1.0);
            let w = lines.iter().map(|l| text.shape_line(l, &font).width).fold(0.0, f32::max)
                + ds.px(20.0);
            let lh = ds.px(18.0);
            let tip_h = lh * lines.len() as f32 + ds.px(12.0);
            let tx = if x + ds.px(12.0) + w > chart.max_x() {
                x - ds.px(12.0) - w
            } else {
                x + ds.px(12.0)
            };
            let tip = Rect::new(tx, chart.min_y(), w, tip_h);
            ds::surface_with(layer, ds, tip, ds::Elevation::Low, radius::SM, p.popup_background);
            for (i, line) in lines.iter().enumerate() {
                let shaped = text.shape_line(line, &font);
                let base =
                    ds::optical_baseline(tip.min_y() + ds.px(6.0) + lh * i as f32, lh, &font);
                ui_kit::draw_text(
                    layer,
                    &shaped,
                    tip.min_x() + ds.px(10.0),
                    base,
                    if i == 0 { p.text_muted } else { p.text },
                );
            }
        }
        self.timeline = Some(chart);
        self.y += h + self.px(GAP);
    }

    /// A table in the look every table has: the header band with each column's name and
    /// subtitle, the rows alternating, the lines between the columns; a share as a bar.
    pub fn table(&mut self, cols: &[Col<'_>], rows: &[Vec<Cell>]) {
        let ds = self.ds;
        let p = ds.palette;
        let header_h = self.px(table::HEADER_H);
        let row_h = self.px(ROW_H);
        let shown = rows.len().clamp(1, self.max_rows.max(1));
        let hidden = rows.len().saturating_sub(shown);
        let more = if hidden > 0 { 1 } else { 0 };
        let h = header_h + row_h * (shown + more) as f32;
        let r = Rect::new(self.x, self.y, self.width, h);
        if r.max_y() < self.clip.min_y() || r.min_y() > self.clip.max_y() {
            self.y += h + self.px(GAP);
            return;
        }
        let total: f32 = cols.iter().map(|c| c.share).sum::<f32>().max(f32::EPSILON);
        let mut edges = vec![r.min_x()];
        for c in cols {
            let last = *edges.last().unwrap_or(&r.min_x());
            edges.push(last + r.size.width * c.share / total);
        }
        let header = Rect::new(r.min_x(), r.min_y(), r.size.width, header_h);
        {
            let layer = self.scene.push_layer(Some(self.clip));
            layer.quad(
                Quad::filled(r, p.text.with_alpha(0.02))
                    .rounded(ds.px(radius::MD))
                    .bordered(ds.line(), p.card_border),
            );
        }
        table::header_band_rounded(self.scene, ds, header, ds.px(radius::MD));
        let text = &mut *self.text;
        let layer = self.scene.push_layer(Some(self.clip));
        let pad = ds.px(table::CELL_PAD);
        for (i, c) in cols.iter().enumerate() {
            let cell = Rect::new(edges[i], header.min_y(), edges[i + 1] - edges[i], header_h);
            let head = table::HeaderCell {
                rect: cell,
                name: c.name,
                subtitle: c.sub,
                right: c.right,
                hovered: false,
                sorted: None,
                strong: true,
                pad,
            };
            table::header_cell(layer, text, ds, &head, 0.0);
            if i + 1 < cols.len() {
                table::header_edge(layer, ds, edges[i + 1], header, false);
            }
        }
        table::header_rule(layer, ds, header);
        let font = ds.ui_font(-0.5);
        let mono = ds.code_font(-1.0);
        if rows.is_empty() {
            let base = ds::optical_baseline(header.max_y(), row_h, &font);
            let empty = t("profiler.nothing");
            ds::draw_ellipsized(
                layer,
                text,
                &empty,
                &font,
                (r.min_x() + pad, base),
                r.size.width,
                p.text_muted,
            );
        }
        for (n, row) in rows.iter().take(self.max_rows.max(1)).enumerate() {
            let top = header.max_y() + row_h * n as f32;
            let line = Rect::new(r.min_x() + ds.line(), top, r.size.width - 2.0 * ds.line(), row_h);
            table::stripe(layer, ds, line, n);
            if self.lit_row == Some(n) {
                layer.quad(Quad::filled(line, p.text.with_alpha(0.05)));
            }
            self.rows.push((line, n));
            table::column_lines(
                layer,
                ds,
                edges[1..edges.len() - 1].iter().copied(),
                top,
                row_h,
                r,
            );
            for (i, cell) in row.iter().enumerate() {
                let Some(c) = cols.get(i) else { continue };
                let (left, right) = (edges[i] + pad, edges[i + 1] - pad);
                let room = (right - left).max(0.0);
                match cell {
                    Cell::Text(s) | Cell::Muted(s) => {
                        let color =
                            if matches!(cell, Cell::Muted(_)) { p.text_muted } else { p.text };
                        let f = if c.right { &mono } else { &font };
                        let base = ds::optical_baseline(top, row_h, f);
                        let w = text.shape_line(s, f).width.min(room);
                        let x = if c.right { right - w } else { left };
                        ds::draw_ellipsized(layer, text, s, f, (x, base), room, color);
                    }
                    Cell::Bar(share, said, color) => {
                        let label_w = ds.px(110.0).min(room * 0.45);
                        let track = Rect::new(
                            left,
                            top + row_h * 0.38,
                            (room - label_w - ds.px(10.0)).max(0.0),
                            row_h * 0.24,
                        );
                        layer
                            .quad(Quad::filled(track, p.text.with_alpha(0.06)).rounded(ds.px(3.0)));
                        let fill_w = (track.size.width * share.clamp(0.0, 1.0)).max(ds.px(2.0));
                        layer.quad(
                            Quad::filled(
                                Rect::new(track.min_x(), track.min_y(), fill_w, track.size.height),
                                color.color(p),
                            )
                            .rounded(ds.px(3.0)),
                        );
                        let base = ds::optical_baseline(top, row_h, &mono);
                        let w = text.shape_line(said, &mono).width.min(label_w);
                        ds::draw_ellipsized(
                            layer,
                            text,
                            said,
                            &mono,
                            (right - w, base),
                            label_w,
                            p.text,
                        );
                    }
                }
            }
        }
        // What didn't fit: said in a row of its own.
        if hidden > 0 {
            let top = header.max_y() + row_h * shown as f32;
            let base = ds::optical_baseline(top, row_h, &font);
            let said = i18n::tr!("profiler.more_rows", count = hidden);
            ds::draw_ellipsized(
                layer,
                text,
                &said,
                &font,
                (r.min_x() + pad, base),
                r.size.width - 2.0 * pad,
                p.text_muted,
            );
        }
        self.y += h + self.px(GAP);
    }
}

impl Page<'_, '_> {
    /// The flame graph in a card (or, nothing timed, said so).
    pub fn flame(&mut self, icicle: &mut ui_components::Icicle, depth: usize) {
        let ds = self.ds;
        let any = depth > 0;
        // Its band, its axis, a row a level (a little room under the deepest).
        let h =
            self.px(if any { FLAME_TOP + FLAME_ROW * depth as f32 + 16.0 } else { ROW_H * 1.5 });
        let r = Rect::new(self.x, self.y, self.width, h);
        {
            let layer = self.scene.push_layer(Some(self.clip));
            Self::card(layer, ds, r);
        }
        let inner = Rect::new(
            r.min_x() + ds.px(8.0),
            r.min_y() + ds.px(8.0),
            r.size.width - ds.px(16.0),
            r.size.height - ds.px(16.0),
        );
        if any && inner.intersect(&self.clip).is_some() {
            icicle.paint(ui_components::IciclePaint {
                bounds: inner,
                scale: ds.scale,
                text: self.text,
                scene: self.scene,
                palette: ds.palette,
                icons: self.icons,
                clip: Some(self.clip),
            });
            self.flame = Some(inner);
        } else if !any {
            let layer = self.scene.push_layer(Some(self.clip));
            let font = ds.ui_font(-0.5);
            let base = ds::optical_baseline(r.min_y(), r.size.height, &font);
            let none = t("profiler.flame_none");
            ds::draw_ellipsized(
                layer,
                self.text,
                &none,
                &font,
                (r.min_x() + ds.px(14.0), base),
                r.size.width,
                ds.palette.text_muted,
            );
        }
        self.y += h + self.px(GAP);
    }
}

/// A round step for a time axis near `want` seconds (1, 2, 5, 10, 15, 30 s, 1, 2, 5, 10 min…).
pub fn nice_step(want: f64) -> f64 {
    const STEPS: [f64; 12] =
        [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 900.0, 1800.0];
    STEPS.iter().copied().find(|s| *s >= want).unwrap_or(3600.0)
}

/// A plot of `values` on `columns` buckets (the highest in each: a spike stays seen).
pub fn plot(values: &[f32], columns: usize) -> Plot {
    Plot::build(Samples::new(values), values.len().max(1), columns.max(1), Reduce::Max)
}
