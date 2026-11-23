//! How a chart looks, all of it from the theme: its series' colors (the named colors in the
//! theme's shade, the same order everywhere), its grid, axes, labels and legend, and which
//! of its parts show. The defaults are a full chart; a card's sparkline or a widget turns
//! parts off.

use kernel::Rgba;
use text_layout::Font;

use crate::ds::Ctx;

/// The series' colors, in the order series take them ([`crate::tag_colors::NAMES`]).
const SERIES: [&str; 8] = ["blue", "green", "orange", "purple", "teal", "red", "yellow", "gray"];

/// The most colors a look holds (a caller's first, the named ones after): kept in place,
/// so a look made each frame allocates nothing.
pub const MAX_COLORS: usize = 12;

/// The grid's lines over the text color.
const GRID_ALPHA: f32 = 0.07;
/// The zero line, stronger than the grid.
const ZERO_ALPHA: f32 = 0.22;
/// An area's fill under its line (stacked ones: fuller, they're read as bands).
pub(crate) const AREA_ALPHA: (f32, f32) = (0.18, 0.55);

/// How a chart appears the first time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Enter {
    /// At once.
    None,
    /// Everything fades in.
    Fade,
    /// Bars grow from their base, points from nothing, lines and slices fade in.
    Grow,
}

/// How a chart goes to new data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Update {
    /// At once.
    None,
    /// The old picture fades into the new.
    Fade,
    /// Bars and points move to their new places; lines and areas fade from old to new.
    Morph,
}

/// A chart's motion: how it appears, how it changes, whether hover lights up smoothly, and
/// how long a transition takes. The app's "reduce motion" turns it all off. Frames are
/// asked only while something moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChartMotion {
    pub enter: Enter,
    pub update: Update,
    /// The lit element fades in under the mouse (else: lit at once).
    pub hover: bool,
    /// A transition's length (milliseconds).
    pub millis: u32,
}

impl Default for ChartMotion {
    fn default() -> Self {
        Self { enter: Enter::Grow, update: Update::Morph, hover: true, millis: 280 }
    }
}

impl ChartMotion {
    /// No motion at all (a chart redrawn every second by a live source may want it).
    pub fn none() -> Self {
        Self { enter: Enter::None, update: Update::None, hover: false, millis: 0 }
    }

    /// As asked, unless the app's "reduce motion" is on.
    pub(crate) fn effective(self) -> Self {
        if crate::decorations::reduce_motion() { Self::none() } else { self }
    }
}

/// Which parts of a chart show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChartParts {
    pub x_axis: bool,
    pub y_axis: bool,
    pub grid: bool,
    /// The legend, when there's more than one series (a pie's always, unless off).
    pub legend: bool,
    /// The tooltip and the highlight under the mouse.
    pub hover: bool,
    /// The wheel zooms a measured X, a drag moves along it, a double click shows it all.
    pub zoom: bool,
}

impl Default for ChartParts {
    fn default() -> Self {
        Self { x_axis: true, y_axis: true, grid: true, legend: true, hover: true, zoom: true }
    }
}

impl ChartParts {
    /// Only the marks (a card's sparkline, a widget): no axes, grid, legend or zoom.
    pub fn bare() -> Self {
        Self { x_axis: false, y_axis: false, grid: false, legend: false, hover: true, zoom: false }
    }
}

/// A chart's look: its colors and fonts from the theme, its parts.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartLook {
    pub parts: ChartParts,
    /// How it moves (see [`ChartMotion`]).
    pub motion: ChartMotion,
    /// Series colors given by the caller (a metric's own), then the named ones, in order
    /// (the first [`Self::color_count`] of them).
    pub colors: [Rgba; MAX_COLORS],
    pub color_count: usize,
    pub text: Rgba,
    pub muted: Rgba,
    pub grid: Rgba,
    pub zero: Rgba,
    /// What's behind the chart (a pie's slices are parted by it).
    pub background: Rgba,
    pub label_font: Font,
    /// Line width, logical pixels.
    pub line_width: f32,
    /// Corners of a bar's top (logical pixels).
    pub bar_radius: f32,
}

impl ChartLook {
    /// A full chart in `ctx`'s theme, on `background`.
    pub fn new(ctx: &Ctx<'_>, background: Rgba) -> Self {
        let p = ctx.palette;
        let mut colors = [p.accent; MAX_COLORS];
        let mut color_count = 0;
        for c in SERIES.iter().filter_map(|name| crate::tag_colors::named(name, p)) {
            colors[color_count] = c;
            color_count += 1;
        }
        Self {
            parts: ChartParts::default(),
            motion: ChartMotion::default(),
            colors,
            color_count,
            text: p.text,
            muted: p.text_muted,
            grid: p.text.with_alpha(GRID_ALPHA),
            zero: p.text.with_alpha(ZERO_ALPHA),
            background,
            label_font: ctx.ui_font(-1.5),
            line_width: 1.75,
            bar_radius: 2.0,
        }
    }

    pub fn parts(mut self, parts: ChartParts) -> Self {
        self.parts = parts;
        self
    }

    pub fn motion(mut self, motion: ChartMotion) -> Self {
        self.motion = motion;
        self
    }

    /// The series' colors in order (a metric's own), the named ones after them.
    pub fn colors(mut self, colors: &[Rgba]) -> Self {
        let (named, named_count) = (self.colors, self.color_count);
        let mut n = 0;
        for &c in colors.iter().chain(&named[..named_count]).take(MAX_COLORS) {
            self.colors[n] = c;
            n += 1;
        }
        self.color_count = n;
        self
    }

    /// The colors in use.
    pub fn palette(&self) -> &[Rgba] {
        &self.colors[..self.color_count]
    }

    /// Series `i`'s color (past the list: again, fainter, so the 13th isn't the 1st).
    pub fn series(&self, i: usize) -> Rgba {
        let n = self.color_count.max(1);
        let c = self.colors.get(i % n).copied().unwrap_or(self.text);
        if i >= n { c.with_alpha(c.a * 0.6) } else { c }
    }

    /// An area's fill for series `i`.
    pub fn area(&self, i: usize, stacked: bool) -> Rgba {
        let c = self.series(i);
        c.with_alpha(c.a * if stacked { AREA_ALPHA.1 } else { AREA_ALPHA.0 })
    }

    /// A fingerprint of the colors masks are rasterized in (they're made again when it
    /// changes: another theme).
    pub(crate) fn stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for c in self.palette().iter().chain([&self.background]) {
            [c.r, c.g, c.b, c.a].map(f32::to_bits).hash(&mut h);
        }
        self.line_width.to_bits().hash(&mut h);
        h.finish()
    }
}
