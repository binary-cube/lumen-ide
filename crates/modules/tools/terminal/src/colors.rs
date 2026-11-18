//! The terminal's colors as chosen (Settings → Terminal): the theme's (its ANSI palette),
//! the classic xterm ones on the theme's background, or the theme's with more contrast —
//! each color pushed toward the text until it reads well on the background (WCAG's 4.5:1).
//! Worked out once per theme and choice, not per frame.

use kernel::Rgba;
use ui_components::Palette;

use crate::config::Colors;

/// xterm's 16 colors (black … white, then the bright ones).
const CLASSIC: [u32; 16] = [
    0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5, 0x7f7f7f,
    0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
];

/// The contrast a color must reach on the background (WCAG AA, normal text).
const CONTRAST: f32 = 4.5;

/// The colors a terminal draws with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TermColors {
    pub foreground: Rgba,
    pub background: Rgba,
    pub cursor: Rgba,
    pub ansi: [Rgba; 16],
}

impl TermColors {
    pub fn new(palette: &Palette, colors: Colors) -> Self {
        let theme = Self {
            foreground: palette.terminal_foreground,
            background: palette.terminal_background,
            cursor: palette.terminal_cursor,
            ansi: palette.terminal_ansi,
        };
        match colors {
            Colors::Theme => theme,
            Colors::Classic => Self { ansi: CLASSIC.map(Rgba::rgb), ..theme },
            Colors::HighContrast => {
                let dark = luminance(theme.background) < 0.18;
                let text = if dark { Rgba::WHITE } else { Rgba::rgb(0x000000) };
                let readable = |c: Rgba| toward(c, text, theme.background);
                Self {
                    foreground: text,
                    cursor: readable(theme.cursor),
                    ansi: theme.ansi.map(readable),
                    ..theme
                }
            }
        }
    }
}

impl TermColors {
    /// As programs are told them when they ask (OSC 10 / 11 / 12 / 4).
    pub fn palette(&self) -> terminal_session::TermPalette {
        let rgb = |c: Rgba| {
            let [r, g, b, _] = c.to_rgba8();
            (r, g, b)
        };
        terminal_session::TermPalette {
            foreground: rgb(self.foreground),
            background: rgb(self.background),
            cursor: rgb(self.cursor),
            ansi: self.ansi.map(rgb),
        }
    }
}

/// `color` mixed toward `text` just enough to reach [`CONTRAST`] on `background`.
fn toward(color: Rgba, text: Rgba, background: Rgba) -> Rgba {
    let mut mixed = color;
    for step in 1..=10 {
        if contrast(mixed, background) >= CONTRAST {
            break;
        }
        mixed = mix(color, text, step as f32 / 10.0);
    }
    mixed
}

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a)
}

/// Relative luminance (sRGB, WCAG).
pub fn luminance(c: Rgba) -> f32 {
    let linear = |v: f32| {
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// The contrast ratio of two colors (1 … 21).
pub fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
#[path = "../tests/unit/colors.rs"]
mod tests;
