//! How bad something is, shown the same everywhere ([`Level`]): always an icon and a color,
//! never a color alone — Info a circle with an `i` in blue, Warning a triangle in orange,
//! Critical an octagon with a `!` in vermilion, from the Okabe–Ito palette (told apart with
//! deuteranopia and protanopia too). Each theme gets its shades ([`Palette::severity`]: the
//! lighter ones on a dark background, the deeper ones on a light one, at least 3:1 against
//! the editor's background), and a heat ramp for shares of time ([`Palette::heat`]: one hue,
//! its intensity growing with the share, so the order reads in grey too).

use kernel::Rgba;

use crate::palette::Palette;

/// How bad a finding (a problem, a remark) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warning,
    Critical,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Info, Level::Warning, Level::Critical];

    /// Its icon (a name of the app's set): a circle, a triangle, an octagon.
    pub fn icon(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warning => "alert",
            Level::Critical => "octagon-alert",
        }
    }

    /// Its color in `palette`.
    pub fn color(self, palette: &Palette) -> Rgba {
        palette.severity(self)
    }
}

/// The relative luminance of `c` (WCAG 2).
pub fn luminance(c: Rgba) -> f32 {
    let channel = |v: f32| {
        if v <= 0.039_28 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// The contrast ratio between `a` and `b` (1 to 21; WCAG 2).
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// `color` moved toward `toward` just enough to read at `ratio` on `background` (as it is
/// when it already does): a theme's muted text kept readable on a card.
pub fn readable(color: Rgba, toward: Rgba, background: Rgba, ratio: f32) -> Rgba {
    (0..=20)
        .map(|i| color.mix(toward, i as f32 * 0.05))
        .find(|c| contrast(*c, background) >= ratio)
        .unwrap_or(toward)
}

impl Palette {
    /// The theme has a dark background.
    pub fn is_dark(&self) -> bool {
        luminance(self.editor_background) < 0.2
    }

    /// `level`'s color in this theme (Okabe–Ito: sky blue / blue, orange, vermilion).
    pub fn severity(&self, level: Level) -> Rgba {
        let dark = self.is_dark();
        match (level, dark) {
            (Level::Info, true) => Rgba::rgb(0x56b4e9),
            (Level::Info, false) => Rgba::rgb(0x0072b2),
            (Level::Warning, true) => Rgba::rgb(0xe69f00),
            (Level::Warning, false) => Rgba::rgb(0xa86b00),
            (Level::Critical, true) => Rgba::rgb(0xf0703a),
            (Level::Critical, false) => Rgba::rgb(0xc44e00),
        }
    }

    /// The heat of a share `t` (0..1): one warm hue, from faint to intense as `t` grows — on
    /// a dark theme brighter and more saturated, on a light one deeper.
    pub fn heat(&self, t: f32) -> Rgba {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        // Small shares stay visible: the ramp starts at a fifth.
        let k = 0.2 + 0.8 * t.sqrt();
        if self.is_dark() {
            Rgba::from_hsl(22.0, 0.35 + 0.55 * k, 0.30 + 0.30 * k, 1.0)
        } else {
            Rgba::from_hsl(22.0, 0.45 + 0.5 * k, 0.80 - 0.38 * k, 1.0)
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/severity.rs"]
mod tests;
