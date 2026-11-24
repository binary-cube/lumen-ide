//! Document layout: from Markdown events, a list of positioned elements (text,
//! rectangles, images, icons) and what can be clicked, relative to the document corner.
//! Text is wrapped to the available width, word by word; the walk over the parser's
//! events is in [`crate::walk`].

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::{Rect, Rgba};
use render_api::ImageData;
use text_layout::{Font, TextSystem};
use ui_components::Palette;

use crate::select::{Sep, TextRun};

mod images;
mod lines;

pub use images::{DECODE_THREAD, ImageCache, ImageSizes};
pub(crate) use images::{Probe, Stamp, decode_width};

/// A drawn element (coordinates relative to the document).
#[derive(Clone)]
pub enum Item {
    Text {
        x: f32,
        baseline: f32,
        text: String,
        font: Font,
        color: Rgba,
    },
    Rect {
        rect: Rect,
        color: Rgba,
        radius: f32,
    },
    Border {
        rect: Rect,
        color: Rgba,
        width: f32,
        radius: f32,
    },
    Image {
        rect: Rect,
        image: ImageData,
    },
    /// A document's image: its file (as of `stamp`), decoded `decode` pixels wide when it's
    /// about to be shown.
    Picture {
        rect: Rect,
        path: PathBuf,
        stamp: Stamp,
        decode: u32,
    },
    /// An icon of the app's set (`info`, `check`), tinted.
    Icon {
        rect: Rect,
        name: &'static str,
        color: Rgba,
    },
    /// A code block's text wider than the block: drawn clipped to `view`, moved by the
    /// block's own scroll (`block`: its place among the document's), `content` wide in
    /// `room`.
    Scroll {
        view: Rect,
        block: usize,
        content: f32,
        room: f32,
        items: Vec<Item>,
    },
    /// A text on its own background, over what scrolls (a code block's language).
    Badge {
        pill: Rect,
        bg: Rgba,
        text: Box<Item>,
    },
    /// A diagram's shapes (its scene at scale `k` in `rect`): drawn in tiles, made for the
    /// part shown (`key`: the tiles' cache key).
    Diagram {
        rect: Rect,
        scene: Arc<crate::mermaid::Scene>,
        k: f32,
        key: u64,
    },
}

/// What a click on a part of the document does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// A link: a URL, a path relative to the file, or an anchor (`#heading`).
    Link(String),
    /// A task list's checkbox: the source's `[ ]` / `[x]` at this range.
    Task(Range<usize>),
    /// A `<details>` (or a folding callout) opened or closed: its index in the document.
    Fold(usize),
    /// A diagram (its index in the layout's diagrams): opened larger.
    Diagram(usize),
    /// A line of the source (0-based): opened there (a diagram's error).
    Line(usize),
}

/// The document to lay out: its source and version, the width and scale, the folder its
/// images are relative to, and the folds the user turned the other way.
pub struct Page<'a> {
    pub source: &'a str,
    pub version: u64,
    pub width: f32,
    pub scale: f32,
    pub dir: &'a Path,
    pub toggled: &'a HashSet<usize>,
}

pub struct Layout {
    pub version: u64,
    /// Long code lines were wrapped (the setting it was built with).
    pub wrap: bool,
    pub width: f32,
    pub scale: f32,
    pub height: f32,
    pub items: Vec<Item>,
    /// Clickable areas and what they do.
    pub hits: Vec<(Rect, Hit)>,
    /// Where each anchor is (headings, footnotes, `<a name>`): its name and its top.
    pub anchors: Vec<(String, f32)>,
    /// The code blocks (their copy button shows over them) and their text.
    pub code_blocks: Vec<(Rect, String)>,
    pub diagrams: Vec<crate::diagrams::DiagramBlock>,
    /// Its text in the document's order, for selecting.
    pub runs: Vec<TextRun>,
    /// Its heights and the source's lines (scrolling with the text).
    pub lines: crate::sync::LineMap,
}

impl Layout {
    /// The top of anchor `name` (`#` left out), if the document has it.
    pub fn anchor(&self, name: &str) -> Option<f32> {
        let name = name.trim_start_matches('#');
        self.anchors.iter().find(|(a, _)| a == name).map(|(_, y)| *y)
    }
}

/// A code block's highlighting: the captures (byte ranges, names); `None`: no grammar.
pub(crate) type Highlight = Option<Arc<Vec<(Range<usize>, Arc<str>)>>>;

/// What the layout looks up instead of working out while painting: the images' sizes, the
/// highlighted code, the diagrams, the formulas. It goes with a layout built on a worker
/// and comes back with it (what arrived meanwhile waits in its channels).
pub struct Caches {
    pub images: ImageSizes,
    pub code: crate::jobs::Jobs<Highlight>,
    pub diagrams: crate::jobs::Jobs<crate::mermaid::Outcome>,
    /// Each diagram of the document (by its place among them) as last drawn: shown while
    /// its new text is worked out.
    pub shown: HashMap<usize, crate::mermaid::Rendered>,
    /// The formulas set (by their text, style, size and colors), on the render pool.
    pub math: crate::jobs::Jobs<crate::math::Typeset>,
    /// The math font's text system, while the document has formulas.
    pub math_text: Option<crate::math::MathText>,
    /// Worked out right away, the layout too (tests).
    inline: bool,
}

impl Default for Caches {
    fn default() -> Self {
        Self {
            images: ImageSizes::default(),
            code: crate::jobs::Jobs::default(),
            diagrams: crate::jobs::Jobs::default(),
            shown: HashMap::new(),
            math: crate::jobs::Jobs::with_limit(256),
            math_text: None,
            inline: false,
        }
    }
}

