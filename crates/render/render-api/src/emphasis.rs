//! The emphasis: what a scene's layers drew up to a point, muted — desaturated and dimmed —
//! everywhere but in a few areas kept as they are ([`Keep`]: rounded rectangles with soft
//! edges). A scene has at most one ([`Scene::emphasize`](crate::Scene::emphasize)): the
//! layers pushed before it are muted, those pushed after (overlays) are not.
//!
//! The math is here, once: [`Emphasis::mask`] (how much of a pixel is kept) and
//! [`Emphasis::apply`] (its color then). The software renderer runs it on the pixels drawn;
//! the wgpu one mirrors it in WGSL (`common.wgsl`). The operator is linear in the color
//! (premultiplied, sRGB values as the renderers blend them), so it can be applied to each
//! primitive as it's drawn instead of to the frame once drawn: the result is the same.

use kernel::Rect;

/// The most areas an emphasis keeps.
pub const MAX_KEEPS: usize = 8;

/// The luma weights (Rec.709) the gray is made with.
pub const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// An area kept as drawn: `rect` with its corners rounded by `radius`, and outside it, for
/// [`Emphasis::feather`] pixels, a smooth way back to muted. `weight` (0..1): how much it's
/// kept (several add up, at most 1).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Keep {
    pub rect: Rect,
    pub radius: f32,
    pub weight: f32,
}

impl Keep {
    /// `rect` kept fully, its corners rounded by `radius`.
    pub fn new(rect: Rect, radius: f32) -> Self {
        Self { rect, radius, weight: 1.0 }
    }
}

/// What layers drew, muted outside its kept areas: `gray` (0..1: how far towards the
/// pixel's luma), `dim` (0..1: how much darker), `feather` (physical pixels: the kept areas'
/// soft edge). Physical pixels, as the scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Emphasis {
    pub gray: f32,
    pub dim: f32,
    pub feather: f32,
    /// The layers muted: those before this one (set by the scene).
    pub(crate) below: usize,
    keeps: [Keep; MAX_KEEPS],
    len: u8,
}

impl Emphasis {
    pub fn new(gray: f32, dim: f32, feather: f32) -> Self {
        Self {
            gray: gray.clamp(0.0, 1.0),
            dim: dim.clamp(0.0, 1.0),
            feather: feather.max(0.0),
            below: 0,
            keeps: [Keep::default(); MAX_KEEPS],
            len: 0,
        }
    }

    /// Keeps `keep` too; `false` (left out) past [`MAX_KEEPS`] or when it covers nothing.
    pub fn keep(&mut self, keep: Keep) -> bool {
        let empty = keep.rect.size.width <= 0.0 || keep.rect.size.height <= 0.0;
        if empty || keep.weight <= 0.0 || usize::from(self.len) == MAX_KEEPS {
            return false;
        }
        self.keeps[usize::from(self.len)] = keep;
        self.len += 1;
        true
    }

    pub fn keeps(&self) -> &[Keep] {
        &self.keeps[..usize::from(self.len)]
    }

    /// The layers it mutes: `0..below`.
    pub fn below(&self) -> usize {
        self.below
    }

    /// It changes something (some gray or dim).
    pub fn is_visible(&self) -> bool {
        self.gray > 0.0 || self.dim > 0.0
    }

    /// The area anything is kept in (the kept areas and their soft edges); outside it,
    /// every pixel is muted fully (`None`: everywhere).
    pub fn reach(&self) -> Option<Rect> {
        let reach = self.feather.max(0.5);
        self.keeps()
            .iter()
            .map(|k| {
                let r = k.rect;
                Rect::new(
                    r.min_x() - reach,
                    r.min_y() - reach,
                    r.size.width + 2.0 * reach,
                    r.size.height + 2.0 * reach,
                )
            })
            .reduce(|a, b| {
                let (x0, y0) = (a.min_x().min(b.min_x()), a.min_y().min(b.min_y()));
                let (x1, y1) = (a.max_x().max(b.max_x()), a.max_y().max(b.max_y()));
                Rect::new(x0, y0, x1 - x0, y1 - y0)
            })
    }

    /// How much of the pixel centered at `(x, y)` is kept (1: as drawn, 0: fully muted).
    pub fn mask(&self, x: f32, y: f32) -> f32 {
        let mut kept = 0.0;
        for k in self.keeps() {
            let r = &k.rect;
            // Far outside (past the soft edge): nothing kept; well inside: all of it — the
            // distance worked out only near the edge (the same values, cheaper).
            let reach = self.feather.max(0.5);
            if x <= r.min_x() - reach
                || x >= r.max_x() + reach
                || y <= r.min_y() - reach
                || y >= r.max_y() + reach
            {
                continue;
            }
            let inset = k.radius.max(0.5);
            if x >= r.min_x() + inset
                && x <= r.max_x() - inset
                && y >= r.min_y() + inset
                && y <= r.max_y() - inset
            {
                kept += k.weight;
                if kept >= 1.0 {
                    return 1.0;
                }
                continue;
            }
            let d = rounded_rect_distance(&k.rect, k.radius, x, y);
            let inside = if self.feather > 0.0 {
                let t = (d / self.feather).clamp(0.0, 1.0);
                1.0 - t * t * (3.0 - 2.0 * t)
            } else {
                (0.5 - d).clamp(0.0, 1.0)
            };
            kept += k.weight * inside;
            if kept >= 1.0 {
                return 1.0;
            }
        }
        kept
    }

    /// The color (premultiplied RGB, 0..1) at the pixel centered at `(x, y)`, muted as much
    /// as it isn't kept. Alpha doesn't change.
    pub fn apply(&self, rgb: [f32; 3], x: f32, y: f32) -> [f32; 3] {
        let muted = 1.0 - self.mask(x, y);
        if muted <= 0.0 {
            return rgb;
        }
        mute(rgb, self.gray * muted, self.dim * muted)
    }
}

/// `rgb` taken `gray` of the way to its luma, then darkened by `dim`.
pub fn mute(rgb: [f32; 3], gray: f32, dim: f32) -> [f32; 3] {
    let luma = LUMA[0] * rgb[0] + LUMA[1] * rgb[1] + LUMA[2] * rgb[2];
    let scale = 1.0 - dim;
    rgb.map(|c| (c + gray * (luma - c)) * scale)
}

/// The signed distance from `(x, y)` to `rect` rounded by `radius` (negative inside); the
/// quads' formula (`quad_coverage`, `fs_quad`).
pub fn rounded_rect_distance(rect: &Rect, radius: f32, x: f32, y: f32) -> f32 {
    let (half_w, half_h) = (rect.size.width / 2.0, rect.size.height / 2.0);
    let radius = radius.min(half_w).min(half_h).max(0.0);
    let dx = (x - (rect.min_x() + half_w)).abs() - (half_w - radius);
    let dy = (y - (rect.min_y() + half_h)).abs() - (half_h - radius);
    let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
    outside + dx.max(dy).min(0.0) - radius
}

#[cfg(test)]
#[path = "../tests/unit/emphasis.rs"]
mod tests;
