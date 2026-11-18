//! Shapes rasterized in tiles, each tile as alpha masks — one byte of coverage per pixel
//! for each color it shows, cut to where that color is — instead of a whole RGBA image: a
//! quarter of the memory or less, and only the tiles shown are made (a diagram taller than
//! the window costs its visible part). The masks are computed so that drawn one over the
//! other, in order, they give exactly what painting the shapes would. Pure work for a
//! background thread (its buffers kept per thread, from tile to tile).

use std::cell::RefCell;

use kernel::{Rect, Rgba};
use render_api::ImageData;

use crate::path::{Line, Seg};

/// A tile's side (device pixels).
pub const TILE: u32 = 512;

/// The cells a tile's masks are cut into (each only where its color is).
const CELL: u32 = 128;
/// The most colors a tile is made of as masks (a weight buffer each, 1 MB for a full tile);
/// a tile with more is one RGBA image instead (as much as four masks).
const MAX_COLORS: usize = 8;

/// One color's mask in a tile: its image and where it goes, in device pixels from the
/// scene's corner.
pub type Mask = (ImageData, Rect);

/// A shape to rasterize: its outline, its fill, its line (either may be left out).
#[derive(Debug, Clone, Copy)]
pub struct Shape<'a> {
    pub segs: &'a [Seg],
    pub fill: Option<Rgba>,
    pub line: Option<&'a Line>,
}

/// The tiles a scene `size` big (its units) has across and down at scale `k`.
pub fn grid(size: (f32, f32), k: f32) -> (u32, u32) {
    let (w, h) = ((size.0 * k).ceil().max(1.0) as u32, (size.1 * k).ceil().max(1.0) as u32);
    (w.div_ceil(TILE), h.div_ceil(TILE))
}

/// What `shape` may paint (its units): its outline with its line's width around it. A tile
/// this box doesn't reach is the same with or without it.
pub fn shape_bounds(shape: &Shape<'_>) -> Rect {
    bounds(shape.segs, shape.line)
}

/// A shape's box in diagram pixels (its line's width around it).
fn bounds(segs: &[Seg], line: Option<&Line>) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) =
        (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut add = |p: &kernel::Point| {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    };
    for s in segs {
        match s {
            Seg::Move(p) | Seg::Line(p) => add(p),
            Seg::Cubic(a, b, c) => {
                add(a);
                add(b);
                add(c);
            }
            Seg::Close => {}
        }
    }
    let pad = line.map_or(1.0, |l| l.width / 2.0 + 1.0);
    if !x0.is_finite() {
        return Rect::default();
    }
    Rect::new(x0 - pad, y0 - pad, x1 - x0 + 2.0 * pad, y1 - y0 + 2.0 * pad)
}

fn skia_path(segs: &[Seg], k: f32, dx: f32, dy: f32) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    let t = |p: &kernel::Point| (p.x * k - dx, p.y * k - dy);
    for s in segs {
        match s {
            Seg::Move(p) => {
                let (x, y) = t(p);
                pb.move_to(x, y);
            }
            Seg::Line(p) => {
                let (x, y) = t(p);
                pb.line_to(x, y);
            }
            Seg::Cubic(a, b, c) => {
                let (ax, ay) = t(a);
                let (bx, by) = t(b);
                let (cx, cy) = t(c);
                pb.cubic_to(ax, ay, bx, by, cx, cy);
            }
            Seg::Close => pb.close(),
        }
    }
    pb.finish()
}

/// The buffers a worker reuses from tile to tile: the coverage of one shape, and each
/// color's weight.
#[derive(Default)]
struct Buffers {
    coverage: Option<tiny_skia::Mask>,
    weights: Vec<Vec<f32>>,
}

thread_local! {
    static BUFFERS: RefCell<Buffers> = RefCell::new(Buffers::default());
}

