//! A page exported as an SVG or a PNG, in draw.io's light colors whatever the editor's theme
//! (white shapes, black lines and texts; the page's background if it has one), with a margin
//! around it. The texts are laid out as the view lays them (`texts`), so they break the
//! same. The PNG is the view's own drawing — the shapes' tiles and the texts' glyphs —
//! composed on the CPU and kept only until it's written. Work for a worker.

use std::fmt::Write as _;

use kernel::{Point, Rect, Rgba};
use render_api::{GlyphFormat, GlyphRasterizer, Layer};
use text_layout::{FontFamily, TextSystem};
use vector_tiles::Seg;

use crate::drawing::{Drawing, Theme};

/// The room around the page (diagram units).
const MARGIN: f32 = 10.0;
/// The PNG's scale (its pixels per diagram unit): sharp on a high-density screen.
pub const PNG_SCALE: f32 = 2.0;
/// The PNG's largest side (pixels): a bigger page is exported smaller.
const MAX_SIDE: f32 = 8192.0;

/// draw.io's own colors.
pub fn light() -> Theme {
    Theme { canvas: Rgba::WHITE, fill: Rgba::WHITE, stroke: Rgba::BLACK, text: Rgba::BLACK }
}

/// The two formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Svg,
    Png,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
        }
    }
}

