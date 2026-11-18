//! A diagram once laid out: shapes and lines (filled and stroked paths, in diagram
//! pixels) and its texts — what every diagram type turns into. The shapes are rasterized
//! together into one image at the screen's exact scale (`tiny-skia`, off the UI thread);
//! the texts are drawn by the app's text system over it (crisp, the UI's font). The same
//! scene is written as SVG and rendered to PNG for the clipboard.

use std::fmt::Write as _;

use kernel::{Point, Rect, Rgba};

pub use vector_tiles::{Line, Seg, ellipse, flowing, polygon, rounded};

/// Where a text is set from its point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

/// A text of the diagram: its point (`x` as `anchor` says, `y` its middle), its size
/// (diagram pixels) and look.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub at: Point,
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    /// In the math font (a formula's symbols).
    pub math: bool,
    pub color: Rgba,
    pub anchor: Anchor,
}

/// A drawn piece.
#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Path { segs: Vec<Seg>, fill: Option<Rgba>, line: Option<Line> },
    Text(Text),
}

/// A diagram laid out, in diagram pixels from (0, 0).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub width: f32,
    pub height: f32,
    pub prims: Vec<Prim>,
    /// Areas that link somewhere (a flowchart's `click`).
    pub links: Vec<(Rect, String)>,
}

impl Scene {
    pub fn path(&mut self, segs: Vec<Seg>, fill: Option<Rgba>, line: Option<Line>) {
        self.prims.push(Prim::Path { segs, fill, line });
    }

    pub fn rect(&mut self, r: Rect, radius: f32, fill: Option<Rgba>, line: Option<Line>) {
        self.path(rounded(r, radius), fill, line);
    }

    pub fn text(&mut self, text: Text) {
        self.prims.push(Prim::Text(text));
    }

    /// A polyline.
    pub fn polyline(&mut self, points: &[Point], line: Line) {
        let mut segs = Vec::with_capacity(points.len());
        for (i, p) in points.iter().enumerate() {
            segs.push(if i == 0 { Seg::Move(*p) } else { Seg::Line(*p) });
        }
        self.path(segs, None, Some(line));
    }

    /// Everything moved by `(dx, dy)`.
    pub fn shift(&mut self, dx: f32, dy: f32) {
        let m = |p: &mut Point| {
            p.x += dx;
            p.y += dy;
        };
        for prim in &mut self.prims {
            match prim {
                Prim::Path { segs, .. } => {
                    for s in segs {
                        match s {
                            Seg::Move(p) | Seg::Line(p) => m(p),
                            Seg::Cubic(a, b, c) => {
                                m(a);
                                m(b);
                                m(c);
                            }
                            Seg::Close => {}
                        }
                    }
                }
                Prim::Text(t) => m(&mut t.at),
            }
        }
        for (r, _) in &mut self.links {
            r.origin.x += dx;
            r.origin.y += dy;
        }
    }

    /// How many bytes it holds (the Task Manager).
    pub fn memory(&self) -> usize {
        self.prims
            .iter()
            .map(|p| match p {
                Prim::Path { segs, .. } => segs.len() * 28 + 48,
                Prim::Text(t) => t.text.len() + 64,
            })
            .sum()
    }
}

/// A head's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadKind {
    /// A filled triangle.
    Arrow,
    /// An open `>`.
    Open,
    Circle,
    Cross,
    /// A hollow triangle (inheritance, realization).
    Hollow,
    /// A filled diamond (composition), a hollow one (aggregation).
    Diamond,
    HollowDiamond,
    /// Crow's feet (an entity relation's cardinality): `|o`, `||`, `}o`, `}|`.
    ZeroOrOne,
    ExactlyOne,
    ZeroOrMore,
    OneOrMore,
}

impl HeadKind {
    /// It's drawn on the line (the line goes to its tip).
    pub fn on_line(self) -> bool {
        matches!(
            self,
            Self::ZeroOrOne | Self::ExactlyOne | Self::ZeroOrMore | Self::OneOrMore | Self::Open
        )
    }
}

