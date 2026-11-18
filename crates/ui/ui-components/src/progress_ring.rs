//! A ring drawn smoothly: an arc with round caps over a faint full track, anti-aliased by
//! its coverage of each pixel. The renderers draw only quads, glyphs and images, so the
//! ring is rasterized here, on the CPU, into a small image at the device pixel size (drawn
//! 1:1, never scaled), and kept until what it shows changes: its size, stroke, colors or
//! the arc, quantized to [`STEPS`] a turn. Unchanged, a frame only draws the image it has
//! (nothing allocated, nothing uploaded again); the renderer frees the textures no longer
//! drawn.

use kernel::cache::{Memo, Purge};
use kernel::{Point, Rect, Rgba};
use render_api::{ImageData, ImageInstance, Layer};

/// The arc's steps for a whole turn (where it starts and how far it goes): one a degree,
/// so a frame at the stack's pace (about 30 a second) moves it by less than the eye sees.
pub const STEPS: u16 = 360;

/// What a ring looks like (device pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RingSpec {
    /// The image's side: the ring is centered in it.
    pub size: u32,
    /// The radius of the stroke's middle.
    pub radius: f32,
    /// The stroke's width.
    pub stroke: f32,
    /// The arc's color.
    pub color: Rgba,
    /// The full ring under the arc (`None`: only the arc).
    pub track: Option<Rgba>,
}

/// `turns` (0..=1) as steps of [`STEPS`] a turn.
pub fn quantize(turns: f32) -> u16 {
    (turns.clamp(0.0, 1.0) * f32::from(STEPS)).round() as u16
}

/// How much of a pixel a shape covers, from the signed distance of the pixel's center to
/// its edge (negative inside): a one-pixel-wide ramp around the edge.
pub fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// The signed distance from `(dx, dy)` (from the center, y down) to an arc of `radius`
/// and `stroke` with round caps, starting at `start` turns clockwise from the top and
/// going `sweep` turns (a whole turn: a closed ring, no caps).
pub fn arc_distance(dx: f32, dy: f32, radius: f32, stroke: f32, start: f32, sweep: f32) -> f32 {
    use std::f32::consts::TAU;
    let half = stroke / 2.0;
    let band = (dx.hypot(dy) - radius).abs() - half;
    if sweep >= 1.0 {
        return band;
    }
    if sweep <= 0.0 {
        return f32::INFINITY;
    }
    // Clockwise from the top (y grows downwards).
    let angle = dx.atan2(-dy).rem_euclid(TAU);
    let from = (start * TAU).rem_euclid(TAU);
    if (angle - from).rem_euclid(TAU) <= sweep * TAU {
        return band;
    }
    // Past either end: the distance to that end's cap.
    let cap = |turns: f32| {
        let a = turns * TAU;
        let (x, y) = (radius * a.sin(), -radius * a.cos());
        (dx - x).hypot(dy - y) - half
    };
    cap(start).min(cap(start + sweep))
}

/// The ring's pixels (RGBA8, not premultiplied, `size`² of them): the arc from `start`
/// going `sweep` turns over the track. The color of the transparent pixels is the arc's,
/// so filtering never darkens the edges.
pub fn rasterize(spec: &RingSpec, start: f32, sweep: f32) -> Vec<u8> {
    let n = spec.size as usize;
    let mut pixels = Vec::with_capacity(n * n * 4);
    let mid = spec.size as f32 / 2.0;
    let arc = spec.color;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 + 0.5 - mid, y as f32 + 0.5 - mid);
            let a1 = arc.a * coverage(arc_distance(dx, dy, spec.radius, spec.stroke, start, sweep));
            let (c0, a0) = match spec.track {
                Some(track) => {
                    let d = arc_distance(dx, dy, spec.radius, spec.stroke, 0.0, 1.0);
                    (track, track.a * coverage(d))
                }
                None => (arc, 0.0),
            };
            // The arc over the track.
            let under = a0 * (1.0 - a1);
            let a = a1 + under;
            let mix = |top: f32, bottom: f32| {
                if a > 0.0 { (top * a1 + bottom * under) / a } else { top }
            };
            pixels.extend_from_slice(
                &Rgba::new(mix(arc.r, c0.r), mix(arc.g, c0.g), mix(arc.b, c0.b), a).to_rgba8(),
            );
        }
    }
    pixels
}

/// What a ring's image was made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RingKey {
    size: u32,
    /// In 1/64 of a pixel.
    radius: u32,
    stroke: u32,
    color: [u8; 4],
    track: Option<[u8; 4]>,
    start: u16,
    sweep: u16,
}

/// A ring kept as an image until what it shows changes (see the module's notes).
#[derive(Debug, Default)]
pub struct ProgressRing {
    image: Memo<RingKey, ImageData>,
}

impl ProgressRing {
    /// The image of the arc from `start` going `sweep` turns (both quantized to [`STEPS`]
    /// a turn): rasterized again only when it differs from the one kept.
    pub fn image(&mut self, spec: &RingSpec, start: f32, sweep: f32) -> &ImageData {
        let q = |v: f32| (v.max(0.0) * 64.0).round() as u32;
        let key = RingKey {
            size: spec.size,
            radius: q(spec.radius),
            stroke: q(spec.stroke),
            color: spec.color.to_rgba8(),
            track: spec.track.map(Rgba::to_rgba8),
            start: quantize(start.rem_euclid(1.0)) % STEPS,
            sweep: quantize(sweep),
        };
        self.image.get_or_update(key, || {
            let turns = |steps: u16| f32::from(steps) / f32::from(STEPS);
            let pixels = rasterize(spec, turns(key.start), turns(key.sweep));
            ImageData::new(spec.size, spec.size, pixels)
        })
    }

    /// Draws the ring centered on `center` (its corner on whole pixels), at `opacity`.
    pub fn paint(
        &mut self,
        layer: &mut Layer,
        center: Point,
        spec: &RingSpec,
        (start, sweep): (f32, f32),
        opacity: f32,
    ) {
        if spec.size == 0 {
            return;
        }
        let side = spec.size as f32;
        let bounds =
            Rect::new((center.x - side / 2.0).round(), (center.y - side / 2.0).round(), side, side);
        let image = self.image(spec, start, sweep).clone();
        layer.images.push(ImageInstance { image, bounds, opacity });
    }

    /// The bytes its image holds.
    pub fn memory(&self) -> usize {
        self.image.value().map_or(0, ImageData::memory)
    }

    /// The image kept, if any (tests).
    pub fn kept(&self) -> Option<&ImageData> {
        self.image.value()
    }
}

impl Purge for ProgressRing {
    fn purge(&mut self) {
        self.image.purge();
    }
}

#[cfg(test)]
#[path = "../tests/unit/progress_ring.rs"]
mod tests;
