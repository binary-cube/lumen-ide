//! Document layout: from Markdown events, a list of positioned elements (text,
//! rectangles, images, icons) and what can be clicked, relative to the document corner.
//! Text is wrapped to the available width, word by word; the walk over the parser's
//! events is in [`crate::walk`].

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime};

use kernel::{Point, Rect, Rgba};
use render_api::ImageData;
use text_layout::{Font, ShapedLine, TextSystem};
use ui_components::Palette;

use crate::select::{Sep, TextRun};

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

/// What's known of an image file: being looked at, unreadable, or its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Probe {
    Pending,
    Missing,
    /// A web image that couldn't be had (why).
    Failed(String),
    Size(u32, u32),
}

/// A file's modification time, as a worker last read it (an image edited on disk is read
/// again).
pub(crate) type Stamp = Option<SystemTime>;

/// A file decoded for a width (rounded up to 256 pixels), at the stamp it was read at.
type DecodeKey = (PathBuf, Stamp, u32);

/// What a worker learned of an image file: its stamp and its size.
type Probed = (Stamp, Probe);

/// The files whose stamp changed since they were probed, probed again.
type Rechecked = Vec<(PathBuf, Probed)>;

/// How often the files of a document being painted are looked at again (on a worker).
const RECHECK: Duration = Duration::from_secs(2);

/// Local images' sizes: the layout asks only these (read from their headers, in the
/// background), each file's modification time read with it on the same worker. Neither
/// the layout nor the paint reads the disk: while the document is painted a worker looks
/// at the stamps again every [`RECHECK`] (an edited image is probed again, and its
/// picture, keyed by the new stamp, decoded again).
pub struct ImageSizes {
    /// Each file's stamp and size, by path.
    sizes: HashMap<PathBuf, Probed>,
    probing: HashMap<PathBuf, Receiver<Probed>>,
    /// The files the layout being built uses (the others go when it's done).
    used: HashSet<PathBuf>,
    /// The stamps being looked at again, and when they last were.
    rechecking: Option<Receiver<Rechecked>>,
    checked: Option<Instant>,
    recheck_every: Duration,
    /// The images from the web (downloaded into the cache folder).
    web: crate::web::WebImages,
    /// Wakes the app when something arrives.
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Worked out right away (tests).
    inline: bool,
}

impl Default for ImageSizes {
    fn default() -> Self {
        Self {
            sizes: HashMap::new(),
            probing: HashMap::new(),
            used: HashSet::new(),
            rechecking: None,
            checked: None,
            recheck_every: RECHECK,
            web: crate::web::WebImages::default(),
            wake: None,
            inline: false,
        }
    }
}

/// Local images' pixels: decoded when an image is about to be shown (a screen away), at
/// the size it's shown — never bigger — on the `image-decode` pool, never while painting,
/// under the stamp the layout gave its picture. They live only on the GPU once drawn
/// (`ImageData::gpu_only`); one whose texture was freed (out of sight: the renderer, the
/// previews' budget) is decoded again when it's shown again.
#[derive(Default)]
pub struct ImageCache {
    decoded: HashMap<DecodeKey, Option<ImageData>>,
    decoding: HashMap<DecodeKey, Receiver<Option<ImageData>>>,
    /// Wakes the app when something arrives.
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Worked out right away (tests).
    inline: bool,
}

/// A file's modification time (on a worker).
fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// An image's stamp and its size, from its header (the stamp read first: an edit made
/// meanwhile is seen at the next look).
fn probe(path: &Path) -> Probed {
    let modified = stamp(path);
    let size = match image_decode::probe(path) {
        Ok(((w, h), _)) if w > 0 && h > 0 => Probe::Size(w, h),
        _ => Probe::Missing,
    };
    (modified, size)
}

/// The files of `known` whose stamp is no longer the one known, probed again.
fn recheck(known: Vec<(PathBuf, Stamp)>) -> Rechecked {
    known
        .into_iter()
        .filter(|(path, modified)| stamp(path) != *modified)
        .map(|(path, _)| {
            let probed = probe(&path);
            (path, probed)
        })
        .collect()
}

/// An image read and decoded at most `width` pixels wide (an SVG at its size × `scale`):
/// the pixels on the GPU once drawn.
fn decode(path: &Path, width: u32, scale: f32) -> Option<ImageData> {
    crate::DISK.read(std::fs::metadata(path).map_or(0, |m| m.len()));
    let decoded = if crate::web::downloaded(path) {
        image_decode::load_untrusted_for_width(path, width, scale)
    } else {
        image_decode::load_for_width(path, width, scale)
    };
    decoded.ok().map(|d| ImageData::gpu_only(d.width, d.height, d.pixels))
}

