//! Mermaid diagrams drawn natively: the block parsed (`diagram-text`), laid out by type
//! with the UI's font measured, turned into a [`Scene`] whose shapes are rasterized at the
//! screen's scale — all on the module's render pool, cached by the block's hash. The
//! colors come from the theme (a [`Look`]); what the diagram styles itself keeps its own.

pub mod cards;
pub mod charts;
pub mod class;
pub mod er;
pub mod flowchart;
pub mod git;
pub mod graph;
pub mod mindmap;
pub mod plots;
pub mod scene;
pub mod sequence;
pub mod state;

use std::cell::RefCell;
use std::sync::Arc;

use diagram_text::mermaid::{self, Diagram, Error, ErrorKind};
use kernel::{Point, Rect, Rgba};
use text_layout::{Font, FontFamily, TextSystem};
use ui_components::Palette;

pub use scene::Scene;

/// The colors and fonts a diagram is drawn with (from the theme), sent to the worker.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub background: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub fill: Rgba,
    pub stroke: Rgba,
    pub line: Rgba,
    pub accent: Rgba,
    pub group: Rgba,
    pub group_stroke: Rgba,
    pub note: Rgba,
    pub note_stroke: Rgba,
    /// The series' colors (a pie's slices, a gantt's sections, git's branches).
    pub series: [Rgba; 8],
    pub font: FontFamily,
    pub mono: FontFamily,
    /// The text's size (diagram pixels).
    pub size: f32,
}

impl Look {
    pub fn of(p: &Palette) -> Self {
        let ansi = &p.terminal_ansi;
        let series = [ansi[4], ansi[2], ansi[5], ansi[3], ansi[6], ansi[1], ansi[12], ansi[10]];
        Self {
            background: p.editor_background,
            text: p.text,
            muted: p.text_muted,
            fill: p.editor_background.mix(p.accent, 0.14),
            stroke: p.accent.mix(p.text, 0.2),
            line: p.text_muted.mix(p.text, 0.25),
            accent: p.accent,
            group: p.text.with_alpha(0.04),
            group_stroke: p.border.mix(p.text_muted, 0.3),
            note: p.editor_background.mix(ansi[3], 0.18),
            note_stroke: ansi[3].mix(p.editor_background, 0.35),
            series,
            font: p.ui_font.clone(),
            mono: p.code_font.clone(),
            size: (p.ui_font_size + 1.0).round(),
        }
    }

    /// What it's cached by.
    pub fn key(&self) -> String {
        format!("{self:?}")
    }
}

/// What measures text for the layout (the UI's fonts).
pub trait Measure {
    /// The width of `text` (one line) at `size`.
    fn width(&mut self, text: &str, size: f32, bold: bool) -> f32;
}

/// Measures with a text system (the worker's own: same fonts, same widths as painting).
pub struct Measurer<'a> {
    pub text: &'a mut TextSystem,
    pub family: FontFamily,
}

impl Measure for Measurer<'_> {
    fn width(&mut self, text: &str, size: f32, bold: bool) -> f32 {
        // Measured at 4× and scaled down: closer to the widths at any zoom.
        let weight = if bold { Font::SEMIBOLD } else { Font::NORMAL };
        let font = Font::new(self.family.clone(), size * 4.0).with_weight(weight);
        text.lines().map(|l| self.text.shape_line(l, &font).width / 4.0).fold(0.0, f32::max)
    }
}

/// Why a block isn't drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The text says something wrong (its line in the block).
    Error(Error),
}

/// A diagram laid out: its scene (its shapes are rasterized in tiles where it's shown,
/// [`crate::tiles`]) and its title.
#[derive(Clone)]
pub struct Rendered {
    pub scene: Arc<Scene>,
    pub title: Option<String>,
}

/// What a block's job hands back.
pub type Outcome = Result<Rendered, Failure>;

