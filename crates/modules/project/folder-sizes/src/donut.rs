//! The ring of the biggest entries: drawn on the CPU into an image (the scene has no curves),
//! its edges smoothed; made again only when what it shows changes (the shares, the slice
//! under the mouse, the colors, the size) — never per frame.

use kernel::Rgba;
use render_api::ImageData;

/// The hole's share of the radius.
pub const HOLE: f32 = 0.58;

/// What a ring is drawn from: its shares (in order, from the top, clockwise), their colors,
/// the slice lit (under the mouse), its diameter in pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Ring {
    pub shares: Vec<f32>,
    pub colors: Vec<Rgba>,
    pub lit: Option<usize>,
    pub size: u32,
}

impl Ring {
    /// The ring as an image (`size` × `size`, RGBA, not premultiplied).
    pub fn image(&self) -> ImageData {
        let n = self.size.max(1);
        let total: f32 = self.shares.iter().sum::<f32>().max(f32::EPSILON);
        // Where each slice ends (a fraction of the turn).
        let mut ends = Vec::with_capacity(self.shares.len());
        let mut sum = 0.0;
        for share in &self.shares {
            sum += share / total;
            ends.push(sum);
        }
        let half = n as f32 / 2.0;
        let mut pixels = vec![0u8; (n * n * 4) as usize];
        for y in 0..n {
            for x in 0..n {
                let (dx, dy) = (x as f32 + 0.5 - half, y as f32 + 0.5 - half);
                let r = (dx * dx + dy * dy).sqrt() / half;
                let Some(slice) = slice_at(dx, dy, &ends) else { continue };
                // A thin gap between slices (when there are several): its width in pixels.
                let gap = if ends.len() > 1 { edge_distance(dx, dy, &ends) } else { f32::MAX };
                // The lit slice a little wider (out and in).
                let grow = if self.lit == Some(slice) { 0.04 } else { 0.0 };
                let (outer, inner) = (0.96 + grow, HOLE - grow);
                // Smoothed edges: a pixel's share inside the ring.
                let px = 1.0 / half;
                let coverage = ((outer - r) / px + 0.5).clamp(0.0, 1.0)
                    * ((r - inner) / px + 0.5).clamp(0.0, 1.0)
                    * (gap - GAP / 2.0 + 0.5).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                let color = self.colors.get(slice).copied().unwrap_or(Rgba::rgb(0x808080));
                let at = ((y * n + x) * 4) as usize;
                pixels[at] = (color.r * 255.0).round() as u8;
                pixels[at + 1] = (color.g * 255.0).round() as u8;
                pixels[at + 2] = (color.b * 255.0).round() as u8;
                pixels[at + 3] = (color.a * coverage * 255.0).round() as u8;
            }
        }
        ImageData::new(n, n, pixels)
    }
}

/// The gap between slices (pixels).
const GAP: f32 = 2.0;

/// How far (in pixels, along the circle) the point at (`dx`, `dy`) is from the nearest edge
/// between two slices.
fn edge_distance(dx: f32, dy: f32, ends: &[f32]) -> f32 {
    let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
    let r = (dx * dx + dy * dy).sqrt();
    let edges = std::iter::once(0.0).chain(ends.iter().copied());
    edges
        .map(|e| {
            let d = (turn - e).abs();
            d.min(1.0 - d) * std::f32::consts::TAU * r
        })
        .fold(f32::MAX, f32::min)
}

/// The slice at offset (`dx`, `dy`) from the center (`ends`: where each slice ends, as a
/// fraction of the turn from the top, clockwise).
pub fn slice_at(dx: f32, dy: f32, ends: &[f32]) -> Option<usize> {
    let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
    ends.iter().position(|end| turn < *end).or_else(|| ends.len().checked_sub(1))
}

/// The slice of `shares` under offset (`dx`, `dy`) of a ring `radius` big, if it's on it.
pub fn hit(shares: &[f32], dx: f32, dy: f32, radius: f32) -> Option<usize> {
    let r = (dx * dx + dy * dy).sqrt() / radius.max(1.0);
    if !(HOLE..=1.0).contains(&r) {
        return None;
    }
    let total: f32 = shares.iter().sum::<f32>().max(f32::EPSILON);
    let mut sum = 0.0;
    let ends: Vec<f32> = shares
        .iter()
        .map(|s| {
            sum += s / total;
            sum
        })
        .collect();
    slice_at(dx, dy, &ends)
}

#[cfg(test)]
#[path = "../tests/unit/donut.rs"]
mod tests;
