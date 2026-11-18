//! TeX formulas typeset: the tree `tex` reads, set in boxes TeX's way (simplified) —
//! atoms spaced by their classes, scripts raised and lowered, fractions on the math axis,
//! roots, big operators with their limits, delimiters and matrices grown to what they
//! hold, accents. The symbols are glyphs of the math font the module brings (STIX Two Math,
//! OFL — the same formulas on every system), in a text system of its own: made with the
//! first preview showing a formula, dropped with the last. Glyphs, bars, radicals and
//! delimiters are drawn together into one mask per color (on the render pool).

use std::sync::{Arc, Mutex, Weak};

use kernel::{Point, Rect, Rgba};
use render_api::{GlyphRasterizer, ImageData};
use tex::{Accent, Class, Env, Font as TexFont, Node};
use text_layout::{Font, FontFamily, TextSystem};

use crate::mermaid::scene::{self, Anchor, Line, Prim, Scene, Seg, Text};

/// The math font (STIX Two Math 2.13, SIL Open Font License 1.1: `data/STIXTwoMath-OFL.txt`),
/// compressed (xz).
const FONT: &[u8] = include_bytes!("../data/STIXTwoMath-Regular.otf.xz");

/// The math font's text system, shared by every preview showing formulas.
pub type MathText = Arc<Mutex<TextSystem>>;

/// The math font's text system: the one alive, or made (the font decompressed) if none is.
pub fn math_text() -> MathText {
    static SHARED: Mutex<Weak<Mutex<TextSystem>>> = Mutex::new(Weak::new());
    let mut shared = SHARED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(text) = shared.upgrade() {
        return text;
    }
    let mut font = Vec::with_capacity(900_000);
    let decoded =
        std::io::Read::read_to_end(&mut lzma_rust2::XzReader::new(FONT, false), &mut font);
    let fonts = if decoded.is_ok() { vec![font] } else { Vec::new() };
    let text = Arc::new(Mutex::new(TextSystem::with_fonts(fonts)));
    *shared = Arc::downgrade(&text);
    text
}

/// The math font's family in `text`.
fn family(text: &TextSystem) -> FontFamily {
    text.families().first().map_or(FontFamily::SansSerif, |f| FontFamily::Named(f.as_str().into()))
}

/// A formula set: its scene (diagram pixels from its top left), its baseline's distance
/// from the top and its depth under it, its glyphs and shapes as masks.
pub struct Formula {
    pub scene: Scene,
    pub ascent: f32,
    pub descent: f32,
    /// What it draws: a mask per color (device pixels from its corner).
    pub masks: Vec<crate::tiles::Mask>,
}

/// A formula, or why it isn't one.
pub type Typeset = Result<Arc<Formula>, String>;

/// Sets `source` at `size` (display style: centered blocks, big operators, limits above
/// and below), in `color`, drawn at `scale`, with the math font's `text` system.
pub fn typeset(
    source: &str,
    display: bool,
    size: f32,
    color: Rgba,
    error: Rgba,
    text: &mut TextSystem,
    scale: f32,
) -> Typeset {
    let nodes = tex::parse(source).map_err(|e| format!("{:?}", e.kind))?;
    let fam = family(text);
    let mut t = Typesetter { text, family: fam, math_font: true, color, error };
    let style = if display { Style::Display } else { Style::Text };
    let b = t.list(&nodes, style, size);
    let ascent = b.asc + 2.0;
    let descent = b.desc + 2.0;
    let mut scene =
        Scene { width: b.w.max(1.0) + 2.0, height: ascent + descent, ..Scene::default() };
    for item in b.items {
        t.emit(&mut scene, item, 1.0, ascent);
    }
    let masks = masks(&scene, scale, t.text, &t.family);
    Ok(Arc::new(Formula { scene, ascent, descent, masks }))
}