thread_local! {
    /// The worker's text system (measuring only), made on its first diagram.
    static TEXT: RefCell<Option<TextSystem>> = const { RefCell::new(None) };
}

/// Lays out `source` (on the worker): its scene.
pub fn render(source: &str, look: &Look) -> Outcome {
    let parsed = mermaid::parse(source).map_err(Failure::Error)?;
    let scene = TEXT.with(|cell| {
        let mut slot = cell.borrow_mut();
        let text = slot.get_or_insert_with(TextSystem::new);
        let mut measure = Measurer { text, family: look.font.clone() };
        let mut scene = layout(&parsed.diagram, look, &mut measure);
        if let Some(title) = &parsed.meta.title {
            titled(&mut scene, title, look, &mut measure);
        }
        scene
    });
    Ok(Rendered { scene: Arc::new(scene), title: parsed.meta.title })
}

/// The whole diagram (its texts too) as a picture at scale `k` on the theme's background
/// (the clipboard, a PNG file): drawn on the worker with its own text system.
pub fn picture(scene: &Scene, look: &Look, k: f32) -> Option<(u32, u32, Vec<u8>, Vec<u8>)> {
    TEXT.with(|cell| {
        let mut slot = cell.borrow_mut();
        let text = slot.get_or_insert_with(TextSystem::new);
        let mut painter = Glyphs { text, look };
        scene::picture(scene, k, look.background, &mut painter)
    })
}

/// The worker's text system as a [`scene::GlyphPainter`].
struct Glyphs<'a> {
    text: &'a mut TextSystem,
    look: &'a Look,
}

impl scene::GlyphPainter for Glyphs<'_> {
    fn glyphs(&mut self, t: &scene::Text, k: f32) -> scene::ShapedText {
        use render_api::{GlyphFormat, GlyphRasterizer};
        let family = if t.mono { self.look.mono.clone() } else { self.look.font.clone() };
        let weight = if t.bold { Font::SEMIBOLD } else { Font::NORMAL };
        let font =
            Font::new(family, (t.size * k).round()).with_weight(weight).with_italic(t.italic);
        let metrics = self.text.metrics(&font);
        let shaped = self.text.shape_line(&t.text, &font);
        let glyphs = shaped
            .glyphs
            .iter()
            .filter_map(|g| {
                let r = self.text.rasterize(g.key)?;
                let color = r.format == GlyphFormat::Color;
                Some((
                    (g.x.round() as i32) + r.left,
                    (g.y.round() as i32) - r.top,
                    r.width,
                    r.height,
                    r.data,
                    color,
                ))
            })
            .collect();
        scene::ShapedText {
            width: shaped.width,
            ascent: metrics.ascent,
            descent: metrics.descent,
            glyphs,
        }
    }
}

/// A diagram's scene, by its type.
pub fn layout(diagram: &Diagram, look: &Look, measure: &mut dyn Measure) -> Scene {
    let mut scene = match diagram {
        Diagram::Flowchart(f) => flowchart::layout(f, look, measure),
        Diagram::Sequence(s) => sequence::layout(s, look, measure),
        Diagram::Class(c) => class::layout(c, look, measure),
        Diagram::State(s) => state::layout(s, look, measure),
        Diagram::Er(e) => er::layout(e, look, measure),
        Diagram::Gantt(g) => charts::gantt(g, look, measure),
        Diagram::Pie(p) => charts::pie(p, look, measure),
        Diagram::Timeline(t) => charts::timeline(t, look, measure),
        Diagram::Mindmap(m) => mindmap::layout(m, look, measure),
        Diagram::Git(g) => git::layout(g, look, measure),
        Diagram::Journey(j) => plots::journey(j, look, measure),
        Diagram::Sankey(s) => plots::sankey(s, look, measure),
        Diagram::XyChart(c) => plots::xychart(c, look, measure),
        Diagram::Quadrant(q) => plots::quadrant(q, look, measure),
        Diagram::Radar(r) => plots::radar(r, look, measure),
        Diagram::Packet(p) => plots::packet(p, look, measure),
        Diagram::Treemap(t) => plots::treemap(t, look, measure),
        Diagram::Requirement(b) => cards::boxes(b, cards::Kind::Requirement, look, measure),
        Diagram::C4(b) => cards::boxes(b, cards::Kind::C4, look, measure),
        Diagram::Architecture(b) => cards::boxes(b, cards::Kind::Architecture, look, measure),
        Diagram::Block(b) => cards::block(b, look, measure),
        Diagram::Kanban(k) => cards::kanban(k, look, measure),
    };
    pad(&mut scene, 8.0);
    scene
}

