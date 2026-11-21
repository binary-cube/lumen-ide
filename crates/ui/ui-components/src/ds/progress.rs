//! How far something is, as a bar: a thin rounded track, filled as far as it went — or,
//! not known yet, a short piece sweeping across. Optionally a row of text over it: what's
//! going on (left, muted) and how far (right: the percent, or a text of the caller's, as
//! "12 MB of 33 MB").
//!
//! One component for every progress the app shows (a download, a transfer, a notification,
//! a modal's work), configured by a builder:
//!
//! ```ignore
//! ProgressBar::of(0.4).label("Downloading").percent().paint(layer, text, ctx, rect);
//! ProgressBar::unknown().tone(Tone::Neutral).paint_bar(layer, ctx, track);
//! ProgressBar::of(0.3).color(ink).track_alpha(0.07).min_fill(2.0).snapped().paint_bar(…);
//! ProgressBar::split(&parts).snapped().paint_bar(layer, ctx, track);
//! ProgressBar::unknown().on_dark().sweep_at(t).glide(0.22).paint_bar(layer, ctx, track);
//! ```
//!
//! Its defaults are the app's usual bar; every option only changes what a place asks for.
//! [`Meter`] is the small vertical sibling: a row of bars standing on a line (the status
//! bar's).

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::TextSystem;
use ui_kit::draw_text;

use super::{Ctx, Tone, optical_baseline};

/// What the text row shows on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProgressValue<'a> {
    #[default]
    None,
    /// `40%` (nothing while it isn't known).
    Percent,
    /// The caller's own (`12 MB of 33 MB`, `3 of 8 files`).
    Text(&'a str),
}

/// What runs under the fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProgressTrack {
    /// The text's color at this alpha (the usual: [`TRACK_ALPHA`]).
    Faint(f32),
    /// A color of the caller's (a bar over a picture or a dark card).
    Color(Rgba),
    /// No track: the fill alone.
    None,
}

/// How the piece of a bar not known yet moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepMotion {
    /// At an even pace (the usual).
    Linear,
    /// Easing in and out of each pass, its piece on whole pixels (the splash's).
    Glide,
}

/// The sweep of a bar not known yet: its piece's share of the track, how it moves, and
/// where it is (0..1 of a pass; `None`: by the clock, one pass every [`SWEEP_MS`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sweep {
    pub share: f32,
    pub motion: SweepMotion,
    pub at: Option<f32>,
}

impl Default for Sweep {
    fn default() -> Self {
        Self { share: SWEEP_SHARE, motion: SweepMotion::Linear, at: None }
    }
}

impl Sweep {
    /// Its piece in `track` at `t` (0..1 of a pass): where it starts and how wide.
    fn piece_at(&self, track: Rect, t: f32) -> (f32, f32) {
        let (w, t) = match self.motion {
            SweepMotion::Linear => (track.size.width * self.share, t),
            SweepMotion::Glide => {
                ((track.size.width * self.share).round(), t * t * (3.0 - 2.0 * t))
            }
        };
        let x = track.min_x() - w + (track.size.width + w) * t;
        let x0 = x.max(track.min_x());
        (x0, ((x + w).min(track.max_x()) - x0).max(0.0))
    }

    /// Its piece in `track` now (or where it was set).
    fn piece(&self, track: Rect) -> (f32, f32) {
        let t = self.at.unwrap_or_else(|| (now_ms() % SWEEP_MS) as f32 / SWEEP_MS as f32);
        self.piece_at(track, t.clamp(0.0, 1.0))
    }
}

/// One part of a bar split in parts (what's used by one thing, then another): its share of
/// the whole track (0..=1) and its color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressSegment {
    pub fraction: f32,
    pub color: Rgba,
}

impl ProgressSegment {
    pub const fn new(fraction: f32, color: Rgba) -> Self {
        Self { fraction, color }
    }
}

