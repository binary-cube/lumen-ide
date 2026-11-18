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
//! ```

use kernel::Rect;
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
}

impl Default for ProgressBar<'_> {
    fn default() -> Self {
        Self {
            fraction: None,
            tone: Tone::Accent,
            thickness: 4.0,
            label: None,
            value: ProgressValue::None,
        }
    }
}

/// The text row over the bar and the space between them (logical pixels).
const ROW: f32 = 18.0;
const ROW_GAP: f32 = 6.0;
/// The sweep: its piece's share of the track and one pass (ms).
const SWEEP_SHARE: f32 = 0.3;
const SWEEP_MS: u128 = 1400;

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

    /// It sweeps (not known): frames are needed while it shows.
    pub fn animating(&self) -> bool {
        self.fraction.is_none()
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
        let r = track.size.height / 2.0;
        layer.quad(Quad::filled(track, ctx.fade(p.text.with_alpha(0.08))).rounded(r));
        let (x, w) = match self.fraction {
            Some(f) => (track.min_x(), track.size.width * f),
            None => sweep(track, now_ms()),
        };
        if w > 0.0 {
            let fill = Rect::new(x, track.min_y(), w.max(track.size.height), track.size.height);
            layer.quad(Quad::filled(fill, ctx.fade(self.tone.color(p))).rounded(r));
        }
    }
}

/// The sweeping piece at `ms` (since the epoch): where it starts and how wide, within
/// `track`.
fn sweep(track: Rect, ms: u128) -> (f32, f32) {
    let t = (ms % SWEEP_MS) as f32 / SWEEP_MS as f32;
    let w = track.size.width * SWEEP_SHARE;
    let x = track.min_x() - w + (track.size.width + w) * t;
    let x0 = x.max(track.min_x());
    (x0, ((x + w).min(track.max_x()) - x0).max(0.0))
}

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

#[cfg(test)]
#[path = "../../tests/unit/ds/progress.rs"]
mod tests;
