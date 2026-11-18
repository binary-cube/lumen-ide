//! Diagnostics overlay (F12): the backend, the frames' CPU time, input → screen latency,
//! what a frame drew — a small two-column table (the label muted on the left, the value on
//! the right in monospace figures so it doesn't jump), the values that have a budget colored
//! by it, a sparkline of the last frames, and a × that hides it.
//!
//! It never asks for frames of its own (measuring itself would keep the app busy): it shows
//! what the frames the app draws anyway gathered, its text made again at most twice a
//! second; idle, the last values stay.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use kernel::{Rect, Rgba, Size};
use render_api::{FrameStats, GlyphInstance, Point, Quad};
use text_layout::{Font, FontFamily};
use ui_components::severity::Level;
use ui_components::tooltip::{Tip, Tooltips};

use crate::ds;
use crate::palette::Palette;

/// How many frames go into the displayed average (and the sparkline).
const WINDOW: usize = 60;

/// The table is made again at most this often.
const REFRESH: Duration = Duration::from_millis(500);

/// How a value reads against its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// No budget: the text color.
    Plain,
    Good,
    Warn,
    Bad,
}

/// A value against its budget: within `warn` good, past `bad` bad, warned between.
fn tone(value: f64, warn: f64, bad: f64) -> Tone {
    if value > bad {
        Tone::Bad
    } else if value > warn {
        Tone::Warn
    } else {
        Tone::Good
    }
}

/// One row of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub tone: Tone,
}

#[derive(Debug)]
pub struct DebugOverlay {
    visible: bool,
    backend: String,
    frame_times: [Duration; WINDOW],
    next: usize,
    recorded: usize,
    last: FrameStats,
    /// The surface's size at the last frame (for the share redrawn).
    viewport: Size,
    /// Input → frame-submitted-to-GPU latency, for the latest events.
    latencies: [Duration; WINDOW],
    next_latency: usize,
    recorded_latencies: usize,
    /// The table shown and when it was made.
    rows: Vec<Row>,
    made_at: Option<Instant>,
    /// The frame times the sparkline shows (oldest first), taken with the table.
    spark: Vec<f32>,
    /// The × as last drawn, and whether the mouse is on it.
    close: Option<Rect>,
    close_hovered: bool,
    tooltips: Tooltips,
}

