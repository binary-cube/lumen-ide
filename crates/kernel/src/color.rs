//! RGBA color, shared by themes, UI and rendering.

use std::fmt;

/// Color in sRGB space, components in `[0, 1]`, non-premultiplied alpha.
#[derive(Clone, Copy, Default, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba::new(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Rgba = Rgba::rgb(0x000000);
    pub const WHITE: Rgba = Rgba::rgb(0xffffff);

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// From `0xRRGGBB`, opaque.
    pub const fn rgb(hex: u32) -> Self {
        Self::new(
            ((hex >> 16) & 0xff) as f32 / 255.0,
            ((hex >> 8) & 0xff) as f32 / 255.0,
            (hex & 0xff) as f32 / 255.0,
            1.0,
        )
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Its relative brightness (0 black … 1 white, the eye's weights on its channels): a
    /// dark theme told from a light one, a text's color chosen to read on it.
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// `self` moved toward `other` by `t` (0: self, 1: other); alpha as well.
    pub fn mix(self, other: Self, t: f32) -> Self {
        let m = |a: f32, b: f32| a + (b - a) * t;
        Self::new(m(self.r, other.r), m(self.g, other.g), m(self.b, other.b), m(self.a, other.a))
    }

    /// `#rrggbb` (the alpha left out).
    pub fn to_hex(self) -> String {
        let [r, g, b, _] = self.to_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// From HSL as in CSS: hue in degrees, saturation and lightness 0..1.
    pub fn from_hsl(h: f32, s: f32, l: f32, a: f32) -> Self {
        let h = h.rem_euclid(360.0) / 360.0;
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let channel = |t: f32| {
            let t = t.rem_euclid(1.0);
            if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 0.5 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        Self::new(channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0), a)
    }

    /// (hue in degrees, saturation, lightness), as in CSS.
    pub fn to_hsl(self) -> (f32, f32, f32) {
        let c = self;
        let max = c.r.max(c.g).max(c.b);
        let min = c.r.min(c.g).min(c.b);
        let l = (max + min) / 2.0;
        if max == min {
            return (0.0, 0.0, l);
        }
        let d = max - min;
        let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        let h = if max == c.r {
            (c.g - c.b) / d + if c.g < c.b { 6.0 } else { 0.0 }
        } else if max == c.g {
            (c.b - c.r) / d + 2.0
        } else {
            (c.r - c.g) / d + 4.0
        };
        (h * 60.0, s, l)
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#')?;
        let nibble = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        let (r, g, b, a) = match hex.len() {
            3 => (nibble(0)? * 17, nibble(1)? * 17, nibble(2)? * 17, 255),
            6 => (byte(0)?, byte(2)?, byte(4)?, 255),
            8 => (byte(0)?, byte(2)?, byte(4)?, byte(6)?),
            _ => return None,
        };
        Some(Self::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0))
    }

    pub fn to_rgba8(self) -> [u8; 4] {
        let q = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b), q(self.a)]
    }
}

impl fmt::Debug for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b, a] = self.to_rgba8();
        write!(f, "#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

#[cfg(test)]
#[path = "../tests/unit/color.rs"]
mod tests;