/// Tile `(tx, ty)` at scale `k` of a scene `size` big holding `shapes` (painted in
/// order): a mask per color it shows (none: an empty tile).
pub fn tile<'a>(
    size: (f32, f32),
    shapes: impl IntoIterator<Item = Shape<'a>>,
    k: f32,
    tx: u32,
    ty: u32,
) -> Vec<Mask> {
    let full_w = (size.0 * k).ceil().max(1.0) as u32;
    let full_h = (size.1 * k).ceil().max(1.0) as u32;
    let (x0, y0) = (tx * TILE, ty * TILE);
    if x0 >= full_w || y0 >= full_h {
        return Vec::new();
    }
    let (w, h) = (TILE.min(full_w - x0), TILE.min(full_h - y0));
    let area = Rect::new(x0 as f32 / k, y0 as f32 / k, w as f32 / k, h as f32 / k);
    // The painting operations reaching the tile, in order: a path and its color, filled
    // or stroked.
    let mut ops: Vec<(&[Seg], Rgba, Option<&Line>)> = Vec::new();
    for Shape { segs, fill, line } in shapes {
        let b = bounds(segs, line);
        if b.intersect(&area).is_none() {
            continue;
        }
        if let Some(fill) = fill.filter(|c| c.a > 0.0) {
            ops.push((segs, fill, None));
        }
        if let Some(l) = line.filter(|l| l.color.a > 0.0 && l.width > 0.0) {
            ops.push((segs, l.color, Some(l)));
        }
    }
    if ops.is_empty() {
        return Vec::new();
    }
    let mut colors: Vec<Rgba> = Vec::new();
    for (_, c, _) in &ops {
        let opaque = c.with_alpha(1.0);
        if !colors.contains(&opaque) {
            colors.push(opaque);
        }
    }
    if colors.len() > MAX_COLORS {
        return rgba_tile(&ops, k, (x0, y0), (w, h));
    }
    let n = (w * h) as usize;
    BUFFERS.with(|cell| {
        let mut buffers = cell.borrow_mut();
        let Buffers { coverage, weights } = &mut *buffers;
        if coverage.as_ref().is_none_or(|m| m.width() != w || m.height() != h) {
            *coverage = tiny_skia::Mask::new(w, h);
        }
        let Some(mask) = coverage.as_mut() else { return Vec::new() };
        // Never more kept than the most a tile uses.
        weights.truncate(MAX_COLORS);
        weights.resize_with(colors.len(), Vec::new);
        for wgt in weights.iter_mut().take(colors.len()) {
            wgt.clear();
            wgt.resize(n, 0.0);
        }
        let (dx, dy) = (x0 as f32, y0 as f32);
        for (segs, color, line) in &ops {
            let Some(filled) = outline(segs, *line, k, dx, dy) else { continue };
            mask.clear();
            mask.fill_path(
                &filled,
                tiny_skia::FillRule::Winding,
                true,
                tiny_skia::Transform::identity(),
            );
            let b = filled.bounds();
            let (bx0, by0) =
                ((b.left().floor().max(0.0)) as u32, (b.top().floor().max(0.0)) as u32);
            let (bx1, by1) = ((b.right().ceil() as u32).min(w), (b.bottom().ceil() as u32).min(h));
            let c = colors.iter().position(|x| *x == color.with_alpha(1.0)).unwrap_or(0);
            let alpha = color.a;
            let data = mask.data();
            // Painting over: what was under is covered by as much.
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let i = (y * w + x) as usize;
                    let m = f32::from(data[i]) / 255.0 * alpha;
                    if m <= 0.0 {
                        continue;
                    }
                    for (j, wgt) in weights.iter_mut().enumerate().take(colors.len()) {
                        let v = &mut wgt[i];
                        *v = if j == c { *v * (1.0 - m) + m } else { *v * (1.0 - m) };
                    }
                }
            }
        }
        // The masks for drawing in order, each over the ones before: from the last, each
        // weighs as much as what's left uncovered by the ones after it allows.
        let mut covered = vec![1.0f32; n];
        let mut out: Vec<(usize, Vec<u8>)> = Vec::with_capacity(colors.len());
        for j in (0..colors.len()).rev() {
            let wgt = &weights[j];
            let mut bytes = vec![0u8; n];
            let mut any = false;
            for i in 0..n {
                if wgt[i] <= 0.0 {
                    continue;
                }
                let a = (wgt[i] / covered[i].max(1e-4)).clamp(0.0, 1.0);
                covered[i] *= 1.0 - a;
                let b = (a * 255.0).round() as u8;
                if b > 0 {
                    bytes[i] = b;
                    any = true;
                }
            }
            if any {
                out.push((j, bytes));
            }
        }
        out.reverse();
        // Each color's mask cut into cells, each to where the color is in it: a color all
        // over the tile (the boxes' outlines) costs its parts, not their whole span.
        let mut masks = Vec::new();
        for (j, bytes) in out {
            for cy in (0..h).step_by(CELL as usize) {
                for cx in (0..w).step_by(CELL as usize) {
                    let (ex, ey) = ((cx + CELL).min(w), (cy + CELL).min(h));
                    let (mut x0c, mut y0c, mut x1c, mut y1c) = (ex, ey, cx, cy);
                    for y in cy..ey {
                        let row = &bytes[(y * w + cx) as usize..(y * w + ex) as usize];
                        if let Some(first) = row.iter().position(|b| *b > 0) {
                            let last = row.iter().rposition(|b| *b > 0).unwrap_or(first);
                            x0c = x0c.min(cx + first as u32);
                            x1c = x1c.max(cx + last as u32 + 1);
                            y0c = y0c.min(y);
                            y1c = y + 1;
                        }
                    }
                    if x1c <= x0c || y1c <= y0c {
                        continue;
                    }
                    let (cw, ch) = (x1c - x0c, y1c - y0c);
                    let mut cut = Vec::with_capacity((cw * ch) as usize);
                    for y in y0c..y1c {
                        let start = (y * w + x0c) as usize;
                        cut.extend_from_slice(&bytes[start..start + cw as usize]);
                    }
                    let rect =
                        Rect::new((x0 + x0c) as f32, (y0 + y0c) as f32, cw as f32, ch as f32);
                    masks.push((ImageData::mask(cw, ch, cut, colors[j]), rect));
                }
            }
        }
        masks
    })
}