impl DebugOverlay {
    pub fn new(backend: String) -> Self {
        Self {
            visible: false,
            backend,
            frame_times: [Duration::ZERO; WINDOW],
            next: 0,
            recorded: 0,
            last: FrameStats::default(),
            viewport: Size::new(1.0, 1.0),
            latencies: [Duration::ZERO; WINDOW],
            next_latency: 0,
            recorded_latencies: 0,
            rows: Vec::new(),
            made_at: None,
            spark: Vec::with_capacity(WINDOW),
            close: None,
            close_hovered: false,
            tooltips: Tooltips::default(),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        // Shown again: the table from what's gathered now.
        self.made_at = None;
        self.close = None;
        self.close_hovered = false;
        self.tooltips.hide();
    }

    /// Records a frame's CPU time (building the scene + submitting to the GPU).
    pub fn record(&mut self, cpu_time: Duration, stats: FrameStats) {
        if stats.skipped {
            return;
        }
        self.frame_times[self.next] = cpu_time;
        self.next = (self.next + 1) % WINDOW;
        self.recorded = (self.recorded + 1).min(WINDOW);
        self.last = stats;
    }

    /// Records the time from an input event to the frame that reflects it.
    pub fn record_latency(&mut self, latency: Duration) {
        self.latencies[self.next_latency] = latency;
        self.next_latency = (self.next_latency + 1) % WINDOW;
        self.recorded_latencies = (self.recorded_latencies + 1).min(WINDOW);
    }

    /// 95th percentile of input → frame latency.
    pub fn latency_p95(&self) -> Option<Duration> {
        let mut samples = self.latencies[..self.recorded_latencies].to_vec();
        if samples.is_empty() {
            return None;
        }
        samples.sort();
        Some(samples[((samples.len() as f32 * 0.95).ceil() as usize).clamp(1, samples.len()) - 1])
    }

    /// When it needs a frame of its own: only for its ×'s tooltip (appearing, fading in),
    /// never to measure.
    pub fn frame_deadline(&self) -> Option<Instant> {
        if !self.visible {
            return None;
        }
        if self.tooltips.is_animating() {
            return Some(Instant::now());
        }
        self.tooltips.deadline()
    }

    /// The mouse at `p`: whether it's on the × (its hover and tooltip follow). `true` if a
    /// redraw is needed.
    pub fn hover(&mut self, p: Point) -> bool {
        let on = self.visible && self.close.is_some_and(|r| r.contains(p));
        let changed = on != self.close_hovered;
        self.close_hovered = on;
        let tip = self.close.filter(|_| on).map(|r| Tip::new(r, i18n::t("debug.close")));
        self.tooltips.set(tip) | changed
    }

    /// The mouse is on the × (the pointer cursor there).
    pub fn close_hovered(&self) -> bool {
        self.close_hovered
    }

    /// A click at `p` on the ×: hidden (as F12). `true` if it was there.
    pub fn click(&mut self, p: Point) -> bool {
        if self.visible && self.close.is_some_and(|r| r.contains(p)) {
            self.toggle();
            return true;
        }
        false
    }

    /// The table made again from what's gathered, if it's due (at most every [`REFRESH`]).
    /// `true` if it was.
    pub fn refresh(&mut self, now: Instant) -> bool {
        if self.made_at.is_some_and(|at| now.duration_since(at) < REFRESH) {
            return false;
        }
        self.made_at = Some(now);
        let rows = self.make_rows();
        self.spark.clear();
        let oldest = if self.recorded < WINDOW { 0 } else { self.next };
        for i in 0..self.recorded {
            self.spark.push(ms(self.frame_times[(oldest + i) % WINDOW]) as f32);
        }
        let changed = rows != self.rows;
        self.rows = rows;
        changed
    }

    /// The table shown.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    fn make_rows(&self) -> Vec<Row> {
        let mut rows = Vec::with_capacity(6);
        let row =
            |label: &str, value: String, tone: Tone| Row { label: i18n::t(label), value, tone };
        rows.push(row("debug.backend", self.backend.clone(), Tone::Plain));
        let (avg, max) = self.timings();
        let mut value = String::new();
        let _ = write!(value, "{:5.2} / {:5.2} ms", ms(avg), ms(max));
        rows.push(row("debug.frame", value, tone(ms(avg), 8.0, 16.0)));
        let latency = match self.latency_p95() {
            Some(p95) => {
                let p95 = ms(p95);
                (format!("{p95:5.1} ms"), tone(p95, 16.0, 33.0))
            }
            None => (i18n::t("debug.input_none"), Tone::Plain),
        };
        rows.push(row("debug.input", latency.0, latency.1));
        let s = &self.last;
        rows.push(row(
            "debug.drawn",
            format!("{} / {} / {}", s.quads, s.glyphs, s.draw_calls),
            Tone::Plain,
        ));
        rows.push(row("debug.glyph_cache", s.cached_glyphs.to_string(), Tone::Plain));
        let share = drawn_share(s.drawn_pixels, self.viewport) * 100.0;
        rows.push(row("debug.area", format!("{share:3.0}%"), Tone::Plain));
        rows
    }