/// A progress bar, as configured (see the module).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressBar<'a> {
    /// How far (0..=1); `None`: not known — the piece sweeps (frames are needed meanwhile:
    /// [`Self::animating`]).
    pub fraction: Option<f32>,
    /// The fill's color (the track is the text's, faint).
    pub tone: Tone,
    /// The bar's thickness (logical pixels).
    pub thickness: f32,
    /// What's going on, on the text row's left.
    pub label: Option<&'a str>,
    /// How far, on its right.
    pub value: ProgressValue<'a>,
    /// The fill's own color, over the tone's.
    pub fill: Option<Rgba>,
    /// The fill's opacity (1: as its color is).
    pub fill_alpha: f32,
    /// What runs under the fill.
    pub track: ProgressTrack,
    /// The narrowest a fill shows once it isn't empty (logical pixels); `None`: the bar's
    /// height (a round dot at the start).
    pub min_fill: Option<f32>,
    /// The corners' radius (logical pixels); `None`: half the height (a pill).
    pub radius: Option<f32>,
    /// The fills on whole pixels.
    pub snapped: bool,
    /// The bar split in parts, one after another (the fraction then unused).
    pub segments: &'a [ProgressSegment],
    /// How the piece of a bar not known yet moves.
    pub sweep: Sweep,
}

impl Default for ProgressBar<'_> {
    fn default() -> Self {
        Self {
            fraction: None,
            tone: Tone::Accent,
            thickness: 4.0,
            label: None,
            value: ProgressValue::None,
            fill: None,
            fill_alpha: 1.0,
            track: ProgressTrack::Faint(TRACK_ALPHA),
            min_fill: None,
            radius: None,
            snapped: false,
            segments: &[],
            sweep: Sweep::default(),
        }
    }
}

/// The usual track: the text's color at this alpha.
pub const TRACK_ALPHA: f32 = 0.08;
/// The track on a dark surface (a picture, the splash's card): white at this alpha.
pub const DARK_TRACK_ALPHA: f32 = 0.08;

/// The text row over the bar and the space between them (logical pixels).
const ROW: f32 = 18.0;
const ROW_GAP: f32 = 6.0;
/// The sweep: its piece's share of the track and one pass (ms).
pub const SWEEP_SHARE: f32 = 0.3;
pub const SWEEP_MS: u128 = 1400;

impl<'a> ProgressBar<'a> {
    /// `fraction` done (clamped to 0..=1).
    pub fn of(fraction: f32) -> Self {
        Self { fraction: Some(fraction.clamp(0.0, 1.0)), ..Self::default() }
    }

    /// How far isn't known (yet).
    pub fn unknown() -> Self {
        Self::default()
    }

    /// `fraction` when known, else the sweep.
    pub fn from(fraction: Option<f32>) -> Self {
        fraction.map_or_else(Self::unknown, Self::of)
    }

    /// The bar split in `segments`, one after another from the start.
    pub fn split(segments: &'a [ProgressSegment]) -> Self {
        Self { fraction: Some(0.0), segments, ..Self::default() }
    }