/// A head of `size` at `tip`, pointing from `from` to `tip`.
pub fn head(
    scene: &mut Scene,
    from: Point,
    tip: Point,
    kind: HeadKind,
    size: f32,
    color: Rgba,
    background: Rgba,
) {
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let len = (dx * dx + dy * dy).sqrt().max(0.001);
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy, ux);
    let at = |along: f32, side: f32| {
        Point::new(tip.x - ux * along + nx * side, tip.y - uy * along + ny * side)
    };
    let line = Line::solid(color, 1.4);
    match kind {
        HeadKind::Arrow => {
            scene.path(
                polygon(&[tip, at(size, size * 0.5), at(size * 0.8, 0.0), at(size, -size * 0.5)]),
                Some(color),
                None,
            );
        }
        HeadKind::Open => {
            scene.path(
                vec![
                    Seg::Move(at(size, size * 0.5)),
                    Seg::Line(tip),
                    Seg::Line(at(size, -size * 0.5)),
                ],
                None,
                Some(line),
            );
        }
        HeadKind::Hollow => {
            scene.path(
                polygon(&[tip, at(size * 1.2, size * 0.6), at(size * 1.2, -size * 0.6)]),
                Some(background),
                Some(line),
            );
        }
        HeadKind::Diamond | HeadKind::HollowDiamond => {
            let fill = if kind == HeadKind::Diamond { color } else { background };
            scene.path(
                polygon(&[
                    tip,
                    at(size * 0.6, size * 0.4),
                    at(size * 1.2, 0.0),
                    at(size * 0.6, -size * 0.4),
                ]),
                Some(fill),
                Some(line),
            );
        }
        HeadKind::ZeroOrOne | HeadKind::ExactlyOne | HeadKind::ZeroOrMore | HeadKind::OneOrMore => {
            let bar = |scene: &mut Scene, along: f32| {
                scene.path(
                    vec![Seg::Move(at(along, size * 0.55)), Seg::Line(at(along, -size * 0.55))],
                    None,
                    Some(line),
                );
            };
            let ring = |scene: &mut Scene, along: f32| {
                let c = at(along, 0.0);
                let r = size * 0.32;
                scene.path(
                    ellipse(Rect::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r)),
                    Some(background),
                    Some(line),
                );
            };
            let many = matches!(kind, HeadKind::ZeroOrMore | HeadKind::OneOrMore);
            if many {
                // The foot: from a point on the line to the box, spread.
                let base = at(size * 1.1, 0.0);
                for side in [size * 0.6, 0.0, -size * 0.6] {
                    scene.path(vec![Seg::Move(base), Seg::Line(at(0.0, side))], None, Some(line));
                }
            }
            match kind {
                HeadKind::ExactlyOne => {
                    bar(scene, size * 0.5);
                    bar(scene, size * 0.9);
                }
                HeadKind::ZeroOrOne => {
                    bar(scene, size * 0.5);
                    ring(scene, size * 1.2);
                }
                HeadKind::OneOrMore => bar(scene, size * 1.35),
                _ => ring(scene, size * 1.55),
            }
        }
        HeadKind::Circle => {
            let c = at(size * 0.4, 0.0);
            let r = size * 0.4;
            scene.path(
                ellipse(Rect::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r)),
                Some(background),
                Some(line),
            );
        }
        HeadKind::Cross => {
            let c = at(size * 0.5, 0.0);
            let s = size * 0.4;
            let p = |a: f32, b: f32| {
                Point::new(c.x + (ux * a + nx * b) * s, c.y + (uy * a + ny * b) * s)
            };
            scene.path(
                vec![Seg::Move(p(-1.0, -1.0)), Seg::Line(p(1.0, 1.0))],
                None,
                Some(Line::solid(color, 2.0)),
            );
            scene.path(
                vec![Seg::Move(p(-1.0, 1.0)), Seg::Line(p(1.0, -1.0))],
                None,
                Some(Line::solid(color, 2.0)),
            );
        }
    }
}

// ---------------------------------------------------------------------------------------
// Raster.

fn draw_shapes(pixmap: &mut tiny_skia::Pixmap, scene: &Scene, k: f32) {
    draw_paths(pixmap, scene, k, None);
}