/// The scene at scale `k` as a mask per color: its paths and its glyphs together.
fn masks(
    scene: &Scene,
    k: f32,
    text: &mut TextSystem,
    family: &FontFamily,
) -> Vec<crate::tiles::Mask> {
    let (w, h) =
        ((scene.width * k).ceil().max(1.0) as u32, (scene.height * k).ceil().max(1.0) as u32);
    let mut colors: Vec<Rgba> = Vec::new();
    let mut add = |c: Rgba| {
        if c.a > 0.0 && !colors.contains(&c) {
            colors.push(c);
        }
    };
    for prim in &scene.prims {
        match prim {
            Prim::Path { fill, line, .. } => {
                if let Some(f) = fill {
                    add(*f);
                }
                if let Some(l) = line {
                    add(l.color);
                }
            }
            Prim::Text(t) => add(t.color),
        }
    }
    let mut out = Vec::new();
    for color in colors {
        let Some(mut pixmap) = tiny_skia::Pixmap::new(w, h) else { continue };
        scene::draw_paths(&mut pixmap, scene, k, Some(color));
        let mut alpha: Vec<u8> = pixmap.pixels().iter().map(|p| p.alpha()).collect();
        for prim in &scene.prims {
            let Prim::Text(t) = prim else { continue };
            if t.color != color {
                continue;
            }
            let weight = if t.bold { Font::SEMIBOLD } else { Font::NORMAL };
            let font = Font::new(family.clone(), (t.size * k).round().max(1.0))
                .with_weight(weight)
                .with_italic(t.italic);
            let metrics = text.metrics(&font);
            let shaped = text.shape_line(&t.text, &font);
            let x0 = t.at.x * k;
            let baseline = t.at.y * k + (metrics.ascent - metrics.descent) / 2.0;
            for g in &shaped.glyphs {
                let Some(r) = text.rasterize(g.key) else { continue };
                let gx = (x0 + g.x).round() as i32 + r.left;
                let gy = (baseline + g.y).round() as i32 - r.top;
                let color_glyph = r.format == render_api::GlyphFormat::Color;
                for row in 0..r.height as i32 {
                    for col in 0..r.width as i32 {
                        let (x, y) = (gx + col, gy + row);
                        if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                            continue;
                        }
                        let i = (row * r.width as i32 + col) as usize;
                        let c = if color_glyph { r.data[i * 4 + 3] } else { r.data[i] };
                        let o = &mut alpha[(y as u32 * w + x as u32) as usize];
                        // Coverage over coverage.
                        *o = (f32::from(*o) + f32::from(c) * (1.0 - f32::from(*o) / 255.0))
                            .round()
                            .min(255.0) as u8;
                    }
                }
            }
        }
        // Cut to where it is.
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            let row = &alpha[(y * w) as usize..((y + 1) * w) as usize];
            if let Some(first) = row.iter().position(|b| *b > 0) {
                let last = row.iter().rposition(|b| *b > 0).unwrap_or(first);
                x0 = x0.min(first as u32);
                x1 = x1.max(last as u32 + 1);
                y0 = y0.min(y);
                y1 = y + 1;
            }
        }
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let (cw, ch) = (x1 - x0, y1 - y0);
        let mut cut = Vec::with_capacity((cw * ch) as usize);
        for y in y0..y1 {
            let start = (y * w + x0) as usize;
            cut.extend_from_slice(&alpha[start..start + cw as usize]);
        }
        let place = Rect::new(x0 as f32, y0 as f32, cw as f32, ch as f32);
        out.push((ImageData::mask(cw, ch, cut, color.with_alpha(1.0)), place));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Display,
    Text,
    Script,
    ScriptScript,
}

impl Style {
    fn script(self) -> Self {
        match self {
            Self::Display | Self::Text => Self::Script,
            _ => Self::ScriptScript,
        }
    }

    fn fraction(self) -> Self {
        match self {
            Self::Display => Self::Text,
            Self::Text => Self::Script,
            _ => Self::ScriptScript,
        }
    }

    fn shrink(self, base: f32) -> f32 {
        match self {
            Self::Display | Self::Text => base,
            Self::Script => base * 0.72,
            Self::ScriptScript => base * 0.55,
        }
    }

    fn tight(self) -> bool {
        matches!(self, Self::Script | Self::ScriptScript)
    }
}

/// What a box holds, from its left on its baseline (`y` up).
#[derive(Debug, Clone)]
enum Item {
    Glyph {
        x: f32,
        y: f32,
        text: String,
        size: f32,
        italic: bool,
        bold: bool,
        color: Rgba,
        ui: bool,
    },
    Rule {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: Rgba,
    },
    Path {
        x: f32,
        y: f32,
        segs: Vec<Seg>,
        width: f32,
        color: Rgba,
        fill: bool,
    },
}