#[cfg(test)]
impl ImageSizes {
    /// Working out while laying out (tests of what's cached, without waiting).
    pub(crate) fn inline() -> Self {
        Self { inline: true, ..Self::default() }
    }
}

#[cfg(test)]
impl ImageCache {
    /// Decoding while painting (tests of what's cached, without waiting).
    pub(crate) fn inline() -> Self {
        Self { inline: true, ..Self::default() }
    }
}

/// The threads reading and decoding a document's images.
pub const DECODE_THREAD: &str = "markdown-image";

/// The previews' decodes, shared by all (each its own pool: as many full decodes at once as
/// previews open).
static DECODES: std::sync::LazyLock<kernel::Pool> =
    std::sync::LazyLock::new(|| kernel::Pool::new(DECODE_THREAD, 2, kernel::work::IDLE));

/// The width an image shown `width` pixels wide is decoded at.
pub(crate) fn decode_width(width: f32) -> u32 {
    ((width.max(1.0) as u32).div_ceil(256) * 256).min(image_decode::MAX_SIDE)
}

/// What came out of a worker's channel: taken, still to come, or gone.
fn take<T>(rx: &Receiver<T>) -> Option<Option<T>> {
    match rx.try_recv() {
        Ok(v) => Some(Some(v)),
        Err(std::sync::mpsc::TryRecvError::Empty) => None,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
    }
}

impl ImageSizes {
    pub fn clear(&mut self) {
        self.sizes.clear();
        self.probing.clear();
        self.used.clear();
        self.rechecking = None;
        self.checked = None;
        self.web.clear();
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.web.set_waker(wake.clone());
        self.wake = wake;
    }

    /// Sizes are being read (or downloaded). A look at the stamps isn't waited for: it
    /// wakes the app if something changed.
    pub fn loading(&self) -> bool {
        !self.probing.is_empty() || self.web.loading()
    }

    /// Takes what arrived: `true` if a size did, or an image changed on disk (the layout is
    /// built again). Every [`RECHECK`] the stamps are looked at again.
    pub fn poll(&mut self) -> bool {
        let mut sizes = Vec::new();
        self.probing.retain(|path, rx| match take(rx) {
            None => true,
            Some(p) => {
                sizes.extend(p.map(|p| (path.clone(), p)));
                false
            }
        });
        let any = !sizes.is_empty();
        self.sizes.extend(sizes);
        self.start_recheck();
        let changed = self.take_recheck();
        any | self.web.poll() | changed
    }

    /// The stamps looked at again arrived: `true` if a file changed (its new size kept).
    fn take_recheck(&mut self) -> bool {
        let Some(rx) = &self.rechecking else { return false };
        let Some(changed) = take(rx) else { return false };
        self.rechecking = None;
        let mut any = false;
        // Only files still known: one the document dropped meanwhile isn't brought back.
        for (path, probed) in changed.unwrap_or_default() {
            if let Some(known) = self.sizes.get_mut(&path) {
                *known = probed;
                any = true;
            }
        }
        any
    }

    /// A look at the known files' stamps, on a worker, once [`RECHECK`] passed since the
    /// last (only while the document is painted: nothing while it's out of sight).
    fn start_recheck(&mut self) {
        if self.rechecking.is_some() || self.sizes.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.checked.is_some_and(|t| now.duration_since(t) < self.recheck_every) {
            return;
        }
        if self.checked.replace(now).is_none() {
            // Just probed: the first look waits a whole interval.
            return;
        }
        // Every file known, a missing one too (it may appear).
        let known: Vec<(PathBuf, Stamp)> =
            self.sizes.iter().map(|(path, (modified, _))| (path.clone(), *modified)).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        if self.inline {
            let _ = tx.send(recheck(known));
        } else {
            let wake = self.wake.clone();
            DECODES.spawn(kernel::Priority::Background, None, move || {
                let changed = recheck(known);
                let any = !changed.is_empty();
                let _ = tx.send(changed);
                if any && let Some(wake) = wake {
                    wake();
                }
            });
        }
        self.rechecking = Some(rx);
    }

    /// A layout starts: what it uses is recorded.
    pub(crate) fn begin(&mut self) {
        self.used.clear();
    }

