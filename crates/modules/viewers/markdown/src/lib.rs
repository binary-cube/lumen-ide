//! Markdown module: rendered preview of `.md` files, next to the source (Editor /
//! Split / Preview). Layout is built directly from `pulldown-cmark` events — CommonMark
//! with GitHub's flavor (tables, task lists, alerts, footnotes, autolinks, emoji, anchors)
//! and the HTML READMEs use — and reused as long as the source and width stay the same.
//! Everything is drawn on the GPU, like the editor.

mod blocks;
mod budget;
mod build;
mod building;
mod diagrams;
mod html;
mod inline;
mod jobs;
mod layout;
mod math;
mod mermaid;
mod pressing;
mod select;
mod settings;
mod sync;
mod tiles;
mod view;
mod viewer;
mod walk;
mod web;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::control::{BarThumb, Control, Controls, Thumb};
use ui_components::ds;
use ui_components::{AppRequest, Viewer, ViewerAction, ViewerKind, ViewerPaint, ViewerProvider};
use ui_kit::{draw_icon, draw_text};

use crate::layout::{Hit, Item, Layout, Page};
use crate::select::Selection;

/// The images read from the disk (the Task Manager's disk column).
pub(crate) static DISK: workbench_model::DiskIo = workbench_model::DiskIo::new();

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("viewer.markdown"),
    name: "Markdown",
    description: "A rendered preview for Markdown files, next to the source",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "markdown", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct MarkdownViewerModule;

impl Module for MarkdownViewerModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    /// "Clear and Rebuild Caches" (an idle of zero): the images downloaded from the web go.
    fn release_idle(&self, idle: std::time::Duration) -> bool {
        if !idle.is_zero() {
            return false;
        }
        if let Some(cache) = web::cache() {
            cache.clear();
        }
        true
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(workbench_model::ModuleCounters { disk: Some(&DISK), traffic: None });
        ctx.contribute(workbench_model::ModuleThreads(&[
            layout::DECODE_THREAD,
            build::LAYOUT_THREAD,
            jobs::RENDER_THREAD,
            web::WEB_THREAD,
        ]));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::MarkdownSettings,
        ));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(MarkdownProvider));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(view::DiagramProvider));
        Ok(())
    }
}

#[derive(Debug)]
struct MarkdownProvider;

const EXTENSIONS: &[&str] = &["md", "markdown", "mdown", "mkd"];

impl ViewerProvider for MarkdownProvider {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        EXTENSIONS.contains(&ext.as_str()).then_some(ViewerKind::Preview)
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(MarkdownViewer::new(path)))
    }
}

struct MarkdownViewer {
    /// The file (a task ticked writes its text) and its directory (images, relative links).
    path: PathBuf,
    dir: PathBuf,
    source: String,
    version: u64,
    /// The text's words, counted for its version (the status bar's).
    words: std::cell::Cell<Option<(u64, usize)>>,
    layout: Option<Layout>,
    scroll: f32,
    bounds: Rect,
    /// What the layout looks up: here, or away with a layout being built (`None`).
    caches: Option<layout::Caches>,
    /// What they held when they left (the memory shown meanwhile).
    caches_ram: u64,
    /// What painting looks up (the pictures' pixels, the diagrams' tiles).
    drawn: layout::Drawn,
    /// The layout being built on a worker, and the text system the builds shape with.
    building: Option<build::Building>,
    layout_text: Option<build::LayoutText>,
    /// What a build was for when it gave nothing back (not tried again for the same).
    failed: Option<build::Want>,
    /// Something the layout depends on changed (a size, a highlight, a fold): built again.
    dirty: bool,
    /// The palette the layout was built with (its colors are in the layout): another theme
    /// builds it again.
    built_with: Option<ui_components::Palette>,
    /// The folds (`<details>`, folding callouts) turned the other way than written.
    toggled: HashSet<usize>,
    /// The mouse, in document coordinates (a code block's copy button shows under it).
    hover: Option<Point>,
    notifications: Vec<workbench_model::Notification>,
    /// A diagram's copy or save being made (from its menu), and the diagram right-clicked.
    export: Option<view::DiagramView>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    menu_diagram: Option<usize>,
    action: Option<ViewerAction>,
    /// Each code block's scroll across (by its place among those that scroll).
    code_scroll: std::collections::HashMap<usize, f32>,
    /// The bar across of the code block under the mouse, and which block it's for.
    hbar: Option<ui_components::Scrollbar>,
    hbar_block: Option<usize>,
    /// The text selected (offsets among the layout's runs), and the drag selecting it.
    selection: Option<Selection>,
    selecting: Option<Selecting>,
    /// When the last frame scrolled while a drag held the mouse near an edge.
    edge_tick: Option<std::time::Instant>,
    /// The document's scrollbar (hidden at rest, as the app's lists').
    vbar: ui_components::EdgeScrollbar,
    /// Its controls' state of use (see [`MarkdownViewer::with_controls`]).
    controls: Controls,
    /// A source line its text asked to be brought to the top, before it was laid out.
    sync_to: Option<f32>,
}

