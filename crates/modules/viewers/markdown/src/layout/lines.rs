//! Text set in lines: spans cut into words and images, broken between the margins (a word
//! too long cut where it stops fitting), each word drawn with its decorations; formulas,
//! rules and a run's measure.

use std::sync::Arc;

use kernel::{Point, Rect, Rgba};
use text_layout::{Font, ShapedLine};

use crate::select::{Sep, TextRun};

use super::{Align, Builder, Hit, Item, Run, Span, Style};

/// A piece of a line before it's placed: a word (with the space after it) or an image.
enum Atom {
    Word {
        text: String,
        font: Font,
        width: f32,
        trimmed: f32,
        span: usize,
    },
    Image {
        width: f32,
        height: f32,
        span: usize,
    },
    /// A formula: its width, and how far it reaches above and below the baseline.
    Math {
        width: f32,
        ascent: f32,
        descent: f32,
        span: usize,
    },
    Break,
}

/// An atom placed on a line: its text (trimmed at the line's start) and where it starts.
struct Placed {
    atom: usize,
    text: Option<String>,
    x: f32,
    width: f32,
    trimmed: f32,
}

impl Builder<'_> {
    pub fn px(&self, v: f32) -> f32 {
        (v * self.s).round()
    }

    pub fn font(&self, style: Style, size: f32, weight: u16) -> Font {
        let size = if style.sup || style.sub { (size * 0.75).round() } else { size };
        if style.code || style.kbd {
            return Font::new(self.palette.code_font.clone(), (size * 0.88).round());
        }
        let weight = if style.bold { weight.max(Font::BOLD) } else { weight };
        Font::new(self.palette.ui_font.clone(), size).with_weight(weight).with_italic(style.italic)
    }

    /// A run of text for selecting, in the current block (`shaped`: its glyphs; `None`: a
    /// whole, copied as `copy`).
    pub fn run(
        &mut self,
        rect: Rect,
        text: String,
        shaped: Option<Arc<ShapedLine>>,
        copy: String,
        sep: Sep,
    ) {
        let start = self.runs.last().map_or(0, TextRun::end);
        let (block, src) = (self.blocks, self.src.clone());
        self.runs.push(TextRun { rect, text, shaped, copy, sep, block, scroll: None, start, src });
    }

    /// The separator the next run takes: the one asked for, or `default`.
    pub fn take_sep(&mut self, default: Sep) -> Sep {
        self.sep.take().unwrap_or(default)
    }

    /// A whole selected as one (a formula, a diagram) in a block of its own at `rect`.
    pub fn whole(&mut self, rect: Rect, copy: String) {
        self.blocks += 1;
        let sep = self.take_sep(Sep::Paragraph);
        self.run(rect, String::new(), None, copy, sep);
    }

    /// Lays out `spans` in lines between `x0` and `x1`, starting at `self.y`; advances
    /// `self.y` below the last line. A line holding an image is as tall as it. Each word is
    /// a run of the selectable text, the spans a block of it.
    pub fn flow(&mut self, spans: &[Span], run: Run) {
        let Run { x0, x1, size, weight, color, line, align } = run;
        let base = Font::new(self.palette.ui_font.clone(), size);
        let metrics = self.text.metrics(&base);
        let line_h = (size * line).round();
        let baseline_in_line =
            ((line_h - (metrics.ascent + metrics.descent)) / 2.0 + metrics.ascent).round();

        let atoms = self.atoms(spans, size, weight);
        let (lines, hard) = self.break_lines(&atoms, x0, x1);

        self.blocks += 1;
        let asked = self.sep.take();
        let mut sep = asked.unwrap_or(Sep::Paragraph);
        let first_run = self.runs.len();
        let mut top = self.y;
        for (n, placed) in lines.iter().enumerate() {
            if n > 0 && hard[n] && sep == Sep::None {
                sep = Sep::Line;
            }
            let tallest = placed
                .iter()
                .filter_map(|p| match atoms[p.atom] {
                    Atom::Image { height, .. } => Some(height),
                    _ => None,
                })
                .fold(0.0, f32::max);
            let image_h = if tallest > 0.0 { line_h.max(tallest + self.px(6.0)) } else { line_h };
            // Formulas sit on the baseline: the line grows above and below for them.
            let mut above = image_h - line_h + baseline_in_line;
            let mut below = line_h - baseline_in_line;
            for p in placed {
                if let Atom::Math { ascent, descent, .. } = atoms[p.atom] {
                    above = above.max(ascent + self.px(2.0));
                    below = below.max(descent + self.px(2.0));
                }
            }
            let height = (above + below).round();
            let end = placed.last().map_or(x0, |p| p.x + p.trimmed);
            let shift = match align {
                Align::Left => 0.0,
                Align::Center => ((x1 - end) / 2.0).max(0.0).round(),
                Align::Right => (x1 - end).max(0.0).round(),
            };
            let baseline = (top + above).round();
            for p in placed {
                let x = p.x + shift;
                match &atoms[p.atom] {
                    Atom::Word { font, span, .. } => {
                        let text = p.text.clone().unwrap_or_default();
                        self.word(&spans[*span], &text, font, (x, baseline, top, line_h), color, p);
                        let shaped = self.text.shape_line(&text, font);
                        let rect = Rect::new(x, top, p.width, height);
                        self.run(rect, text, Some(shaped), String::new(), sep);
                        sep = Sep::None;
                    }
                    Atom::Image { width, height, span } => {
                        let span = &spans[*span];
                        let Some(image) = &span.image else { continue };
                        let y = (baseline + metrics.descent - height).max(top + self.px(3.0));
                        let rect = Rect::new(x, y.round(), *width, *height);
                        let (path, stamp, decode) = (image.path.clone(), image.stamp, image.decode);
                        self.items.push(Item::Picture { rect, path, stamp, decode });
                        if let Some(url) = &span.link {
                            self.hits.push((rect, Hit::Link(url.clone())));
                        }
                    }
                    Atom::Math { ascent, span, .. } => {
                        let span = &spans[*span];
                        let Some(formula) = &span.math else { continue };
                        self.formula(formula, Point::new(x, baseline - ascent));
                        let rect = Rect::new(x, top, p.width, height);
                        self.run(rect, String::new(), None, span.text.clone(), sep);
                        sep = Sep::None;
                    }
                    Atom::Break => {}
                }
            }
            top += height;
        }
        if lines.is_empty() && !spans.is_empty() {
            top += line_h;
        }
        // Nothing to select where a separator was asked for (an empty cell): an empty run
        // keeps it.
        if self.runs.len() == first_run && asked.is_some() {
            let rect = Rect::new(x0, self.y, 0.0, line_h);
            let empty = Arc::new(ShapedLine::default());
            self.run(rect, String::new(), Some(empty), String::new(), sep);
        }
        self.y = top;
    }

    /// One word's text and decorations, at `x` on the line (`baseline`, `top`, `line_h`).
    fn word(
        &mut self,
        span: &Span,
        word: &str,
        font: &Font,
        (x, baseline, top, line_h): (f32, f32, f32, f32),
        color: Rgba,
        placed: &Placed,
    ) {
        let style = span.style;
        let (width, trimmed) = (placed.width, placed.trimmed);
        let mut color = color;
        let mut baseline = baseline;
        if style.sup {
            baseline -= (font.size * 0.55).round();
        } else if style.sub {
            baseline += (font.size * 0.3).round();
        }
        if style.link {
            color = self.palette.accent;
        }
        let hairline = self.px(1.0).max(1.0);
        if style.mark {
            let rect = Rect::new(x, top + self.px(2.0), width, line_h - self.px(4.0));
            let mark = self.palette.terminal_ansi[3].with_alpha(0.32);
            self.items.push(Item::Rect { rect, color: mark, radius: self.px(2.0) });
        }
        if style.code || style.kbd {
            let pad = self.px(3.0);
            // A space inside the code is covered too (one box for `two words`).
            let covered = if word.ends_with(' ') && style.code { width } else { trimmed };
            let rect = Rect::new(
                x - pad / 2.0,
                top + self.px(3.0),
                covered.min(width) + pad,
                line_h - self.px(6.0),
            );
            let radius = self.px(4.0);
            self.items.push(Item::Rect {
                rect,
                color: self.palette.text.with_alpha(if style.kbd { 0.04 } else { 0.07 }),
                radius,
            });
            if style.kbd {
                let border = self.palette.border;
                self.items.push(Item::Border { rect, color: border, width: hairline, radius });
                // The key's lower edge, a little thicker.
                let bottom = Rect::new(
                    rect.min_x() + radius,
                    rect.max_y() - hairline,
                    rect.size.width - 2.0 * radius,
                    hairline,
                );
                self.items.push(Item::Rect { rect: bottom, color: border, radius: 0.0 });
            }
            color = self.palette.code_text;
        }
        if style.strike {
            let metrics = self.text.metrics(font);
            let y = (baseline - metrics.ascent * 0.32).round();
            self.items.push(Item::Rect {
                rect: Rect::new(x, y, trimmed, hairline),
                color,
                radius: 0.0,
            });
        }
        if style.link || style.underline {
            let y = baseline + self.px(2.0);
            let alpha = if style.link { 0.45 } else { 0.9 };
            self.items.push(Item::Rect {
                rect: Rect::new(x, y, trimmed, hairline),
                color: color.with_alpha(alpha),
                radius: 0.0,
            });
        }
        if let Some(url) = &span.link {
            self.hits.push((Rect::new(x, top, width, line_h), Hit::Link(url.clone())));
        }
        self.items.push(Item::Text {
            x,
            baseline,
            text: word.to_owned(),
            font: font.clone(),
            color,
        });
    }

    /// The spans cut into words (with the space after each) and images.
    fn atoms(&mut self, spans: &[Span], size: f32, weight: u16) -> Vec<Atom> {
        let mut atoms = Vec::new();
        for (i, span) in spans.iter().enumerate() {
            if let Some(image) = &span.image {
                atoms.push(Atom::Image { width: image.width, height: image.height, span: i });
                continue;
            }
            if let Some(f) = &span.math {
                let s = self.s;
                atoms.push(Atom::Math {
                    width: (f.scene.width * s).round(),
                    ascent: f.ascent * s,
                    descent: f.descent * s,
                    span: i,
                });
                continue;
            }
            let font = self.font(span.style, size, weight);
            for (n, part) in span.text.split('\n').enumerate() {
                if n > 0 {
                    atoms.push(Atom::Break);
                }
                for word in part.split_inclusive(' ') {
                    let width = self.text.shape_line(word, &font).width;
                    let trimmed = self.text.shape_line(word.trim_end(), &font).width;
                    let text = word.to_owned();
                    atoms.push(Atom::Word { text, font: font.clone(), width, trimmed, span: i });
                }
            }
        }
        atoms
    }

    /// The atoms placed in lines between `x0` and `x1`: a word that doesn't fit goes to
    /// the next line, one wider than the whole line is cut where it must. With each line,
    /// whether a break started it (not the width).
    fn break_lines(&mut self, atoms: &[Atom], x0: f32, x1: f32) -> (Vec<Vec<Placed>>, Vec<bool>) {
        let mut lines: Vec<Vec<Placed>> = Vec::new();
        let mut hard = vec![false];
        let mut line: Vec<Placed> = Vec::new();
        let mut x = x0;
        let mut any = false;
        for (i, atom) in atoms.iter().enumerate() {
            match atom {
                Atom::Break => {
                    lines.push(std::mem::take(&mut line));
                    hard.push(true);
                    x = x0;
                }
                Atom::Image { width, .. } | Atom::Math { width, .. } => {
                    any = true;
                    if x > x0 && x + width > x1 {
                        lines.push(std::mem::take(&mut line));
                        hard.push(false);
                        x = x0;
                    }
                    line.push(Placed { atom: i, text: None, x, width: *width, trimmed: *width });
                    x += width;
                }
                Atom::Word { text, font, width, trimmed, .. } => {
                    if x > x0 && x + trimmed > x1 {
                        lines.push(std::mem::take(&mut line));
                        hard.push(false);
                        x = x0;
                    }
                    let (mut word, mut width, mut trimmed) = (text.clone(), *width, *trimmed);
                    if x == x0 {
                        let start = word.trim_start();
                        if start.is_empty() {
                            continue;
                        }
                        if start.len() != word.len() {
                            word = start.to_owned();
                            width = self.text.shape_line(&word, font).width;
                            trimmed = self.text.shape_line(word.trim_end(), font).width;
                        }
                    }
                    any = true;
                    // Longer than the whole line (a long URL): cut by characters.
                    while x == x0 && trimmed > x1 - x0 {
                        let (head, rest) = self.cut(&word, font, x1 - x0);
                        if rest.is_empty() {
                            break;
                        }
                        let w = self.text.shape_line(&head, font).width;
                        line.push(Placed { atom: i, text: Some(head), x, width: w, trimmed: w });
                        lines.push(std::mem::take(&mut line));
                        hard.push(false);
                        word = rest;
                        width = self.text.shape_line(&word, font).width;
                        trimmed = self.text.shape_line(word.trim_end(), font).width;
                    }
                    line.push(Placed { atom: i, text: Some(word), x, width, trimmed });
                    x += width;
                }
            }
        }
        if !line.is_empty() || (any && lines.is_empty()) {
            lines.push(line);
        }
        hard.resize(lines.len(), false);
        (lines, hard)
    }

    /// `word` cut where it stops fitting in `width` (at least one character).
    fn cut(&mut self, word: &str, font: &Font, width: f32) -> (String, String) {
        let shaped = self.text.shape_line(word, font);
        let at = shaped.byte_at(width, word.len());
        let at = word
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= at)
            .last()
            .filter(|&i| i > 0)
            .unwrap_or_else(|| word.chars().next().map_or(word.len(), char::len_utf8));
        (word[..at].to_owned(), word[at..].to_owned())
    }

    /// A formula at `at` (its top left): its masks (glyphs and shapes).
    pub fn formula(&mut self, f: &crate::math::Formula, at: Point) {
        let at = Point::new(at.x.round(), at.y.round());
        for (image, r) in &f.masks {
            let rect = Rect::new(at.x + r.min_x(), at.y + r.min_y(), r.size.width, r.size.height);
            self.items.push(Item::Image { rect, image: image.clone() });
        }
    }

    pub fn rule(&mut self, x0: f32, x1: f32) {
        let h = self.px(1.0).max(1.0);
        self.items.push(Item::Rect {
            rect: Rect::new(x0, self.y, x1 - x0, h),
            color: self.palette.border,
            radius: 0.0,
        });
        self.y += h;
    }

    /// The width `spans` take on one line, and their widest word (a table's column).
    pub fn measure(&mut self, spans: &[Span], size: f32, weight: u16) -> (f32, f32) {
        let mut total: f32 = 0.0;
        let mut widest: f32 = 0.0;
        let mut line: f32 = 0.0;
        for atom in self.atoms(spans, size, weight) {
            match atom {
                Atom::Word { width, trimmed, .. } => {
                    widest = widest.max(trimmed);
                    line += width;
                }
                Atom::Image { width, .. } | Atom::Math { width, .. } => {
                    widest = widest.max(width);
                    line += width;
                }
                Atom::Break => {
                    total = total.max(line);
                    line = 0.0;
                }
            }
        }
        (total.max(line), widest)
    }
}