    /// `done` of `total` (a total of 0: not known).
    pub fn counted(done: u64, total: u64) -> Self {
        Self::from((total > 0).then(|| done as f32 / total as f32))
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    /// The bar's thickness (logical pixels).
    pub fn thickness(mut self, thickness: f32) -> Self {
        self.thickness = thickness;
        self
    }

    /// The fill in `color` (over the tone's).
    pub fn color(mut self, color: Rgba) -> Self {
        self.fill = Some(color);
        self
    }

    /// The fill's opacity (0..=1).
    pub fn fill_alpha(mut self, alpha: f32) -> Self {
        self.fill_alpha = alpha.clamp(0.0, 1.0);
        self
    }

    /// What runs under the fill.
    pub fn track(mut self, track: ProgressTrack) -> Self {
        self.track = track;
        self
    }

    /// The usual track, at `alpha`.
    pub fn track_alpha(self, alpha: f32) -> Self {
        self.track(ProgressTrack::Faint(alpha))
    }

    /// No track: the fill alone.
    pub fn fill_only(self) -> Self {
        self.track(ProgressTrack::None)
    }

    /// Drawn on a dark surface (a picture, a dark card): a faint white track.
    pub fn on_dark(self) -> Self {
        self.track(ProgressTrack::Color(Rgba::WHITE.with_alpha(DARK_TRACK_ALPHA)))
    }

    /// The narrowest a fill shows once it isn't empty (logical pixels).
    pub fn min_fill(mut self, width: f32) -> Self {
        self.min_fill = Some(width.max(0.0));
        self
    }

    /// The corners' radius (logical pixels).
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius.max(0.0));
        self
    }

    /// The fills on whole pixels.
    pub fn snapped(mut self) -> Self {
        self.snapped = true;
        self
    }

    /// The piece of a bar not known yet at `t` of its pass (0..1: the caller's clock).
    pub fn sweep_at(mut self, t: f32) -> Self {
        self.sweep.at = Some(t);
        self
    }

    /// The piece glides (easing in and out of each pass), `share` of the track wide.
    pub fn glide(mut self, share: f32) -> Self {
        self.sweep.share = share.clamp(0.0, 1.0);
        self.sweep.motion = SweepMotion::Glide;
        self
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// The percent on the right.
    pub fn percent(mut self) -> Self {
        self.value = ProgressValue::Percent;
        self
    }

    /// `value` on the right.
    pub fn value(mut self, value: &'a str) -> Self {
        self.value = ProgressValue::Text(value);
        self
    }

    /// It sweeps by the clock (not known, no position given): frames are needed while it
    /// shows.
    pub fn animating(&self) -> bool {
        self.fraction.is_none() && self.sweep.at.is_none()
    }

    /// The text row is drawn (a label or a value).
    fn has_row(&self) -> bool {
        self.label.is_some() || self.value != ProgressValue::None
    }

    /// The height it takes (physical pixels): the bar, and the text row over it if any.
    pub fn height(&self, ctx: &Ctx<'_>) -> f32 {
        let row = if self.has_row() { ctx.px(ROW + ROW_GAP) } else { 0.0 };
        row + ctx.px(self.thickness)
    }

    /// The right side's text as it shows now.
    pub fn value_text(&self) -> Option<String> {
        match self.value {
            ProgressValue::None => None,
            ProgressValue::Percent => self.fraction.map(|f| format!("{}%", (f * 100.0).round())),
            ProgressValue::Text(text) => Some(text.to_owned()),
        }
    }

    /// Drawn in `rect`: the text row at its top (when there's one), the bar at its bottom,
    /// as wide as `rect`.
    pub fn paint(&self, layer: &mut Layer, text: &mut TextSystem, ctx: &Ctx<'_>, rect: Rect) {
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        if self.has_row() {
            let font = ctx.ui_font(-1.0);
            let row = Rect::new(rect.min_x(), rect.min_y(), rect.size.width, px(ROW));
            let base = optical_baseline(row.min_y(), row.size.height, &font);
            let value = self.value_text().map(|v| text.shape_line(&v, &font));
            let value_w = value.as_ref().map_or(0.0, |v| v.width + px(8.0));
            if let Some(label) = self.label {
                let room = (rect.size.width - value_w).max(0.0);
                let shown = super::ellipsize(text, label, &font, room);
                let shaped = text.shape_line(&shown, &font);
                draw_text(layer, &shaped, row.min_x(), base, ctx.fade(p.text_muted));
            }
            if let Some(value) = value {
                let x = (row.max_x() - value.width).round();
                draw_text(layer, &value, x, base, ctx.fade(p.text));
            }
        }
        let h = px(self.thickness);
        self.paint_bar(layer, ctx, Rect::new(rect.min_x(), rect.max_y() - h, rect.size.width, h));
    }

    /// Only the bar, in `track` (its height the bar's: [`Self::thickness`] not used).
    pub fn paint_bar(&self, layer: &mut Layer, ctx: &Ctx<'_>, track: Rect) {
        let p = ctx.palette;
        let h = track.size.height;
        let r = self.radius.map_or(h / 2.0, |v| ctx.px(v));
        let under = match self.track {
            ProgressTrack::Faint(alpha) => Some(p.text.with_alpha(alpha)),
            ProgressTrack::Color(color) => Some(color),
            ProgressTrack::None => None,
        };
        if let Some(under) = under {
            layer.quad(Quad::filled(track, ctx.fade(under)).rounded(r));
        }
        if !self.segments.is_empty() {
            self.paint_segments(layer, ctx, track, r);
            return;
        }
        let (x, w) = match self.fraction {
            Some(f) => (track.min_x(), track.size.width * f),
            None => self.sweep.piece(track),
        };
        if w > 0.0 {
            let w = w.max(self.min_fill.map_or(h, |v| ctx.px(v)));
            let w = if self.snapped { w.round() } else { w };
            let fill = Rect::new(x, track.min_y(), w, h);
            layer.quad(Quad::filled(fill, ctx.fade(self.ink(self.tone.color(p)))).rounded(r));
        }
    }

    /// The fill's color from `base`, at its opacity.
    fn ink(&self, base: Rgba) -> Rgba {
        let color = self.fill.unwrap_or(base);
        color.with_alpha(color.a * self.fill_alpha)
    }

    /// The parts one after another from the track's start, never past its end.
    fn paint_segments(&self, layer: &mut Layer, ctx: &Ctx<'_>, track: Rect, r: f32) {
        let width = track.size.width;
        let mut used = 0.0;
        for part in self.segments {
            let w = (width * part.fraction.clamp(0.0, 1.0)).min(width - used);
            let w = if self.snapped { w.round() } else { w };
            if w > 0.0 {
                let at = Rect::new(track.min_x() + used, track.min_y(), w, track.size.height);
                layer.quad(Quad::filled(at, ctx.fade(self.ink(part.color))).rounded(r));
                used += w;
            }
        }
    }
}

