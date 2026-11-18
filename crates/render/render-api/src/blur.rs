//! The backdrop blur (a Glass backdrop): what was drawn under a layer, blurred, shown in
//! the layer's [`BackdropBlur`] rectangles before its own primitives.
//!
//! Every renderer blurs the same way: the area under the blur (the union of the layer's
//! rectangles, its *bounds*) is averaged down by a [`BlurPlan::factor`] (2, 4 or 8: a box
//! of that many pixels on each side), blurred by a separable gaussian at that size, and
//! drawn back scaled up (bilinear). The result is kept while what's under it stays the
//! same ([`Damage::first_layer`](crate::Damage::first_layer) tells), so an overlay that
//! stays open blurs once.

use kernel::Rect;

use crate::{PixelRect, Scene};

/// The window under a layer, blurred by `radius` (physical pixels), shown in `rect` with
/// its corners rounded by `corner` (0: square), at `opacity` (0..1: over what's under it,
/// as it fades in).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackdropBlur {
    pub rect: Rect,
    pub radius: f32,
    pub opacity: f32,
    pub corner: f32,
}

/// How much of a pixel whose center is at `(x, y)` is inside `rect` with its corners
/// rounded by `corner` (0..1: the edge smoothed over a pixel) — every renderer masks a
/// rounded blur the same way.
pub fn rounded_coverage(rect: &Rect, corner: f32, x: f32, y: f32) -> f32 {
    let r = corner.min(rect.size.width / 2.0).min(rect.size.height / 2.0).max(0.0);
    let (cx, cy) = (rect.min_x() + rect.size.width / 2.0, rect.min_y() + rect.size.height / 2.0);
    let qx = (x - cx).abs() - rect.size.width / 2.0 + r;
    let qy = (y - cy).abs() - rect.size.height / 2.0 + r;
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let d = outside + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// The largest gaussian reach, in texels of the reduced image, on each side.
pub const MAX_TAPS: u32 = 12;

/// How a blur of a given radius is made.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurPlan {
    /// The image is reduced by this much on each side (2, 4 or 8).
    pub factor: u32,
    /// The gaussian's sigma, in texels of the reduced image.
    pub sigma: f32,
    /// Texels taken on each side of the one blurred.
    pub taps: u32,
}

impl BlurPlan {
    /// The plan for `radius` physical pixels (the gaussian reaching that far: its sigma a
    /// third of it).
    pub fn for_radius(radius: f32) -> Self {
        let sigma = radius.max(1.0) / 3.0;
        // Two to three texels of sigma at the reduced size: smooth once scaled back up.
        let factor = ((sigma / 2.5).ceil() as u32).next_power_of_two().clamp(2, 8);
        let sigma = (sigma / factor as f32).max(0.5);
        let taps = ((sigma * 3.0).ceil() as u32).clamp(1, MAX_TAPS);
        Self { factor, sigma, taps }
    }

    /// The gaussian's weights for offsets `0..=taps` (each side shares them; they sum to 1
    /// over `-taps..=taps`).
    pub fn weights(&self) -> Vec<f32> {
        let w: Vec<f32> = (0..=self.taps)
            .map(|k| (-(k as f32).powi(2) / (2.0 * self.sigma * self.sigma)).exp())
            .collect();
        let total = w[0] + 2.0 * w[1..].iter().sum::<f32>();
        w.into_iter().map(|v| v / total).collect()
    }

    /// The reduced image's size for `bounds`.
    pub fn reduced(&self, bounds: PixelRect) -> (u32, u32) {
        (bounds.width().div_ceil(self.factor).max(1), bounds.height().div_ceil(self.factor).max(1))
    }
}

/// A layer of a scene that blurs what's under it: its place, the pixels its blur covers
/// (within its clip and the surface), how much.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurLayer {
    pub layer: usize,
    pub bounds: PixelRect,
    pub radius: f32,
}

/// The layers of `scene` that blur (in order), on a surface of `size` pixels. A layer
/// whose blur covers nothing on the surface is left out.
pub fn blur_layers(scene: &Scene, surface: PixelRect, out: &mut Vec<BlurLayer>) {
    out.clear();
    for (i, layer) in scene.layers().iter().enumerate() {
        if layer.blurs.is_empty() {
            continue;
        }
        let clip = match layer.clip {
            Some(c) => PixelRect::covering(&c).intersect(&surface),
            None => Some(surface),
        };
        let Some(clip) = clip else { continue };
        let bounds = layer
            .blurs
            .iter()
            .filter_map(|b| PixelRect::covering(&b.rect).intersect(&clip))
            .reduce(|a, b| a.union(&b));
        // Too small to blur (a few pixels): nothing.
        let Some(bounds) = bounds.filter(|b| b.width() >= 4 && b.height() >= 4) else {
            continue;
        };
        let radius = layer.blurs.iter().map(|b| b.radius).fold(0.0, f32::max);
        if radius >= 1.0 {
            out.push(BlurLayer { layer: i, bounds, radius });
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/blur.rs"]
mod tests;