    /// The layout is done: what it didn't use goes.
    pub(crate) fn end(&mut self) {
        let used = std::mem::take(&mut self.used);
        self.sizes.retain(|path, _| used.contains(path));
        self.used = used;
    }

    /// Image `src`'s size and the stamp it came with (asked in the background the first
    /// time; the disk is never read here).
    pub(crate) fn size(&mut self, dir: &Path, src: &str) -> (Probe, PathBuf, Stamp) {
        let lower = src.get(..8).unwrap_or(src).to_ascii_lowercase();
        let path = if lower.starts_with("https://") || lower.starts_with("http://") {
            match self.web.get(src) {
                crate::web::Fetch::Ready(path) => path,
                crate::web::Fetch::Pending => return (Probe::Pending, PathBuf::new(), None),
                crate::web::Fetch::Failed(why) => {
                    return (Probe::Failed(why), PathBuf::new(), None);
                }
            }
        } else {
            dir.join(src)
        };
        if src.starts_with("data:") || (src.contains("://") && !crate::web::downloaded(&path)) {
            return (Probe::Missing, path, None);
        }
        self.used.insert(path.clone());
        if let Some((modified, p)) = self.sizes.get(&path) {
            return (p.clone(), path, *modified);
        }
        if self.inline {
            let (modified, p) = probe(&path);
            self.sizes.insert(path.clone(), (modified, p.clone()));
            return (p, path, modified);
        }
        if !self.probing.contains_key(&path) {
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = self.wake.clone();
            let p = path.clone();
            DECODES.spawn(kernel::Priority::Interactive, None, move || {
                let _ = tx.send(probe(&p));
                if let Some(wake) = wake {
                    wake();
                }
            });
            self.probing.insert(path.clone(), rx);
        }
        (Probe::Pending, path, None)
    }
}

impl ImageCache {
    /// What its images hold: (in memory, on the GPU), bytes.
    pub fn memory(&self) -> (u64, u64) {
        self.decoded.values().flatten().fold((0, 0), |(ram, gpu), image| {
            (ram + image.memory() as u64, gpu + image.gpu_memory())
        })
    }

    pub fn clear(&mut self) {
        self.decoded.clear();
        self.decoding.clear();
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.wake = wake;
    }

    /// Pixels are being decoded.
    pub fn loading(&self) -> bool {
        !self.decoding.is_empty()
    }

    /// Takes the pixels that arrived (they only redraw).
    pub fn poll(&mut self) {
        let mut images = Vec::new();
        self.decoding.retain(|key, rx| match take(rx) {
            None => true,
            Some(i) => {
                images.extend(i.map(|i| (key.clone(), i)));
                false
            }
        });
        self.decoded.extend(images);
    }

    /// A layout was put in place: the pixels of files (or stamps) it doesn't show go.
    pub(crate) fn keep(&mut self, layout: &Layout) {
        let shown: HashSet<(&Path, Stamp)> = layout
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Picture { path, stamp, .. } => Some((path.as_path(), *stamp)),
                _ => None,
            })
            .collect();
        self.decoded.retain(|(path, modified, _), _| shown.contains(&(path.as_path(), *modified)));
    }

    /// The pixels of `path` (as of `stamp`) decoded `width` pixels wide (an SVG at most its
    /// size × `scale`), if they're there; otherwise asked for. The disk is never read here.
    pub(crate) fn image(
        &mut self,
        path: &Path,
        stamp: Stamp,
        width: u32,
        scale: f32,
    ) -> Option<ImageData> {
        let key = (path.to_path_buf(), stamp, width);
        match self.decoded.get(&key) {
            Some(Some(image)) if !image.is_lost() => return Some(image.clone()),
            Some(None) => return None,
            Some(Some(_)) => {
                // Freed out of sight: decoded again.
                self.decoded.remove(&key);
            }
            None => {}
        }
        if self.inline {
            let image = decode(path, width, scale);
            self.decoded.insert(key, image.clone());
            return image;
        }
        if !self.decoding.contains_key(&key) {
            let (tx, rx) = std::sync::mpsc::channel();
            let wake = self.wake.clone();
            let p = path.to_path_buf();
            DECODES.spawn(kernel::Priority::Interactive, None, move || {
                let _ = tx.send(decode(&p, width, scale));
                if let Some(wake) = wake {
                    wake();
                }
            });
            self.decoding.insert(key, rx);
        }
        None
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

#[cfg(test)]
#[path = "../tests/unit/layout.rs"]
mod tests;