    /// Draws the panel in the bottom-right corner of `area` (the work area: below the top
    /// bar, above the status bar, already inset by the popover gap), in the palette's popover
    /// colors. Only its × takes clicks: the rest lets them through to what's under it.
    pub fn paint(&mut self, painter: crate::Painter<'_>, area: Rect, viewport: Size, scale: f32) {
        let crate::Painter { text, scene, palette, icons } = painter;
        if !self.visible {
            self.close = None;
            return;
        }
        self.viewport = viewport;
        self.refresh(Instant::now());
        let ctx = ds::Ctx::new(palette, scale);
        let label_font = Font::new(FontFamily::SansSerif, ctx.px(11.0));
        let value_font = Font::new(FontFamily::Monospace, ctx.px(11.0));
        let metrics = text.metrics(&value_font);
        let line_height = (value_font.size * 1.6).round();
        let padding = ctx.px(ds::space::SM + 2.0);
        let gap = ctx.px(ds::space::LG);
        let close_size = ctx.px(18.0);
        let spark_height = ctx.px(22.0);
        let label_width = self
            .rows
            .iter()
            .map(|r| text.shape_line(&r.label, &label_font).width)
            .fold(0.0, f32::max);
        // The values' column as wide as its widest (monospace: the digits keep their places).
        let value_width = self
            .rows
            .iter()
            .map(|r| text.shape_line(&r.value, &value_font).width)
            .fold(0.0, f32::max);
        let width = (label_width + gap + value_width).max(ctx.px(220.0));
        let size = Size::new(
            (width + 2.0 * padding + close_size + ctx.px(4.0)).ceil(),
            (line_height * self.rows.len() as f32 + spark_height + 2.0 * padding + ctx.px(6.0))
                .ceil(),
        );
        let panel = Rect::new(
            (area.max_x() - size.width).max(area.min_x()).round(),
            (area.max_y() - size.height).max(area.min_y()).round(),
            size.width,
            size.height,
        );

        let layer = scene.push_layer(Some(panel));
        layer.quad(
            Quad::filled(panel, palette.popup_background)
                .rounded(ctx.px(ds::radius::MD))
                .bordered(ctx.line(), palette.popup_border),
        );
        let left = panel.min_x() + padding;
        let right = panel.max_x() - padding - close_size - ctx.px(4.0);
        for (i, row) in self.rows.iter().enumerate() {
            let baseline =
                (panel.min_y() + padding + i as f32 * line_height + metrics.ascent).round();
            // The first row is the title (the backend's name in the text color).
            let label_color = if i == 0 { palette.text } else { palette.text_muted };
            let shaped = text.shape_line(&row.label, &label_font);
            layer.glyphs.extend(shaped.glyphs.iter().map(|g| GlyphInstance {
                key: g.key,
                origin: Point::new(left + g.x, baseline + g.y),
                color: label_color,
            }));
            let shaped = text.shape_line(&row.value, &value_font);
            let x = (right - shaped.width).round();
            let color = tone_color(row.tone, palette);
            layer.glyphs.extend(shaped.glyphs.iter().map(|g| GlyphInstance {
                key: g.key,
                origin: Point::new(x + g.x, baseline + g.y),
                color,
            }));
        }
        // The last frames' CPU time, a bar each (16 ms at the top), colored by its budget.
        let spark =
            Rect::new(left, panel.max_y() - padding - spark_height, right - left, spark_height);
        let bar = (spark.size.width / WINDOW as f32).max(1.0);
        for (i, frame) in self.spark.iter().enumerate() {
            let h = ((*frame / 16.0).min(1.0) * spark.size.height).max(ctx.px(1.0));
            let x = spark.max_x() - (self.spark.len() - i) as f32 * bar;
            let color = tone_color(tone(f64::from(*frame), 8.0, 16.0), palette).with_alpha(0.7);
            layer.quad(Quad::filled(
                Rect::new(x.round(), (spark.max_y() - h).round(), (bar - 1.0).max(1.0), h.round()),
                color,
            ));
        }
        // The ×, top right: hides it (F12 again).
        let close = Rect::new(
            panel.max_x() - padding / 2.0 - close_size,
            panel.min_y() + padding / 2.0,
            close_size,
            close_size,
        );
        let hovered = self.close_hovered;
        let r = ds::clear_button(
            layer,
            icons,
            &ctx,
            close.min_x() + close_size / 2.0,
            close.min_y() + close_size / 2.0,
            hovered,
        );
        self.close = Some(r);
        self.tooltips.paint(text, scene, palette, scale, area);
    }

    fn timings(&self) -> (Duration, Duration) {
        let samples = &self.frame_times[..self.recorded];
        if samples.is_empty() {
            return (Duration::ZERO, Duration::ZERO);
        }
        let total: Duration = samples.iter().sum();
        (total / samples.len() as u32, samples.iter().copied().max().unwrap_or_default())
    }
}

/// A tone's color: the palette's success (green), warning and error shades.
fn tone_color(tone: Tone, palette: &Palette) -> Rgba {
    match tone {
        Tone::Plain => palette.text,
        Tone::Good => {
            ui_components::tag_colors::named("green", palette).unwrap_or(palette.vcs_added)
        }
        Tone::Warn => Level::Warning.color(palette),
        Tone::Bad => Level::Critical.color(palette),
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

#[cfg(test)]
#[path = "../tests/unit/debug_overlay.rs"]
mod tests;

/// Pixels drawn as a share of the surface (0..=1).
fn drawn_share(pixels: u64, viewport: Size) -> f32 {
    let area = (viewport.width * viewport.height).max(1.0);
    (pixels as f32 / area).min(1.0)
}