/// A drag selecting text: by what it grows, the press on a link or a diagram (followed
/// if the mouse doesn't move), where the mouse is (window coordinates).
struct Selecting {
    grain: Grain,
    pending: Option<(Point, Hit)>,
    last: Point,
}

/// What a drag selects by: characters, words (a double click's), blocks (a triple's);
/// with the first word or block.
#[derive(Clone, Copy)]
enum Grain {
    Char,
    Word(usize, usize),
    Block(usize, usize),
}

/// How close to the top or bottom a drag scrolls (logical pixels).
const EDGE: f32 = 28.0;

/// The place of the document's scrollbar among its controls (see
/// [`MarkdownViewer::with_controls`]).
const DOCUMENT_BAR: usize = 0;

impl MarkdownViewer {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            source: String::new(),
            version: 0,
            words: std::cell::Cell::new(None),
            layout: None,
            scroll: 0.0,
            bounds: Rect::default(),
            caches: Some(layout::Caches::default()),
            caches_ram: 0,
            drawn: layout::Drawn::default(),
            building: None,
            layout_text: None,
            failed: None,
            dirty: false,
            built_with: None,
            toggled: HashSet::new(),
            hover: None,
            notifications: Vec::new(),
            export: None,
            waker: None,
            menu_diagram: None,
            action: None,
            code_scroll: std::collections::HashMap::new(),
            hbar: None,
            hbar_block: None,
            selection: None,
            selecting: None,
            edge_tick: None,
            vbar: ui_components::EdgeScrollbar::auto_hiding(),
            controls: Controls::default(),
            sync_to: None,
        }
    }

    fn max_scroll(&self) -> f32 {
        self.layout.as_ref().map_or(0.0, |l| (l.height - self.bounds.size.height).max(0.0))
    }

    /// An image about to be shown is lost (its pixels were only on the GPU, and the
    /// renderer freed the texture while it was out of sight): the layout is built again,
    /// which decodes it again.
    fn shows_lost_image(&self, layout: &Layout, b: Rect) -> bool {
        let dy = b.min_y() - self.scroll.round();
        layout.items.iter().any(|item| match item {
            Item::Image { rect, image } => {
                rect.max_y() + dy >= b.min_y() && rect.min_y() + dy <= b.max_y() && image.is_lost()
            }
            _ => false,
        })
    }

    /// `p` (window coordinates) in document coordinates.
    fn local(&self, p: Point) -> Point {
        let b = self.bounds;
        Point::new(p.x - b.min_x(), p.y - b.min_y() + self.scroll.round())
    }

    /// The copy button of the code block under the mouse: the block's index and the
    /// button (document coordinates).
    fn copy_button(&self) -> Option<(usize, Rect)> {
        let layout = self.layout.as_ref()?;
        let at = self.hover?;
        layout
            .code_blocks
            .iter()
            .position(|(r, _)| r.contains(at))
            .map(|i| (i, corner(layout.code_blocks[i].0, layout.scale)))
    }

    /// The "open larger" button of the diagram under the mouse.
    fn zoom_button(&self) -> Option<(usize, Rect)> {
        let layout = self.layout.as_ref()?;
        let at = self.hover?;
        layout
            .diagrams
            .iter()
            .position(|d| d.rect.contains(at))
            .map(|i| (i, corner(layout.diagrams[i].rect, layout.scale)))
    }

    /// What the menu and the modal of diagram `index` work on.
    fn diagram_request(&self, index: usize) -> Option<view::DiagramRequest> {
        let d = self.layout.as_ref()?.diagrams.get(index)?;
        Some(view::DiagramRequest {
            scene: Arc::clone(d.scene.as_ref()?),
            source: d.source.clone(),
            title: d.title.clone(),
            dir: self.dir.clone(),
        })
    }

    /// Diagram `index` opened larger, in the module's modal.
    fn open_diagram(&mut self, index: usize) -> Option<ViewerAction> {
        let request = self.diagram_request(index)?;
        let title = request.title.clone().unwrap_or_else(|| i18n::t("markdown.diagram_title"));
        Some(ViewerAction::OpenView(ui_components::OpenView::modal(
            title,
            view::EXTENSION,
            request,
        )))
    }
}