/// The diagram's title over it, centered.
fn titled(scene: &mut Scene, title: &str, look: &Look, measure: &mut dyn Measure) {
    let size = look.size * 1.2;
    let (w, h) = text_size(measure, title, size, true);
    let room = h + 12.0;
    scene.shift(0.0, room);
    scene.height += room;
    if w > scene.width {
        scene.shift((w - scene.width) / 2.0, 0.0);
        scene.width = w;
    }
    scene.text(scene::Text {
        at: Point::new(scene.width / 2.0, h / 2.0 + 2.0),
        text: title.to_owned(),
        size,
        bold: true,
        italic: false,
        mono: false,
        math: false,
        color: look.text,
        anchor: scene::Anchor::Middle,
    });
}

/// Room around the scene (strokes and heads aren't cut at its edges).
fn pad(scene: &mut Scene, by: f32) {
    scene.shift(by, by);
    scene.width += 2.0 * by;
    scene.height += 2.0 * by;
}

/// A label's lines and its size (width, height) at `size`.
pub fn text_size(measure: &mut dyn Measure, text: &str, size: f32, bold: bool) -> (f32, f32) {
    let lines = text.lines().count().max(1) as f32;
    (measure.width(text, size, bold), lines * size * 1.3)
}

/// A label's lines as texts centered on `at`.
pub fn label_texts(scene: &mut Scene, text: &str, at: Point, size: f32, color: Rgba, bold: bool) {
    let lines: Vec<&str> = text.lines().collect();
    let line_h = size * 1.3;
    let top = at.y - line_h * lines.len() as f32 / 2.0 + line_h / 2.0;
    for (i, line) in lines.iter().enumerate() {
        scene.text(scene::Text {
            at: Point::new(at.x, top + i as f32 * line_h),
            text: (*line).to_owned(),
            size,
            bold,
            italic: false,
            mono: false,
            math: false,
            color,
            anchor: scene::Anchor::Middle,
        });
    }
}

/// `r` grown by `by` on every side.
pub fn grow(r: Rect, by: f32) -> Rect {
    Rect::new(r.min_x() - by, r.min_y() - by, r.size.width + 2.0 * by, r.size.height + 2.0 * by)
}

/// The text an error shows (its line, what's wrong), in the user's language.
pub fn describe(error: &Error) -> String {
    let what = match &error.kind {
        ErrorKind::Empty => i18n::t("markdown.mermaid_empty"),
        ErrorKind::Unsupported(kind) => {
            i18n::tr!("markdown.mermaid_unsupported", kind = kind.as_str())
        }
        ErrorKind::UnknownType(kind) => i18n::tr!("markdown.mermaid_unknown", kind = kind.as_str()),
        ErrorKind::Unexpected(text) => {
            i18n::tr!("markdown.mermaid_unexpected", text = text.as_str())
        }
        ErrorKind::Unclosed(closer) => i18n::tr!("markdown.mermaid_unclosed", closer = *closer),
    };
    if error.line == 0 {
        what
    } else {
        i18n::tr!("markdown.mermaid_at_line", line = error.line, what = what.as_str())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mermaid/mod.rs"]
pub(crate) mod tests;