/// What an operation covers, as a filled path: the shape, or its line's outline (dashed
/// when the line is).
fn outline(segs: &[Seg], line: Option<&Line>, k: f32, dx: f32, dy: f32) -> Option<tiny_skia::Path> {
    let path = skia_path(segs, k, dx, dy)?;
    let Some(l) = line else { return Some(path) };
    let mut stroke = tiny_skia::Stroke {
        width: l.width * k,
        line_join: tiny_skia::LineJoin::Round,
        line_cap: tiny_skia::LineCap::Round,
        ..tiny_skia::Stroke::default()
    };
    let dashed = l.dash.and_then(|(on, off)| {
        stroke.line_cap = tiny_skia::LineCap::Butt;
        path.dash(&tiny_skia::StrokeDash::new(vec![on * k, off * k], 0.0)?, 1.0)
    });
    dashed.as_ref().unwrap_or(&path).stroke(&stroke, 1.0)
}

/// A tile of many colors painted as one RGBA image (`w` × `h` at `(x0, y0)`).
fn rgba_tile(
    ops: &[(&[Seg], Rgba, Option<&Line>)],
    k: f32,
    (x0, y0): (u32, u32),
    (w, h): (u32, u32),
) -> Vec<Mask> {
    let Some(mut pixmap) = tiny_skia::Pixmap::new(w, h) else { return Vec::new() };
    for (segs, color, line) in ops {
        let Some(filled) = outline(segs, *line, k, x0 as f32, y0 as f32) else { continue };
        let [r, g, b, a] = color.to_rgba8();
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        pixmap.fill_path(
            &filled,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        bytes.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    // Its pixels let go once on the GPU, as a mask's are.
    vec![(ImageData::gpu_only(w, h, bytes), Rect::new(x0 as f32, y0 as f32, w as f32, h as f32))]
}

#[cfg(test)]
#[path = "../tests/unit/tiles.rs"]
pub(crate) mod tests;