impl Caches {
    #[cfg(test)]
    pub(crate) fn inline() -> Self {
        Self {
            images: ImageSizes::inline(),
            code: crate::jobs::Jobs::inline(),
            diagrams: crate::jobs::Jobs::inline(),
            shown: HashMap::new(),
            math: crate::jobs::Jobs::inline(),
            math_text: None,
            inline: true,
        }
    }

    /// Everything is worked out right away, the layout too (tests).
    pub(crate) fn is_inline(&self) -> bool {
        self.inline
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.images.set_waker(wake.clone());
        self.code.set_waker(wake.clone());
        self.diagrams.set_waker(wake.clone());
        self.math.set_waker(wake);
    }

    /// What came from the workers taken: `true` if any of it changes the layout (an image's
    /// size, a code block's colors, a diagram, a formula).
    pub fn poll(&mut self) -> bool {
        self.images.poll() | self.code.poll() | self.diagrams.poll() | self.math.poll()
    }

    pub fn loading(&self) -> bool {
        self.images.loading()
            || self.code.loading()
            || self.diagrams.loading()
            || self.math.loading()
    }

    pub fn clear(&mut self) {
        self.images.clear();
        self.code.clear();
        self.diagrams.clear();
        self.shown.clear();
        self.math.clear();
        self.math_text = None;
    }

    pub(crate) fn begin(&mut self) {
        self.images.begin();
        self.code.begin();
        self.diagrams.begin();
        self.math.begin();
    }

    pub(crate) fn end(&mut self) {
        self.images.end();
        self.code.end();
        self.diagrams.end();
        self.math.end();
    }

    /// What they hold in memory, bytes.
    pub fn memory(&self) -> u64 {
        let code: usize = self.code.values().flatten().map(|h| h.len() * 32).sum();
        let mut diagrams = 0u64;
        let mut seen = std::collections::HashSet::new();
        for r in self.diagrams.values().filter_map(|o| o.as_ref().ok()).chain(self.shown.values()) {
            if seen.insert(Arc::as_ptr(&r.scene)) {
                diagrams += r.scene.memory() as u64;
            }
        }
        code as u64 + diagrams
    }
}

/// What painting looks up: the images' pixels and the diagrams' tiles, made for what's
/// shown. They stay with the view while a layout is built.
pub struct Drawn {
    pub images: ImageCache,
    /// The diagrams' tiles (their masks), made where they're shown.
    pub tiles: crate::jobs::Jobs<Arc<Vec<crate::tiles::Mask>>>,
}

impl Default for Drawn {
    fn default() -> Self {
        Self { images: ImageCache::default(), tiles: crate::jobs::Jobs::with_limit(512) }
    }
}

impl Drawn {
    #[cfg(test)]
    pub(crate) fn inline() -> Self {
        Self { images: ImageCache::inline(), tiles: crate::jobs::Jobs::inline() }
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.images.set_waker(wake.clone());
        self.tiles.set_waker(wake);
    }

    /// Takes what arrived (it only redraws).
    pub fn poll(&mut self) {
        self.images.poll();
        self.tiles.poll();
    }

    pub fn loading(&self) -> bool {
        self.images.loading() || self.tiles.loading()
    }

    pub fn clear(&mut self) {
        self.images.clear();
        self.tiles.clear();
    }

    /// What they hold: (in memory, on the GPU), bytes.
    pub fn memory(&self) -> (u64, u64) {
        let (ram, gpu) = self.images.memory();
        let tiles: u64 =
            self.tiles.values().flat_map(|t| t.iter()).map(|(i, _)| i.gpu_memory()).sum();
        (ram, gpu + tiles)
    }
}

/// An inline style (nested tags add up).
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    pub link: bool,
    pub sup: bool,
    pub sub: bool,
    pub kbd: bool,
    pub mark: bool,
    pub underline: bool,
}

/// An image set in a line of text, at its size on the screen (physical pixels): its file
/// (as of its stamp) and the width it's decoded at.
#[derive(Clone)]
pub(crate) struct InlineImage {
    pub path: PathBuf,
    pub stamp: Stamp,
    pub decode: u32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone)]
pub(crate) struct Span {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
    /// An image in place of the text.
    pub image: Option<InlineImage>,
    /// A formula in place of the text.
    pub math: Option<Arc<crate::math::Formula>>,
}

impl Span {
    pub fn text(text: impl Into<String>, style: Style, link: Option<String>) -> Self {
        Self { text: text.into(), style, link, image: None, math: None }
    }
}

/// How a block's lines sit between its margins.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub(crate) enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// How a run of text is set: between `x0` and `x1`, its size, weight, color, line height
/// and alignment.
pub(crate) struct Run {
    pub x0: f32,
    pub x1: f32,
    pub size: f32,
    pub weight: u16,
    pub color: Rgba,
    pub line: f32,
    pub align: Align,
}

pub(crate) struct Builder<'a> {
    pub text: &'a mut TextSystem,
    pub palette: &'a Palette,
    pub s: f32,
    /// Content margins (centered, reading width).
    pub left: f32,
    pub right: f32,
    pub y: f32,
    pub items: Vec<Item>,
    pub hits: Vec<(Rect, Hit)>,
    pub anchors: Vec<(String, f32)>,
    pub code_blocks: Vec<(Rect, String)>,
    pub diagrams: Vec<crate::diagrams::DiagramBlock>,
    /// How many code blocks scroll across.
    pub scrolls: usize,
    pub body: f32,
    /// The text for selecting, the separator the next run takes (its own when `None`), the
    /// blocks met and the source of the current one.
    pub runs: Vec<TextRun>,
    pub sep: Option<Sep>,
    pub blocks: u32,
    pub src: Range<usize>,
}

#[cfg(test)]
#[path = "../../tests/unit/layout.rs"]
mod tests;