/// The scene's paths at scale `k` (only those in color `only`, when given).
pub fn draw_paths(pixmap: &mut tiny_skia::Pixmap, scene: &Scene, k: f32, only: Option<Rgba>) {
    let transform = tiny_skia::Transform::from_scale(k, k);
    let wanted = |c: &Rgba| only.is_none_or(|o| o == *c);
    for prim in &scene.prims {
        let Prim::Path { segs, fill, line } = prim else { continue };
        let Some(path) = skia_path(segs) else { continue };
        if let Some(fill) = fill
            && fill.a > 0.0
            && wanted(fill)
        {
            let paint = paint(*fill);
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
        }
        if let Some(line) = line
            && line.color.a > 0.0
            && line.width > 0.0
            && wanted(&line.color)
        {
            let mut stroke = tiny_skia::Stroke {
                width: line.width,
                line_join: tiny_skia::LineJoin::Round,
                line_cap: tiny_skia::LineCap::Round,
                ..tiny_skia::Stroke::default()
            };
            if let Some((on, off)) = line.dash {
                stroke.line_cap = tiny_skia::LineCap::Butt;
                stroke.dash = tiny_skia::StrokeDash::new(vec![on, off], 0.0);
            }
            pixmap.stroke_path(&path, &paint(line.color), &stroke, transform, None);
        }
    }
}

fn paint(c: Rgba) -> tiny_skia::Paint<'static> {
    let mut paint = tiny_skia::Paint { anti_alias: true, ..tiny_skia::Paint::default() };
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    paint.set_color_rgba8(b(c.r), b(c.g), b(c.b), b(c.a));
    paint
}

fn skia_path(segs: &[Seg]) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    for s in segs {
        match *s {
            Seg::Move(p) => pb.move_to(p.x, p.y),
            Seg::Line(p) => pb.line_to(p.x, p.y),
            Seg::Cubic(a, b, p) => pb.cubic_to(a.x, a.y, b.x, b.y, p.x, p.y),
            Seg::Close => pb.close(),
        }
    }
    pb.finish()
}

/// What draws a scene's texts into an image (the PNG export): the text's glyphs, shaped
/// and rasterized by the app's text system, as coverage over the pixels.
pub trait GlyphPainter {
    /// The glyphs of `text` (diagram size × `k`): each one's coverage mask (or color),
    /// its position from the text's start on its baseline, and the text's width and
    /// ascent / descent.
    fn glyphs(&mut self, text: &Text, k: f32) -> ShapedText;
}

/// A text as glyph bitmaps.
pub struct ShapedText {
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
    /// Left, top (from the baseline, downward positive), width, height, coverage (one byte
    /// per pixel) or RGBA (`color`).
    pub glyphs: Vec<(i32, i32, u32, u32, Vec<u8>, bool)>,
}

/// The whole scene (its texts too) at scale `k`, on `background`: its size, its
/// straight-alpha RGBA pixels and the same as a PNG.
pub fn picture(
    scene: &Scene,
    k: f32,
    background: Rgba,
    painter: &mut dyn GlyphPainter,
) -> Option<(u32, u32, Vec<u8>, Vec<u8>)> {
    let (w, h) =
        ((scene.width * k).ceil().max(1.0) as u32, (scene.height * k).ceil().max(1.0) as u32);
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    pixmap.fill(tiny_skia::Color::from_rgba8(
        b(background.r),
        b(background.g),
        b(background.b),
        255,
    ));
    draw_shapes(&mut pixmap, scene, k);
    for prim in &scene.prims {
        let Prim::Text(t) = prim else { continue };
        let shaped = painter.glyphs(t, k);
        let x0 = match t.anchor {
            Anchor::Start => t.at.x * k,
            Anchor::Middle => t.at.x * k - shaped.width / 2.0,
            Anchor::End => t.at.x * k - shaped.width,
        };
        let baseline = t.at.y * k + (shaped.ascent - shaped.descent) / 2.0;
        for (left, top, gw, gh, data, color) in &shaped.glyphs {
            let gx = (x0 + *left as f32).round() as i32;
            let gy = (baseline + *top as f32).round() as i32;
            blend_glyph(&mut pixmap, (gx, gy, *gw, *gh), data, *color, t.color);
        }
    }
    let png = pixmap.encode_png().ok()?;
    let (w, h) = (pixmap.width(), pixmap.height());
    Some((w, h, pixmap.take_demultiplied(), png))
}