/// `#rrggbb`.
fn hex(c: Rgba) -> String {
    let [r, g, b, _] = c.to_rgba8();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// `text` escaped for XML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters aren't allowed in XML.
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// A color's attribute (`fill`, `stroke`) with its opacity when it has one.
fn paint(name: &str, c: Rgba) -> String {
    if c.a < 1.0 {
        format!(r#" {name}="{}" {name}-opacity="{:.3}""#, hex(c), c.a.max(0.0))
    } else {
        format!(r#" {name}="{}""#, hex(c))
    }
}

/// `segs` as a path's `d`, moved by `(dx, dy)`.
fn path_data(segs: &[Seg], dx: f32, dy: f32) -> String {
    let mut d = String::new();
    let p = |p: Point| format!("{:.2} {:.2}", p.x + dx, p.y + dy);
    for seg in segs {
        let _ = match *seg {
            Seg::Move(a) => write!(d, "M{} ", p(a)),
            Seg::Line(a) => write!(d, "L{} ", p(a)),
            Seg::Cubic(a, b, c) => write!(d, "C{} {} {} ", p(a), p(b), p(c)),
            Seg::Close => write!(d, "Z "),
        };
    }
    d.trim_end().to_owned()
}

/// The page as an SVG document.
pub fn svg(
    drawing: &Drawing,
    background: Option<Rgba>,
    text: &mut TextSystem,
    family: &FontFamily,
) -> String {
    let (w, h) =
        (drawing.bounds.size.width + 2.0 * MARGIN, drawing.bounds.size.height + 2.0 * MARGIN);
    let mut out = String::new();
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" viewBox="0 0 {w:.2} {h:.2}">"#
    );
    if let Some(bg) = background {
        let _ = writeln!(out, r#"<rect width="100%" height="100%"{}/>"#, paint("fill", bg));
    }
    for item in &drawing.items {
        let mut attrs = match item.fill {
            Some(c) => paint("fill", c),
            None => r#" fill="none""#.to_owned(),
        };
        if let Some(line) = &item.line {
            attrs += &paint("stroke", line.color);
            let _ = write!(attrs, r#" stroke-width="{}" stroke-linejoin="round""#, line.width);
            if let Some((dash, gap)) = line.dash {
                let _ = write!(attrs, r#" stroke-dasharray="{dash} {gap}""#);
            }
        }
        let _ = writeln!(out, r#"<path d="{}"{attrs}/>"#, path_data(&item.segs, MARGIN, MARGIN));
    }
    // The texts' boxes are absolute: moved to the page's corner.
    let (dx, dy) = (MARGIN - drawing.bounds.min_x(), MARGIN - drawing.bounds.min_y());
    for t in &drawing.texts {
        let area = Rect::new(
            t.area.min_x() + dx,
            t.area.min_y() + dy,
            t.area.size.width,
            t.area.size.height,
        );
        let (lines, line_h) = crate::texts::lay(text, family, t, area, 1.0);
        let weight = if t.bold { r#" font-weight="bold""# } else { "" };
        let style = if t.italic { r#" font-style="italic""# } else { "" };
        for line in lines.iter().filter(|l| !l.text.is_empty()) {
            let Point { x, y } = line.at;
            if let Some(back) = t.backdrop {
                let _ = writeln!(
                    out,
                    r#"<rect x="{:.2}" y="{y:.2}" width="{:.2}" height="{line_h:.2}"{}/>"#,
                    x - 2.0,
                    line.shaped.width + 4.0,
                    paint("fill", back)
                );
            }
            let _ = writeln!(
                out,
                r#"<text x="{x:.2}" y="{:.2}" font-family="Helvetica, Arial, sans-serif" font-size="{}"{weight}{style}{} xml:space="preserve">{}</text>"#,
                y + t.size,
                t.size,
                paint("fill", t.color),
                escape(&line.text)
            );
        }
    }
    out.push_str("</svg>\n");
    out
}

/// The page as a PNG at `scale` pixels per unit (less if it would be too big).
pub fn png(
    drawing: &Drawing,
    background: Option<Rgba>,
    text: &mut TextSystem,
    family: &FontFamily,
    scale: f32,
) -> Result<Vec<u8>, String> {
    let size =
        (drawing.bounds.size.width + 2.0 * MARGIN, drawing.bounds.size.height + 2.0 * MARGIN);
    if !(size.0.is_finite() && size.1.is_finite()) {
        return Err("a page too big".into());
    }
    // The scale asked for, less for a big page: its largest side within the limit (and the
    // image's sides never past it, whatever the rounding).
    let k = scale.min(MAX_SIDE / size.0.max(size.1).max(1.0));
    let side = |v: f32| (v * k).ceil().clamp(1.0, MAX_SIDE) as u32;
    let (w, h) = (side(size.0), side(size.1));
    // The background, opaque everywhere (what's drawn over it stays opaque).
    let [r, g, b, _] = background.unwrap_or(Rgba::WHITE).with_alpha(1.0).to_rgba8();
    let mut pixels: Vec<u8> = [r, g, b, 255].repeat((w as usize) * (h as usize));
    // The shapes, tile by tile (as the view makes them), moved by the margin: each tile's
    // masks composed as soon as it's made, then let go (never the whole page's at once).
    let inner = (drawing.bounds.size.width, drawing.bounds.size.height);
    let (across, down) = vector_tiles::grid(inner, k);
    let offset = MARGIN * k;
    for ty in 0..down {
        for tx in 0..across {
            let mut layer = Layer::default();
            for (image, r) in vector_tiles::tile(inner, drawing.shapes(), k, tx, ty) {
                let at =
                    Rect::new(r.min_x() + offset, r.min_y() + offset, r.size.width, r.size.height);
                layer.image(&image, at);
            }
            compose(&layer, text, &mut pixels, w, h);
        }
    }
    // The texts over them (a layer of their own, as in the view: a label's patch covers
    // the line under it).
    let mut texts = Layer::default();
    let (dx, dy) = (MARGIN - drawing.bounds.min_x(), MARGIN - drawing.bounds.min_y());
    for t in &drawing.texts {
        let area = Rect::new(
            (t.area.min_x() + dx) * k,
            (t.area.min_y() + dy) * k,
            t.area.size.width * k,
            t.area.size.height * k,
        );
        crate::texts::paint(&mut texts, text, family, t, area, k);
    }
    compose(&texts, text, &mut pixels, w, h);
    // Encoded from these pixels (no copy of the page).
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&pixels).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// `color` at `coverage` (0…1) over pixel `px` (RGBA8, opaque).
fn blend(px: &mut [u8], color: Rgba, coverage: f32) {
    let a = (color.a * coverage).clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let [r, g, b, _] = color.to_rgba8();
    for (dst, src) in px.iter_mut().zip([r, g, b]) {
        *dst = (f32::from(src) * a + f32::from(*dst) * (1.0 - a)).round() as u8;
    }
    px[3] = 255;
}

/// `layer` drawn into `pixels` (`w` × `h`, RGBA8): its quads (plain boxes), then its images
/// (the tiles' masks, unscaled), then its glyphs — the order a layer is drawn in.
fn compose(layer: &Layer, glyphs: &mut TextSystem, pixels: &mut [u8], w: u32, h: u32) {
    let index = |x: i64, y: i64| -> Option<usize> {
        (x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h))
            .then(|| (y as usize * w as usize + x as usize) * 4)
    };
    let span = |from: f32, to: f32, most: u32| {
        (from.round().max(0.0) as i64)..(to.round().min(most as f32).max(0.0) as i64)
    };
    for quad in &layer.quads {
        let b = quad.bounds;
        for y in span(b.min_y(), b.max_y(), h) {
            for x in span(b.min_x(), b.max_x(), w) {
                if let Some(i) = index(x, y) {
                    blend(&mut pixels[i..i + 4], quad.color, 1.0);
                }
            }
        }
    }
    for instance in &layer.images {
        let image = &instance.image;
        let (ox, oy) =
            (instance.bounds.min_x().round() as i64, instance.bounds.min_y().round() as i64);
        let (iw, ih) = (image.width, image.height);
        let tint = image.tint;
        image.with_pixels(|data| {
            for y in 0..ih {
                for x in 0..iw {
                    let Some(i) = index(ox + i64::from(x), oy + i64::from(y)) else { continue };
                    let at = (y * iw + x) as usize;
                    match tint {
                        Some(c) => blend(&mut pixels[i..i + 4], c, f32::from(data[at]) / 255.0),
                        None => {
                            let s = &data[at * 4..at * 4 + 4];
                            let c = Rgba::new(
                                f32::from(s[0]) / 255.0,
                                f32::from(s[1]) / 255.0,
                                f32::from(s[2]) / 255.0,
                                1.0,
                            );
                            blend(&mut pixels[i..i + 4], c, f32::from(s[3]) / 255.0);
                        }
                    }
                }
            }
        });
    }
    for g in &layer.glyphs {
        let Some(glyph) = glyphs.rasterize(g.key) else { continue };
        let left = g.origin.x.round() as i64 + i64::from(glyph.left);
        let top = g.origin.y.round() as i64 - i64::from(glyph.top);
        for gy in 0..glyph.height {
            for gx in 0..glyph.width {
                let Some(i) = index(left + i64::from(gx), top + i64::from(gy)) else { continue };
                let at = (gy * glyph.width + gx) as usize;
                match glyph.format {
                    GlyphFormat::Mask => {
                        blend(&mut pixels[i..i + 4], g.color, f32::from(glyph.data[at]) / 255.0)
                    }
                    GlyphFormat::Color => {
                        let s = &glyph.data[at * 4..at * 4 + 4];
                        let c = Rgba::new(
                            f32::from(s[0]) / 255.0,
                            f32::from(s[1]) / 255.0,
                            f32::from(s[2]) / 255.0,
                            1.0,
                        );
                        blend(&mut pixels[i..i + 4], c, f32::from(s[3]) / 255.0);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/export.rs"]
mod tests;