/// A meter: a row of small vertical bars standing on a line, each filled from the bottom
/// as high as its level, over its own color faint (the status bar's). Sizes in logical
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meter {
    pub bar_width: f32,
    pub gap: f32,
    pub height: f32,
    pub radius: f32,
    /// The least a bar shows, even empty (it still reads as a bar).
    pub min_fill: f32,
    /// Its track: its color at this alpha.
    pub track_alpha: f32,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            bar_width: 5.0,
            gap: 3.0,
            height: 13.0,
            radius: 1.5,
            min_fill: 2.0,
            track_alpha: 0.16,
        }
    }
}

impl Meter {
    /// The width of `bars` bars (physical pixels).
    pub fn width(&self, ctx: &Ctx<'_>, bars: usize) -> f32 {
        let n = bars as f32;
        n * ctx.px(self.bar_width) + (n - 1.0).max(0.0) * ctx.px(self.gap)
    }

    /// Its height (physical pixels).
    pub fn bar_height(&self, ctx: &Ctx<'_>) -> f32 {
        ctx.px(self.height)
    }

    /// `bars` (a level 0..=1 and a color each) from `x`, standing on `bottom`.
    pub fn paint(
        &self,
        layer: &mut Layer,
        ctx: &Ctx<'_>,
        (x, bottom): (f32, f32),
        bars: impl IntoIterator<Item = (f32, Rgba)>,
    ) {
        let (w, gap, h) = (ctx.px(self.bar_width), ctx.px(self.gap), ctx.px(self.height));
        let r = ctx.px(self.radius);
        for (i, (level, color)) in bars.into_iter().enumerate() {
            let left = x + i as f32 * (w + gap);
            let track = Rect::new(left, bottom - h, w, h);
            layer
                .quad(Quad::filled(track, ctx.fade(color.with_alpha(self.track_alpha))).rounded(r));
            let filled = (level.clamp(0.0, 1.0) * h).max(ctx.px(self.min_fill)).round();
            let bar = Rect::new(left, bottom - filled, w, filled);
            layer.quad(Quad::filled(bar, ctx.fade(color)).rounded(r));
        }
    }
}

/// The sweeping piece at `ms` (since the epoch): where it starts and how wide, within
/// `track`.
#[cfg(test)]
fn sweep(track: Rect, ms: u128) -> (f32, f32) {
    Sweep::default().piece_at(track, (ms % SWEEP_MS) as f32 / SWEEP_MS as f32)
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

#[cfg(test)]
#[path = "../../tests/unit/ds/progress.rs"]
mod tests;