/// A floating button's room from its block's corner, and its icon (logical pixels).
const CORNER_INSET: f32 = 6.0;
const FLOATING_ICON: f32 = 14.0;

/// A block's corner button (document coordinates).
fn corner(r: Rect, s: f32) -> Rect {
    let side = (ds::button_size::MEDIUM * s).round();
    let inset = (CORNER_INSET * s).round();
    Rect::new(r.max_x() - side - inset, r.min_y() + inset, side, side)
}

impl MarkdownViewer {
    /// Code block `block` that scrolls across: its view (document coordinates) and its
    /// scroll's measures.
    /// Its controls (the document's scrollbar, the bar across of the code block under the
    /// mouse or dragged), the topmost first.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let block = self.hscroll_block().map(|(block, _, m)| (block, m));
        let mut document = Thumb { bar: &mut self.vbar, offset: &mut self.scroll };
        // Made once a block scrolling across was under the mouse; its scroll kept by block.
        let mut code = match (self.hbar.as_mut(), block) {
            (Some(bar), Some((block, metrics))) => Some(BarThumb {
                bar,
                metrics,
                offset: self.code_scroll.entry(block).or_insert(0.0),
            }),
            _ => None,
        };
        let mut parts: [&mut dyn Control; 2] = [&mut document, &mut code];
        f(&mut self.controls, &mut parts)
    }

    fn scroll_block(&self, block: usize) -> Option<(usize, Rect, ui_components::ScrollMetrics)> {
        let layout = self.layout.as_ref()?;
        layout.items.iter().find_map(|item| match item {
            Item::Scroll { view, block: b, content, room, .. } if *b == block => {
                let offset = self
                    .code_scroll
                    .get(b)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, (content - room).max(0.0));
                Some((
                    *b,
                    *view,
                    ui_components::ScrollMetrics { content: *content, viewport: *room, offset },
                ))
            }
            _ => None,
        })
    }

    /// The code block that scrolls across under the mouse (or whose bar is being dragged).
    fn hscroll_block(&self) -> Option<(usize, Rect, ui_components::ScrollMetrics)> {
        if self.hbar.as_ref().is_some_and(ui_components::Scrollbar::is_dragging)
            && let Some(block) = self.hbar_block
        {
            return self.scroll_block(block);
        }
        let at = self.hover?;
        let layout = self.layout.as_ref()?;
        let block = layout.items.iter().find_map(|item| match item {
            Item::Scroll { view, block, .. } if view.contains(at) => Some(*block),
            _ => None,
        })?;
        self.scroll_block(block)
    }

    /// A diagram's copy or save from the menu: what it asks for, once made.
    fn settle(&mut self) {
        let Some(export) = &mut self.export else { return };
        let (action, notes) = export.settle();
        self.notifications.extend(notes);
        if action.is_some() {
            self.action = action;
        }
    }

    fn hit_at(&self, local: Point) -> Option<Hit> {
        let layout = self.layout.as_ref()?;
        layout.hits.iter().rev().find(|(r, _)| r.contains(local)).map(|(_, hit)| hit.clone())
    }

    /// A link clicked: an anchor of the document scrolls to it; relative links become
    /// absolute paths (opened in the editor).
    fn open_link(&mut self, url: &str) -> Option<ViewerAction> {
        if let Some(anchor) = url.strip_prefix('#') {
            let y = self.layout.as_ref()?.anchor(anchor)?;
            let margin = (12.0 * self.layout.as_ref().map_or(1.0, |l| l.scale)).round();
            self.scroll = (y - margin).clamp(0.0, self.max_scroll());
            return None;
        }
        if url.contains("://") || url.starts_with("mailto:") {
            return Some(ViewerAction::OpenLink(url.to_owned()));
        }
        let path = url.split('#').next().unwrap_or(url);
        let path = percent_decode(path);
        Some(ViewerAction::OpenLink(self.dir.join(path).to_string_lossy().into_owned()))
    }
}

/// `%20` and the like decoded (a link to `My%20File.md`).
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_owned())
}

/// The source with the task at `range` (its `[ ]` / `[x]`) ticked or unticked.
fn toggle_task(source: &str, range: std::ops::Range<usize>) -> Option<String> {
    let marker = source.get(range.clone())?;
    let open = marker.find('[')?;
    let at = range.start + open + 1;
    let current = source.get(at..at + 1)?;
    let new = if current == " " { "x" } else { " " };
    let mut text = String::with_capacity(source.len());
    text.push_str(&source[..at]);
    text.push_str(new);
    text.push_str(&source[at + 1..]);
    Some(text)
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