impl Item {
    fn moved(self, dx: f32, dy: f32) -> Self {
        match self {
            Self::Glyph { x, y, text, size, italic, bold, color, ui } => {
                Self::Glyph { x: x + dx, y: y + dy, text, size, italic, bold, color, ui }
            }
            Self::Rule { x, y, w, h, color } => Self::Rule { x: x + dx, y: y + dy, w, h, color },
            Self::Path { x, y, segs, width, color, fill } => {
                Self::Path { x: x + dx, y: y + dy, segs, width, color, fill }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Box_ {
    w: f32,
    asc: f32,
    desc: f32,
    items: Vec<Item>,
}

impl Box_ {
    fn add(&mut self, other: Box_, dx: f32, dy: f32) {
        self.asc = self.asc.max(other.asc + dy);
        self.desc = self.desc.max(other.desc - dy);
        self.items.extend(other.items.into_iter().map(|i| i.moved(dx, dy)));
    }
}

struct Typesetter<'a> {
    text: &'a mut TextSystem,
    family: FontFamily,
    math_font: bool,
    color: Rgba,
    error: Rgba,
}

/// The math axis (fraction bars, operators' centers) above the baseline.
fn axis(size: f32) -> f32 {
    size * 0.25
}

fn rule(size: f32) -> f32 {
    (size * 0.055).max(0.8)
}

impl Typesetter<'_> {
    fn font(&self, size: f32, italic: bool, bold: bool, _ui: bool) -> Font {
        let family = self.family.clone();
        let weight = if bold { Font::BOLD } else { Font::NORMAL };
        Font::new(family, size * 4.0).with_italic(italic).with_weight(weight)
    }

    fn width(&mut self, s: &str, size: f32, italic: bool, bold: bool, ui: bool) -> f32 {
        let font = self.font(size, italic, bold, ui);
        self.text.shape_line(s, &font).width / 4.0
    }

    /// A glyph run as a box (its height from the font's size: digits and capitals reach
    /// about 0.7 of it, descenders 0.2).
    fn glyphs(&mut self, s: &str, size: f32, italic: bool, bold: bool, color: Rgba) -> Box_ {
        let w = self.width(s, size, italic, bold, false);
        let deep = s.chars().any(|c| {
            "gjpqyβγζημξρφχψ(){}[]|∫∑∏,;ℊ".contains(c)
                || ('\u{1d400}'..='\u{1d7ff}').contains(&c) && "gjpqy".contains(plain(c))
        });
        let tall = s.chars().any(|c| {
            c.is_uppercase()
                || c.is_ascii_digit()
                || "bdfhklt∂∫∑∏(){}[]|ℓ√".contains(c)
                || ('\u{1d400}'..='\u{1d7ff}').contains(&c)
        });
        Box_ {
            w,
            asc: size * if tall { 0.72 } else { 0.52 },
            desc: size * if deep { 0.22 } else { 0.02 },
            items: vec![Item::Glyph {
                x: 0.0,
                y: 0.0,
                text: s.to_owned(),
                size,
                italic,
                bold,
                color,
                ui: false,
            }],
        }
    }

    /// A run of atoms, spaced by their classes.
    fn list(&mut self, nodes: &[Node], style: Style, base: f32) -> Box_ {
        let size = style.shrink(base);
        let mut out = Box_::default();
        let mut previous: Option<Class> = None;
        for node in nodes {
            let mut class = class_of(node);
            // A binary operator with nothing to its left is unary (`-x`).
            if class == Class::Bin
                && matches!(
                    previous,
                    None | Some(Class::Bin | Class::Rel | Class::Open | Class::Punct | Class::Op)
                )
            {
                class = Class::Ord;
            }
            if let Some(p) = previous {
                out.w += spacing(p, class, style) * size;
            }
            let b = self.node(node, style, base);
            let w = b.w;
            out.add(b, out.w, 0.0);
            out.w += w;
            if !matches!(node, Node::Space(_)) {
                previous = Some(class);
            }
        }
        if nodes.is_empty() {
            out.asc = size * 0.5;
        }
        out
    }

    fn node(&mut self, node: &Node, style: Style, base: f32) -> Box_ {
        let size = style.shrink(base);
        match node {
            Node::Symbol { text, class, font } => {
                let (text, italic, bold) = self.styled(text, *font);
                let mut b = self.glyphs(&text, size, italic, bold, self.color);
                if *class == Class::Op {
                    b.asc = size * 0.75;
                }
                b
            }
            Node::Group(nodes) => self.list(nodes, style, base),
            Node::Text { text, font } => {
                let bold = *font == TexFont::Bold;
                let italic = *font == TexFont::Italic;
                self.glyphs(text, size, italic, bold, self.color)
            }
            Node::Space(em) => Box_ { w: em * size, ..Box_::default() },
            Node::Error(name) => {
                let mut b = self.glyphs(name, size, false, false, self.error);
                if let Some(Item::Glyph { ui, .. }) = b.items.first_mut() {
                    *ui = true;
                }
                b.w = self.width(name, size, false, false, true);
                b
            }
            Node::Colored { color, body } => {
                let saved = self.color;
                if let Some(c) = diagram_text::mermaid::color(color) {
                    self.color = c;
                }
                let b = self.node(body, style, base);
                self.color = saved;
                b
            }
            Node::Scripts { base: nucleus, sub, sup } => {
                self.scripts(nucleus, sub.as_deref(), sup.as_deref(), style, base)
            }
            Node::BigOp { text, symbol, .. } => self.big_op(text, *symbol, style, base),
            Node::Frac { num, den, bar, parens, display } => {
                self.fraction((num, den), *bar, *parens, *display, style, base)
            }
            Node::Sqrt { index, body } => self.sqrt(index.as_deref(), body, style, base),
            Node::Delimited { left, body, right } => {
                let inner = self.list(body, style, base);
                self.fenced(inner, left, right, size)
            }
            Node::Matrix { env, rows, align } => self.matrix(*env, rows, align, style, base),
            Node::Accent { accent, body } => self.accent(*accent, body, style, base),
        }
    }

    /// A symbol's text in its alphabet (the math font's italic, double-struck… letters).
    fn styled(&self, text: &str, font: TexFont) -> (String, bool, bool) {
        let real = self.math_font;
        match font {
            TexFont::Italic if real => {
                (text.chars().map(|c| alphabet(c, 0)).collect(), false, false)
            }
            TexFont::Italic => (text.to_owned(), text.chars().all(char::is_alphabetic), false),
            TexFont::Roman => (text.to_owned(), false, false),
            TexFont::Bold if real => (text.chars().map(|c| alphabet(c, 1)).collect(), false, false),
            TexFont::Bold => (text.to_owned(), false, true),
            TexFont::BoldItalic if real => {
                (text.chars().map(|c| alphabet(c, 2)).collect(), false, false)
            }
            TexFont::BoldItalic => (text.to_owned(), true, true),
            TexFont::DoubleStruck => (text.chars().map(|c| alphabet(c, 3)).collect(), false, false),
            TexFont::Script => (text.chars().map(|c| alphabet(c, 4)).collect(), false, false),
            TexFont::Fraktur => (text.chars().map(|c| alphabet(c, 5)).collect(), false, false),
            TexFont::SansSerif => (text.chars().map(|c| alphabet(c, 6)).collect(), false, false),
            TexFont::Monospace => (text.chars().map(|c| alphabet(c, 7)).collect(), false, false),
        }
    }

    fn scripts(
        &mut self,
        nucleus: &Node,
        sub: Option<&Node>,
        sup: Option<&Node>,
        style: Style,
        base: f32,
    ) -> Box_ {
        let size = style.shrink(base);
        let limits = matches!(nucleus, Node::BigOp { limits: true, .. }) && style == Style::Display;
        let mut b = self.node(nucleus, style, base);
        let script = style.script();
        let sup_box = sup.map(|n| self.node(n, script, base));
        let sub_box = sub.map(|n| self.node(n, script, base));
        if limits {
            // Above and below, centered.
            let w =
                b.w.max(sup_box.as_ref().map_or(0.0, |s| s.w))
                    .max(sub_box.as_ref().map_or(0.0, |s| s.w));
            let mut out = Box_ { w, ..Box_::default() };
            let (asc, desc) = (b.asc, b.desc);
            let bw = b.w;
            out.add(b, (w - bw) / 2.0, 0.0);
            if let Some(s) = sup_box {
                let dy = asc + size * 0.12 + s.desc;
                let sw = s.w;
                out.add(s, (w - sw) / 2.0, dy);
            }
            if let Some(s) = sub_box {
                let dy = -(desc + size * 0.12 + s.asc);
                let sw = s.w;
                out.add(s, (w - sw) / 2.0, dy);
            }
            return out;
        }
        let x = b.w
            + if matches!(nucleus, Node::BigOp { symbol: true, .. }) {
                size * 0.05
            } else {
                size * 0.03
            };
        let mut up = (b.asc - size * 0.3).max(size * 0.38);
        let mut down = (b.desc + size * 0.05).max(size * 0.18);
        if let (Some(p), Some(s)) = (&sup_box, &sub_box) {
            // Apart by a little at least.
            let gap = (up - p.desc) - (s.asc - down);
            if gap < size * 0.2 {
                down += size * 0.2 - gap;
            }
        } else if sub_box.is_some() && sup_box.is_none() {
            up = 0.0;
        }
        let mut w = b.w;
        if let Some(p) = sup_box {
            w = w.max(x + p.w);
            b.add(p, x, up);
        }
        if let Some(s) = sub_box {
            w = w.max(x + s.w);
            b.add(s, x, -down);
        }
        b.w = w + size * 0.04;
        b
    }

    fn big_op(&mut self, text: &str, symbol: bool, style: Style, base: f32) -> Box_ {
        let size = style.shrink(base);
        if !symbol {
            let mut b = self.glyphs(text, size, false, false, self.color);
            b.asc = b.asc.max(size * 0.72);
            return b;
        }
        let integral = text.chars().next().is_some_and(|c| ('\u{222b}'..='\u{2233}').contains(&c));
        let k = match (style, integral) {
            (Style::Display, true) => 2.3,
            (Style::Display, false) => 2.0,
            (_, true) => 1.5,
            _ => 1.35,
        };
        let big = size * k;
        let w = self.width(text, big, false, false, false);
        // Centered on the axis: the glyph's middle (about 0.3 of its size above its
        // baseline for these) on it.
        let lift = axis(size) - big * 0.3;
        let item = Item::Glyph {
            x: 0.0,
            y: lift,
            text: text.to_owned(),
            size: big,
            italic: false,
            bold: false,
            color: self.color,
            ui: false,
        };
        Box_ {
            w,
            asc: axis(size) + big * 0.5,
            desc: (big * 0.56 - axis(size)).max(0.0),
            items: vec![item],
        }
    }

    fn fraction(
        &mut self,
        (num, den): (&Node, &Node),
        bar: bool,
        parens: bool,
        display: Option<bool>,
        style: Style,
        base: f32,
    ) -> Box_ {
        let style = match display {
            Some(true) => Style::Display,
            Some(false) if style == Style::Display => Style::Text,
            _ => style,
        };
        let size = style.shrink(base);
        let inner = style.fraction();
        let n = self.node(num, inner, base);
        let d = self.node(den, inner, base);
        let t = if bar { rule(size) } else { 0.0 };
        let gap = size * if style == Style::Display { 0.2 } else { 0.12 };
        let w = n.w.max(d.w) + size * 0.24;
        let a = axis(size);
        let mut out = Box_ { w, ..Box_::default() };
        let (nw, dw) = (n.w, d.w);
        let num_y = a + t / 2.0 + gap + n.desc;
        let den_y = a - t / 2.0 - gap - d.asc;
        out.add(n, (w - nw) / 2.0, num_y);
        out.add(d, (w - dw) / 2.0, den_y);
        if bar {
            out.items.push(Item::Rule {
                x: size * 0.06,
                y: a - t / 2.0,
                w: w - size * 0.12,
                h: t,
                color: self.color,
            });
        }
        if parens {
            return self.fenced(out, "(", ")", size);
        }
        out
    }

    fn sqrt(&mut self, index: Option<&Node>, body: &Node, style: Style, base: f32) -> Box_ {
        let size = style.shrink(base);
        let b = self.node(body, style, base);
        let t = rule(size);
        let gap = size * 0.12;
        let top = b.asc + gap + t;
        let bottom = b.desc + size * 0.08;
        let surd = size * 0.55;
        let mut out =
            Box_ { w: surd + b.w + size * 0.12, asc: top + t, desc: bottom, items: Vec::new() };
        let p = Point::new;
        // The radical: a tick, down to the bottom, up to the top, along over the body (y
        // down in paths: the box's baseline is 0).
        let segs = vec![
            Seg::Move(p(0.0, -axis(size) * 0.6)),
            Seg::Line(p(surd * 0.22, -axis(size) * 0.9)),
            Seg::Line(p(surd * 0.5, bottom)),
            Seg::Line(p(surd * 0.92, -top + t / 2.0)),
            Seg::Line(p(out.w, -top + t / 2.0)),
        ];
        out.items.push(Item::Path {
            x: 0.0,
            y: 0.0,
            segs,
            width: t,
            color: self.color,
            fill: false,
        });
        let bw = b.w;
        out.add(b, surd, 0.0);
        let _ = bw;
        if let Some(index) = index {
            let i = self.node(index, Style::ScriptScript, base);
            let iw = i.w;
            let dy = axis(size) * 0.9 + i.desc + size * 0.1;
            let shift = (surd * 0.5 - iw).min(0.0);
            if shift < 0.0 {
                out.items = out.items.into_iter().map(|it| it.moved(-shift, 0.0)).collect();
                out.w -= shift;
            }
            out.add(i, (surd * 0.5 - iw).max(0.0), dy);
        }
        out
    }

    /// `inner` between delimiters grown to its height.
    fn fenced(&mut self, inner: Box_, left: &str, right: &str, size: f32) -> Box_ {
        let a = axis(size);
        let half = (inner.asc - a).max(inner.desc + a).max(size * 0.5) + size * 0.1;
        let mut out = Box_::default();
        let lw = self.delimiter(&mut out, left, (0.0, a, half), size, true);
        let iw = inner.w;
        out.add(inner, lw, 0.0);
        let rw = self.delimiter(&mut out, right, (lw + iw, a, half), size, false);
        out.w = lw + iw + rw;
        out.asc = out.asc.max(a + half);
        out.desc = out.desc.max(half - a);
        out
    }

    /// A delimiter drawn at `x`, `half` above and below the axis `a`: its width.
    fn delimiter(
        &mut self,
        out: &mut Box_,
        d: &str,
        (x, a, half): (f32, f32, f32),
        size: f32,
        left: bool,
    ) -> f32 {
        if d.is_empty() || d == "." {
            return size * 0.1;
        }
        let t = rule(size) * 1.15;
        let (top, bottom) = (-(a + half), half - a);
        let mid = -a;
        let p = Point::new;
        let w = match d {
            "{" | "}" => size * 0.5,
            "|" | "‖" => size * 0.3,
            _ => size * 0.38,
        };
        let (x0, x1) = (x + size * 0.08, x + w - size * 0.08);
        let (near, far) = if left { (x1, x0) } else { (x0, x1) };
        let segs = match d {
            "(" | ")" => {
                let k = (far - near) * 1.3;
                vec![
                    Seg::Move(p(near, top)),
                    Seg::Cubic(
                        p(near + k, top + (bottom - top) * 0.25),
                        p(near + k, bottom - (bottom - top) * 0.25),
                        p(near, bottom),
                    ),
                ]
            }
            "[" | "]" | "⌊" | "⌋" | "⌈" | "⌉" => {
                let (has_top, has_bottom) = match d {
                    "⌊" | "⌋" => (false, true),
                    "⌈" | "⌉" => (true, false),
                    _ => (true, true),
                };
                let mut s = Vec::new();
                if has_top {
                    s.push(Seg::Move(p(near, top)));
                    s.push(Seg::Line(p(far, top)));
                } else {
                    s.push(Seg::Move(p(far, top)));
                }
                s.push(Seg::Line(p(far, bottom)));
                if has_bottom {
                    s.push(Seg::Line(p(near, bottom)));
                }
                s
            }
            "{" | "}" => {
                let m = (far + near) / 2.0;
                let q = (bottom - top) / 4.0;
                vec![
                    Seg::Move(p(near, top)),
                    Seg::Cubic(p(m, top), p(m, top), p(m, top + q * 0.5)),
                    Seg::Line(p(m, mid - q * 0.5)),
                    Seg::Cubic(p(m, mid), p(m, mid), p(far, mid)),
                    Seg::Cubic(p(m, mid), p(m, mid), p(m, mid + q * 0.5)),
                    Seg::Line(p(m, bottom - q * 0.5)),
                    Seg::Cubic(p(m, bottom), p(m, bottom), p(near, bottom)),
                ]
            }
            "|" => vec![Seg::Move(p(x + w / 2.0, top)), Seg::Line(p(x + w / 2.0, bottom))],
            "‖" => {
                out.items.push(Item::Path {
                    x: 0.0,
                    y: 0.0,
                    segs: vec![Seg::Move(p(x + w * 0.3, top)), Seg::Line(p(x + w * 0.3, bottom))],
                    width: t * 0.8,
                    color: self.color,
                    fill: false,
                });
                vec![Seg::Move(p(x + w * 0.7, top)), Seg::Line(p(x + w * 0.7, bottom))]
            }
            "⟨" | "⟩" | "<" | ">" => {
                vec![Seg::Move(p(near, top)), Seg::Line(p(far, mid)), Seg::Line(p(near, bottom))]
            }
            other => {
                // A delimiter not drawn: its glyph.
                let g = self.glyphs(other, size, false, false, self.color);
                let gw = g.w;
                out.add(g, x, 0.0);
                return gw;
            }
        };
        out.items.push(Item::Path {
            x: 0.0,
            y: 0.0,
            segs,
            width: if matches!(d, "(" | ")" | "{" | "}") { t * 1.1 } else { t * 0.9 },
            color: self.color,
            fill: false,
        });
        w
    }

    fn matrix(
        &mut self,
        env: Env,
        rows: &[Vec<Vec<Node>>],
        align: &[char],
        style: Style,
        base: f32,
    ) -> Box_ {
        let size = style.shrink(base);
        let cell_style = if env == Env::Aligned {
            style
        } else if style == Style::Display {
            Style::Text
        } else {
            style
        };
        let cells: Vec<Vec<Box_>> = rows
            .iter()
            .map(|r| r.iter().map(|c| self.list(c, cell_style, base)).collect())
            .collect();
        let cols = cells.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0.0f32; cols];
        for row in &cells {
            for (c, b) in row.iter().enumerate() {
                widths[c] = widths[c].max(b.w);
            }
        }
        let gap = |c: usize| -> f32 {
            match env {
                Env::Aligned => {
                    if c % 2 == 1 {
                        size * 0.25
                    } else {
                        size * 1.0
                    }
                }
                Env::Cases => size * 1.0,
                _ => size * 0.8,
            }
        };
        let row_gap = size * 0.3;
        let mut out = Box_::default();
        // Rows from the top: their baselines.
        let heights: Vec<(f32, f32)> = cells
            .iter()
            .map(|r| {
                (
                    r.iter().map(|b| b.asc).fold(size * 0.7, f32::max),
                    r.iter().map(|b| b.desc).fold(size * 0.2, f32::max),
                )
            })
            .collect();
        let total: f32 = heights.iter().map(|(a, d)| a + d).sum::<f32>()
            + row_gap * heights.len().saturating_sub(1) as f32;
        let mut y = total / 2.0 + axis(size);
        for (r, row) in cells.into_iter().enumerate() {
            let (asc, desc) = heights[r];
            y -= asc;
            let mut x = 0.0;
            for (c, b) in row.into_iter().enumerate() {
                let how = match env {
                    Env::Aligned => {
                        if c % 2 == 0 {
                            'r'
                        } else {
                            'l'
                        }
                    }
                    Env::Cases => 'l',
                    Env::Array => align.get(c).copied().unwrap_or('c'),
                    _ => 'c',
                };
                let bw = b.w;
                let dx = match how {
                    'l' => 0.0,
                    'r' => widths[c] - bw,
                    _ => (widths[c] - bw) / 2.0,
                };
                out.add(b, x + dx, y);
                x += widths[c] + if c + 1 < cols { gap(c) } else { 0.0 };
            }
            out.w = out.w.max(x);
            y -= desc + row_gap;
        }
        out.w = widths.iter().sum::<f32>() + (0..cols.saturating_sub(1)).map(gap).sum::<f32>();
        out.asc = out.asc.max(total / 2.0 + axis(size));
        out.desc = out.desc.max(total / 2.0 - axis(size));
        let (l, r) = match env {
            Env::PMatrix => ("(", ")"),
            Env::BMatrix => ("[", "]"),
            Env::BraceMatrix => ("{", "}"),
            Env::VMatrix => ("|", "|"),
            Env::DoubleVMatrix => ("‖", "‖"),
            Env::Cases => ("{", ""),
            _ => ("", ""),
        };
        if l.is_empty() && r.is_empty() {
            return out;
        }
        let padded = Box_ { w: out.w + size * 0.2, ..out };
        let shifted = Box_ {
            items: padded.items.into_iter().map(|i| i.moved(size * 0.1, 0.0)).collect(),
            ..padded
        };
        self.fenced(shifted, l, r, size)
    }

    fn accent(&mut self, accent: Accent, body: &Node, style: Style, base: f32) -> Box_ {
        let size = style.shrink(base);
        let mut b = self.node(body, style, base);
        let t = rule(size);
        let w = b.w;
        let p = Point::new;
        let top = -(b.asc + size * 0.08);
        let narrow = matches!(
            accent,
            Accent::Hat
                | Accent::Tilde
                | Accent::Dot
                | Accent::Ddot
                | Accent::Acute
                | Accent::Grave
                | Accent::Breve
                | Accent::Check
                | Accent::Bar
                | Accent::Vec
        );
        let span = if narrow { (w * 0.8).min(size * 0.5).max(size * 0.3) } else { w };
        let (x0, x1) = (
            (w - span) / 2.0 + if narrow { size * 0.05 } else { 0.0 },
            (w + span) / 2.0 + if narrow { size * 0.05 } else { 0.0 },
        );
        let h = size * 0.16;
        let color = self.color;
        let stroke = |segs: Vec<Seg>, width: f32| Item::Path {
            x: 0.0,
            y: 0.0,
            segs,
            width,
            color,
            fill: false,
        };
        let (item, grow_up, grow_down) = match accent {
            Accent::Hat | Accent::WideHat => (
                stroke(
                    vec![
                        Seg::Move(p(x0, top)),
                        Seg::Line(p((x0 + x1) / 2.0, top - h)),
                        Seg::Line(p(x1, top)),
                    ],
                    t,
                ),
                h + t,
                0.0,
            ),
            Accent::Check => (
                stroke(
                    vec![
                        Seg::Move(p(x0, top - h)),
                        Seg::Line(p((x0 + x1) / 2.0, top)),
                        Seg::Line(p(x1, top - h)),
                    ],
                    t,
                ),
                h + t,
                0.0,
            ),
            Accent::Bar | Accent::Overline => (
                stroke(vec![Seg::Move(p(x0, top - t)), Seg::Line(p(x1, top - t))], t),
                2.0 * t,
                0.0,
            ),
            Accent::Underline => {
                let y = b.desc + size * 0.1;
                (stroke(vec![Seg::Move(p(x0, y)), Seg::Line(p(x1, y))], t), 0.0, size * 0.1 + t)
            }
            Accent::Vec => {
                let y = top - h * 0.6;
                let segs = vec![
                    Seg::Move(p(x0, y)),
                    Seg::Line(p(x1, y)),
                    Seg::Move(p(x1 - h * 0.8, y - h * 0.6)),
                    Seg::Line(p(x1, y)),
                    Seg::Line(p(x1 - h * 0.8, y + h * 0.6)),
                ];
                (stroke(segs, t), h + t, 0.0)
            }
            Accent::Tilde | Accent::WideTilde => {
                let y = top - h * 0.5;
                let q = (x1 - x0) / 3.0;
                (
                    stroke(
                        vec![
                            Seg::Move(p(x0, y + h * 0.3)),
                            Seg::Cubic(
                                p(x0 + q, y - h * 0.9),
                                p(x1 - q, y + h * 0.9),
                                p(x1, y - h * 0.3),
                            ),
                        ],
                        t,
                    ),
                    h + t,
                    0.0,
                )
            }
            Accent::Dot | Accent::Ddot => {
                let r = size * 0.055;
                let c = (x0 + x1) / 2.0;
                let mut segs = scene::ellipse(Rect::new(
                    c - r - if accent == Accent::Ddot { size * 0.1 } else { 0.0 },
                    top - h,
                    2.0 * r,
                    2.0 * r,
                ));
                if accent == Accent::Ddot {
                    segs.extend(scene::ellipse(Rect::new(
                        c - r + size * 0.1,
                        top - h,
                        2.0 * r,
                        2.0 * r,
                    )));
                }
                (Item::Path { x: 0.0, y: 0.0, segs, width: 0.0, color, fill: true }, h + t, 0.0)
            }
            Accent::Acute => (
                stroke(vec![Seg::Move(p(x0 + span * 0.3, top)), Seg::Line(p(x1, top - h))], t),
                h + t,
                0.0,
            ),
            Accent::Grave => (
                stroke(vec![Seg::Move(p(x0, top - h)), Seg::Line(p(x1 - span * 0.3, top))], t),
                h + t,
                0.0,
            ),
            Accent::Breve => (
                stroke(
                    vec![
                        Seg::Move(p(x0, top - h)),
                        Seg::Cubic(p(x0, top), p(x1, top), p(x1, top - h)),
                    ],
                    t,
                ),
                h + t,
                0.0,
            ),
            Accent::OverBrace | Accent::UnderBrace => {
                let over = accent == Accent::OverBrace;
                let y0 = if over { top } else { b.desc + size * 0.1 };
                let dir = if over { -1.0 } else { 1.0 };
                let m = (x0 + x1) / 2.0;
                let hh = size * 0.25 * dir;
                let segs = vec![
                    Seg::Move(p(x0, y0)),
                    Seg::Cubic(
                        p(x0, y0 + hh * 0.6),
                        p(x0 + size * 0.1, y0 + hh * 0.5),
                        p(x0 + size * 0.3, y0 + hh * 0.5),
                    ),
                    Seg::Line(p(m - size * 0.25, y0 + hh * 0.5)),
                    Seg::Cubic(
                        p(m - size * 0.05, y0 + hh * 0.5),
                        p(m, y0 + hh * 0.7),
                        p(m, y0 + hh),
                    ),
                    Seg::Cubic(
                        p(m, y0 + hh * 0.7),
                        p(m + size * 0.05, y0 + hh * 0.5),
                        p(m + size * 0.25, y0 + hh * 0.5),
                    ),
                    Seg::Line(p(x1 - size * 0.3, y0 + hh * 0.5)),
                    Seg::Cubic(p(x1 - size * 0.1, y0 + hh * 0.5), p(x1, y0 + hh * 0.6), p(x1, y0)),
                ];
                let grow = size * 0.3;
                (
                    stroke(segs, t),
                    if over { grow } else { 0.0 },
                    if over { 0.0 } else { grow + size * 0.1 },
                )
            }
        };
        b.items.push(item);
        b.asc += grow_up + size * 0.06;
        b.desc += grow_down;
        b
    }

    /// An item into the scene (`ascent`: where the baseline is from the top).
    fn emit(&mut self, scene: &mut Scene, item: Item, _k: f32, ascent: f32) {
        match item {
            Item::Glyph { x, y, text, size, italic, bold, color, ui } => {
                let font = self.font(size, italic, bold, ui);
                let m = self.text.metrics(&font);
                let (a, d) = (m.ascent / 4.0, m.descent / 4.0);
                let baseline = ascent - y;
                scene.text(Text {
                    at: Point::new(x + 1.0, baseline - (a - d) / 2.0),
                    text,
                    size,
                    bold,
                    italic,
                    mono: false,
                    math: !ui,
                    color,
                    anchor: Anchor::Start,
                });
            }
            Item::Rule { x, y, w, h, color } => {
                let r = Rect::new(x + 1.0, ascent - y - h, w, h);
                scene.rect(r, 0.0, Some(color), None);
            }
            Item::Path { x, y, segs, width, color, fill } => {
                let dx = x + 1.0;
                let dy = ascent - y;
                let m = |p: Point| Point::new(p.x + dx, p.y + dy);
                let segs = segs
                    .into_iter()
                    .map(|s| match s {
                        Seg::Move(p) => Seg::Move(m(p)),
                        Seg::Line(p) => Seg::Line(m(p)),
                        Seg::Cubic(a, b, c) => Seg::Cubic(m(a), m(b), m(c)),
                        Seg::Close => Seg::Close,
                    })
                    .collect();
                if fill {
                    scene.path(segs, Some(color), None);
                } else {
                    scene.path(segs, None, Some(Line::solid(color, width)));
                }
            }
        }
    }
}

fn class_of(node: &Node) -> Class {
    match node {
        Node::Symbol { class, .. } => *class,
        Node::BigOp { .. } => Class::Op,
        Node::Scripts { base, .. } => class_of(base),
        Node::Delimited { .. } | Node::Frac { .. } | Node::Matrix { .. } => Class::Ord,
        Node::Colored { body, .. } => class_of(body),
        _ => Class::Ord,
    }
}

/// The space between two atoms (ems), by their classes (TeX's table, simplified; less in
/// scripts).
fn spacing(left: Class, right: Class, style: Style) -> f32 {
    let (thin, medium, thick) = (3.0 / 18.0, 4.0 / 18.0, 5.0 / 18.0);
    use Class::*;
    let s = match (left, right) {
        (Op, Ord | Op) | (Ord, Op) | (Close, Op) => thin,
        (Bin, _) | (_, Bin) => medium,
        (Rel, Rel) => 0.0,
        (Rel, _) | (_, Rel) => thick,
        (Punct, _) => thin,
        _ => 0.0,
    };
    if style.tight() && !matches!((left, right), (Op, _) | (_, Op)) { 0.0 } else { s }
}

/// A letter or digit in one of the Mathematical Alphanumeric Symbols' alphabets: 0
/// italic, 1 bold, 2 bold italic, 3 double-struck, 4 script, 5 fraktur, 6 sans-serif, 7
/// monospace (others as they are).
fn alphabet(c: char, which: usize) -> char {
    // The letters Unicode had before (holes in the blocks).
    let special = match (which, c) {
        (0, 'h') => Some('ℎ'),
        (3, 'C') => Some('ℂ'),
        (3, 'H') => Some('ℍ'),
        (3, 'N') => Some('ℕ'),
        (3, 'P') => Some('ℙ'),
        (3, 'Q') => Some('ℚ'),
        (3, 'R') => Some('ℝ'),
        (3, 'Z') => Some('ℤ'),
        (4, 'B') => Some('ℬ'),
        (4, 'E') => Some('ℰ'),
        (4, 'F') => Some('ℱ'),
        (4, 'H') => Some('ℋ'),
        (4, 'I') => Some('ℐ'),
        (4, 'L') => Some('ℒ'),
        (4, 'M') => Some('ℳ'),
        (4, 'R') => Some('ℛ'),
        (4, 'e') => Some('ℯ'),
        (4, 'g') => Some('ℊ'),
        (4, 'o') => Some('ℴ'),
        (5, 'C') => Some('ℭ'),
        (5, 'H') => Some('ℌ'),
        (5, 'I') => Some('ℑ'),
        (5, 'R') => Some('ℜ'),
        (5, 'Z') => Some('ℨ'),
        _ => None,
    };
    if let Some(s) = special {
        return s;
    }
    // Greek in italics (and bold italics).
    if let ('\u{3b1}'..='\u{3c9}', 0 | 2) = (c, which) {
        let base = if which == 0 { 0x1D6FC } else { 0x1D736 };
        return char::from_u32(base + (c as u32 - 0x3B1)).unwrap_or(c);
    }
    if let ('\u{391}'..='\u{3a9}', 0 | 2) = (c, which) {
        let base = if which == 0 { 0x1D6E2 } else { 0x1D71C };
        return char::from_u32(base + (c as u32 - 0x391)).unwrap_or(c);
    }
    let (upper, lower, digits): (u32, u32, Option<u32>) = match which {
        0 => (0x1D434, 0x1D44E, None),
        1 => (0x1D400, 0x1D41A, Some(0x1D7CE)),
        2 => (0x1D468, 0x1D482, None),
        3 => (0x1D538, 0x1D552, Some(0x1D7D8)),
        4 => (0x1D49C, 0x1D4B6, None),
        5 => (0x1D504, 0x1D51E, None),
        6 => (0x1D5A0, 0x1D5BA, Some(0x1D7E2)),
        _ => (0x1D670, 0x1D68A, Some(0x1D7F6)),
    };
    let code = match c {
        'A'..='Z' => Some(upper + (c as u32 - 'A' as u32)),
        'a'..='z' => Some(lower + (c as u32 - 'a' as u32)),
        '0'..='9' => digits.map(|d| d + (c as u32 - '0' as u32)),
        _ => None,
    };
    code.and_then(char::from_u32).unwrap_or(c)
}

/// A math alphanumeric's plain letter (`𝑔` → `g`), for its depth.
fn plain(c: char) -> char {
    let code = c as u32;
    for (upper, lower) in [(0x1D434u32, 0x1D44Eu32), (0x1D400, 0x1D41A), (0x1D468, 0x1D482)] {
        if (lower..lower + 26).contains(&code) {
            return char::from_u32('a' as u32 + code - lower).unwrap_or(c);
        }
        if (upper..upper + 26).contains(&code) {
            return char::from_u32('A' as u32 + code - upper).unwrap_or(c);
        }
    }
    c
}

#[cfg(test)]
#[path = "../tests/unit/math.rs"]
mod tests;