fn blend_glyph(
    pixmap: &mut tiny_skia::Pixmap,
    (gx, gy, gw, gh): (i32, i32, u32, u32),
    data: &[u8],
    color: bool,
    tint: Rgba,
) {
    let (w, h) = (pixmap.width() as i32, pixmap.height() as i32);
    let pixels = pixmap.data_mut();
    for row in 0..gh as i32 {
        for col in 0..gw as i32 {
            let (x, y) = (gx + col, gy + row);
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            let i = (row * gw as i32 + col) as usize;
            let (r, g, b, a) = if color {
                let p = &data[i * 4..i * 4 + 4];
                (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0)
            } else {
                (tint.r, tint.g, tint.b, data[i] as f32 / 255.0 * tint.a)
            };
            if a <= 0.0 {
                continue;
            }
            let o = ((y * w + x) * 4) as usize;
            // Premultiplied over premultiplied (the background is opaque).
            for (c, v) in [(0, r), (1, g), (2, b)] {
                let dst = pixels[o + c] as f32 / 255.0;
                pixels[o + c] = ((v * a + dst * (1.0 - a)) * 255.0).round() as u8;
            }
            pixels[o + 3] = 255;
        }
    }
}

// ---------------------------------------------------------------------------------------
// SVG.

fn hex(c: Rgba) -> String {
    c.to_hex()
}

/// The scene as an SVG document (texts as `<text>` in `font`).
pub fn svg(scene: &Scene, background: Option<Rgba>, font: &str) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.1} {h:.1}\">",
        w = scene.width.ceil(),
        h = scene.height.ceil()
    );
    if let Some(bg) = background {
        let _ = write!(out, "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>", hex(bg));
    }
    for prim in &scene.prims {
        match prim {
            Prim::Path { segs, fill, line } => {
                let mut d = String::new();
                for s in segs {
                    let _ = match *s {
                        Seg::Move(p) => write!(d, "M{:.1} {:.1}", p.x, p.y),
                        Seg::Line(p) => write!(d, "L{:.1} {:.1}", p.x, p.y),
                        Seg::Cubic(a, b, p) => write!(
                            d,
                            "C{:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
                            a.x, a.y, b.x, b.y, p.x, p.y
                        ),
                        Seg::Close => write!(d, "Z"),
                    };
                }
                let fill = fill.map_or("none".to_owned(), hex);
                let _ = write!(out, "<path d=\"{d}\" fill=\"{fill}\"");
                if let Some(f) = prim_fill_alpha(prim) {
                    let _ = write!(out, " fill-opacity=\"{f:.2}\"");
                }
                if let Some(l) = line {
                    let _ = write!(
                        out,
                        " stroke=\"{}\" stroke-width=\"{:.1}\" stroke-linejoin=\"round\"",
                        hex(l.color),
                        l.width
                    );
                    if l.color.a < 1.0 {
                        let _ = write!(out, " stroke-opacity=\"{:.2}\"", l.color.a);
                    }
                    if let Some((on, off)) = l.dash {
                        let _ = write!(out, " stroke-dasharray=\"{on} {off}\"");
                    }
                }
                out.push_str("/>");
            }
            Prim::Text(t) => {
                let anchor = match t.anchor {
                    Anchor::Start => "start",
                    Anchor::Middle => "middle",
                    Anchor::End => "end",
                };
                let family = if t.mono {
                    "monospace"
                } else if t.math {
                    "STIX Two Math, Cambria Math, serif"
                } else {
                    font
                };
                let _ = write!(
                    out,
                    "<text x=\"{:.1}\" y=\"{:.1}\" font-family=\"{}\" font-size=\"{:.1}\" fill=\"{}\" text-anchor=\"{anchor}\" dominant-baseline=\"central\"{}{}>{}</text>",
                    t.at.x,
                    t.at.y,
                    escape(family),
                    t.size,
                    hex(t.color),
                    if t.bold { " font-weight=\"bold\"" } else { "" },
                    if t.italic { " font-style=\"italic\"" } else { "" },
                    escape(&t.text)
                );
            }
        }
    }
    out.push_str("</svg>");
    out
}

fn prim_fill_alpha(prim: &Prim) -> Option<f32> {
    match prim {
        Prim::Path { fill: Some(c), .. } if c.a < 1.0 => Some(c.a),
        _ => None,
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/scene.rs"]
mod tests;
