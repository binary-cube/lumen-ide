//! Markdown module: rendered preview of `.md` files, next to the source (Editor /
//! Split / Preview). Layout is built directly from `pulldown-cmark` events — CommonMark
//! with GitHub's flavor (tables, task lists, alerts, footnotes, autolinks, emoji, anchors)
//! and the HTML READMEs use — and reused as long as the source and width stay the same.
//! Everything is drawn on the GPU, like the editor.

mod blocks;
mod budget;
mod build;
mod diagrams;
mod html;
mod inline;
mod jobs;
mod layout;
mod math;
mod mermaid;
mod select;
mod settings;
mod sync;
mod tiles;
mod view;
mod walk;
mod web;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::{Point, Rect};
use render_api::Quad;
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

impl Viewer for MarkdownViewer {
    /// Its source and its images.
    fn memory(&self) -> ui_components::ViewMemory {
        let (ram, gpu) = self.drawn.memory();
        let ram = ram + self.caches.as_ref().map_or(self.caches_ram, layout::Caches::memory);
        // The selectable text: each run's place and text.
        let runs = self.layout.as_ref().map_or(0, |l| {
            l.runs
                .iter()
                .map(|r| std::mem::size_of_val(r) + r.text.capacity() + r.copy.capacity())
                .sum::<usize>()
        });
        ui_components::ViewMemory { ram: ram + (self.source.capacity() + runs) as u64, gpu }
    }

    fn purge(&mut self) {
        // The layout holds the images too: it goes as well (rebuilt on the next paint). A
        // build on its way is let go, with the caches it took.
        self.drawn.clear();
        match &mut self.caches {
            Some(caches) => caches.clear(),
            None => self.caches = Some(layout::Caches::default()),
        }
        self.building = None;
        self.layout_text = None;
        self.layout = None;
    }

    fn set_source(&mut self, source: &str) {
        if source != self.source {
            self.source = source.to_owned();
            self.version += 1;
        }
    }

    fn moved(&mut self, path: &Path) {
        self.path = path.to_path_buf();
        self.dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    }

    /// Its images are still being decoded (the app redraws until they're in).
    fn is_loading(&self) -> bool {
        self.building.is_some()
            || self.caches.as_ref().is_some_and(layout::Caches::loading)
            || self.drawn.loading()
            || self.export.as_ref().is_some_and(view::DiagramView::waiting)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.bounds = ctx.bounds;
        let palette = ctx.palette;
        let width = ctx.bounds.size.width;
        self.waker.clone_from(&ctx.waker);
        self.drawn.set_waker(ctx.waker.clone());
        self.drawn.poll();
        self.take_built();
        if let Some(caches) = &mut self.caches {
            caches.set_waker(ctx.waker.clone());
            if caches.poll() {
                self.dirty = true;
            }
        }
        let wrap = settings::wrap_code();
        let stale = self.dirty
            || self.layout.as_ref().is_none_or(|l| {
                l.version != self.version
                    || l.width != width
                    || l.scale != ctx.scale
                    || l.wrap != wrap
                    || self.shows_lost_image(l, ctx.bounds)
            })
            || self.built_with.as_ref() != Some(palette);
        // Made only when something is to be built (the palette is copied into it).
        if stale && self.building.is_none() {
            let version = self.version;
            let want =
                build::Want { version, width, scale: ctx.scale, wrap, palette: palette.clone() };
            if self.failed.as_ref() != Some(&want) {
                self.build(want, ctx.text);
            }
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        self.edge_scroll();
        let copy = self.copy_button();
        let selected = self.selected();
        let Some(layout) = &self.layout else { return };
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, palette.editor_background));
        // Whole pixels, so text and boxes scroll in step (no jitter).
        let dy = b.min_y() - self.scroll.round();
        let visible = |r: &Rect| r.max_y() + dy >= b.min_y() && r.min_y() + dy <= b.max_y();
        let offset = |r: &Rect| {
            Rect::new(r.min_x() + b.min_x(), r.min_y() + dy, r.size.width, r.size.height)
        };
        // The selection, under everything it covers (a formula's glyphs, a diagram's shapes,
        // the text; what's tinted over it lets it show).
        let boxes = selected.map_or_else(Vec::new, |(from, to)| {
            select::boxes(&layout.runs, from, to, |r| visible(r))
        });
        for (rect, _, _) in boxes.iter().filter(|(_, scroll, _)| scroll.is_none()) {
            layer.quad(Quad::filled(offset(rect), palette.selection));
        }
        for item in &layout.items {
            match item {
                Item::Rect { rect, color, radius } if visible(rect) => {
                    layer.quad(Quad::filled(offset(rect), *color).rounded(*radius));
                }
                Item::Border { rect, color, width, radius } if visible(rect) => {
                    layer.quad(
                        Quad::filled(offset(rect), kernel::Rgba::TRANSPARENT)
                            .rounded(*radius)
                            .bordered(*width, *color),
                    );
                }
                Item::Image { rect, image } if visible(rect) => {
                    layer.image(image, offset(rect));
                    budget::drawn(image);
                }
                Item::Picture { rect, path, stamp, decode } => {
                    // Decoded a screen ahead; drawn when it's in view.
                    let margin = b.size.height;
                    let near = Rect::new(
                        rect.min_x(),
                        rect.min_y() - margin,
                        rect.size.width,
                        rect.size.height + 2.0 * margin,
                    );
                    if visible(&near)
                        && let Some(image) =
                            self.drawn.images.image(path, *stamp, *decode, ctx.scale)
                        && visible(rect)
                    {
                        layer.image(&image, offset(rect));
                        budget::drawn(&image);
                    }
                }
                Item::Diagram { rect, scene, k, key } => {
                    // The tiles in view, and those a screen away made ahead.
                    let margin = b.size.height;
                    let near = Rect::new(
                        rect.min_x(),
                        rect.min_y() - margin,
                        rect.size.width,
                        rect.size.height + 2.0 * margin,
                    );
                    if visible(&near) {
                        let at = offset(rect);
                        let placement = tiles::Placement {
                            origin: Point::new(at.min_x(), at.min_y()),
                            stretch: 1.0,
                            view: b,
                            margin,
                            fetch: true,
                        };
                        tiles::draw(layer, &mut self.drawn.tiles, scene, *k, *key, placement);
                    }
                }
                _ => {}
            }
        }
        for item in &layout.items {
            match item {
                Item::Text { x, baseline, text, font, color } => {
                    let y = baseline + dy;
                    if y + font.size < b.min_y() || y - font.size > b.max_y() {
                        continue;
                    }
                    let shaped = ctx.text.shape_line(text, font);
                    draw_text(layer, &shaped, (x + b.min_x()).round(), y.round(), *color);
                }
                Item::Icon { rect, name, color } if visible(rect) => {
                    if let Some(id) = ctx.icons.id(name) {
                        draw_icon(layer, ctx.icons, id, offset(rect), *color);
                    }
                }
                _ => {}
            }
        }
        // Code blocks that scroll across: their text clipped to their frame, moved by its
        // own scroll.
        for item in &layout.items {
            let Item::Scroll { view, block, content, room, items } = item else { continue };
            if !visible(view) {
                continue;
            }
            let clip = offset(view);
            let shift = self
                .code_scroll
                .get(block)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, (content - room).max(0.0))
                .round();
            let inner = ctx.scene.push_layer(Some(clip.intersect(&b).unwrap_or(clip)));
            for (rect, _, _) in boxes.iter().filter(|(_, scroll, _)| *scroll == Some(*block)) {
                let r = offset(rect);
                let r = Rect::new(r.min_x() - shift, r.min_y(), r.size.width, r.size.height);
                inner.quad(Quad::filled(r, palette.selection));
            }
            for it in items {
                let Item::Text { x, baseline, text, font, color } = it else { continue };
                let y = baseline + dy;
                if y + font.size < b.min_y() || y - font.size > b.max_y() {
                    continue;
                }
                let shaped = ctx.text.shape_line(text, font);
                draw_text(inner, &shaped, (x - shift + b.min_x()).round(), y.round(), *color);
            }
        }
        // The hovered block's bar across (or the one being dragged).
        if let Some((block, view, metrics)) = self.hscroll_block() {
            let v = offset(&view);
            // The whole track: the thumb widened on hover fits it (a thinner one cut its
            // rounded ends); at rest it sits by the edge as before.
            let thickness = ui_components::Scrollbar::track_thickness(ctx.scale);
            let track = Rect::new(v.min_x(), v.max_y() - thickness, v.size.width, thickness);
            let bar = self.hbar.get_or_insert_with(|| {
                ui_components::Scrollbar::new(
                    ui_components::ScrollbarStyle::from_palette(palette),
                    ui_components::Orientation::Horizontal,
                )
            });
            bar.set_style(ui_components::ScrollbarStyle::from_palette(palette));
            bar.set_track(track, ctx.scale);
            bar.set_expansion(if bar.is_hovered() || bar.is_dragging() { 1.0 } else { 0.0 });
            bar.paint(ctx.scene, metrics);
            self.hbar_block = Some(block);
        } else if !self.hbar.as_ref().is_some_and(ui_components::Scrollbar::is_dragging) {
            self.hbar_block = None;
        }
        let layer = ctx.scene.push_layer(Some(b));
        // What sits over scrolled text (a code block's language).
        for item in &layout.items {
            let Item::Badge { pill, bg, text } = item else { continue };
            if !visible(pill) {
                continue;
            }
            layer.quad(Quad::filled(offset(pill), *bg).rounded((4.0 * ctx.scale).round()));
            if let Item::Text { x, baseline, text, font, color } = &**text {
                let shaped = ctx.text.shape_line(text, font);
                draw_text(layer, &shaped, (x + b.min_x()).round(), (baseline + dy).round(), *color);
            }
        }
        // The hovered code block's copy button, over its language; a diagram's "open
        // larger".
        let buttons = copy
            .map(|(_, b)| (b, "copy"))
            .into_iter()
            .chain(self.zoom_button().map(|(_, b)| (b, "maximize-2")));
        for (button, icon) in buttons {
            let r = offset(&button);
            let over = self.hover.is_some_and(|h| button.contains(h));
            let fill = if over { palette.tab_hover } else { palette.editor_background };
            layer.quad(
                Quad::filled(r, fill)
                    .rounded((6.0 * ctx.scale).round())
                    .bordered(ctx.scale.max(1.0), palette.border),
            );
            if let Some(id) = ctx.icons.id(icon) {
                let inset = (6.0 * ctx.scale).round();
                let icon = Rect::new(
                    r.min_x() + inset,
                    r.min_y() + inset,
                    r.size.width - 2.0 * inset,
                    r.size.height - 2.0 * inset,
                );
                let color = if over { palette.text } else { palette.text_muted };
                draw_icon(layer, ctx.icons, id, icon, color);
            }
        }
        // Over the previews' budget: what's been out of sight longest goes.
        budget::frame_done(budget::GPU_BUDGET);
        // The scrollbar (only when the document is taller than the view).
        let (height, scroll) = (layout.height, self.scroll);
        self.vbar.paint(ctx.scene, palette, ctx.scale, b, height, scroll);
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        // Mostly across, over a code block wider than itself: that block scrolls.
        if dx.abs() > dy.abs()
            && let Some((block, _, m)) = self.hscroll_block()
        {
            let to = (m.offset + dx).clamp(0.0, (m.content - m.viewport).max(0.0));
            self.code_scroll.insert(block, to);
            return true;
        }
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
        self.scroll != before
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some(to) = self.vbar.drag(p) {
            let before = self.scroll;
            self.scroll = to.clamp(0.0, self.max_scroll());
            return self.scroll != before;
        }
        if let Some(sel) = &mut self.selecting {
            sel.last = p;
            // A press on a link or a diagram: followed unless the mouse moves away.
            if let Some((from, _)) = &sel.pending {
                let s = self.layout.as_ref().map_or(1.0, |l| l.scale);
                if (p.x - from.x).hypot(p.y - from.y) < 4.0 * s {
                    return false;
                }
                sel.pending = None;
            }
            return self.extend_to(p);
        }
        let (Some(block), Some(bar)) = (self.hbar_block, &self.hbar) else { return false };
        let Some((_, _, m)) = self.scroll_block(block) else { return false };
        match bar.drag_to(p, m) {
            Some(to) => {
                self.code_scroll.insert(block, to.clamp(0.0, (m.content - m.viewport).max(0.0)));
                true
            }
            None => false,
        }
    }

    fn release(&mut self) -> bool {
        if self.vbar.release() {
            return true;
        }
        self.edge_tick = None;
        if let Some(sel) = self.selecting.take() {
            if let Some((_, hit)) = sel.pending {
                self.selection = None;
                self.action = match hit {
                    Hit::Link(url) => self.open_link(&url),
                    Hit::Diagram(index) => self.open_diagram(index),
                    _ => None,
                };
            }
            if self.selection.is_some_and(|s| s.is_empty()) {
                self.selection = None;
            }
            return true;
        }
        self.hbar.as_mut().is_some_and(ui_components::Scrollbar::end_drag)
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press_at(p, 1, ui_components::PressMods::default())
    }

    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.press_at(p, clicks, mods)
    }

    fn is_animating(&self) -> bool {
        self.edge_speed().is_some()
    }

    /// The scrollbar fading in or out: a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.vbar.next_frame()
    }

    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        if !self.bounds.contains(p) {
            return None;
        }
        let local = self.local(p);
        if self.hit_at(local).is_some() {
            return None;
        }
        let layout = self.layout.as_ref()?;
        let shift = |b: usize| self.code_shift(b);
        let over = layout.runs.iter().any(|r| {
            r.shaped.is_some() && {
                let x = local.x + r.scroll.map_or(0.0, shift);
                r.rect.contains(Point::new(x, local.y))
            }
        });
        over.then_some(ui_components::ViewerCursor::Text)
    }

    fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selected()?;
        let text = select::text(&self.layout.as_ref()?.runs, a, b);
        (!text.is_empty()).then_some(text)
    }

    fn select_all(&mut self) -> bool {
        let Some(layout) = &self.layout else { return false };
        self.selection = Some(Selection { anchor: 0, focus: select::total(&layout.runs) });
        true
    }

    fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    fn selects_in_split(&self) -> bool {
        true
    }

    fn source_line_at_top(&self) -> Option<f32> {
        if !settings::sync_scroll() {
            return None;
        }
        if let Some(line) = self.sync_to {
            return Some(line);
        }
        Some(self.layout.as_ref()?.lines.line_at(self.scroll))
    }

    fn scroll_to_source(&mut self, line: f32) -> bool {
        if !settings::sync_scroll() {
            return false;
        }
        match &self.layout {
            Some(l) if l.version == self.version => {
                let before = self.scroll;
                self.scroll = l.lines.y_of(line).clamp(0.0, self.max_scroll());
                (self.scroll - before).abs() > 0.5
            }
            // Not laid out yet (or for an older text): there once it is.
            _ => {
                self.sync_to = Some(line);
                true
            }
        }
    }

    fn reveal_source_line(&mut self, line: f32) -> bool {
        if !settings::sync_scroll() {
            return false;
        }
        let Some(l) = self.layout.as_ref().filter(|l| l.version == self.version) else {
            return false;
        };
        let y = l.lines.y_of(line);
        let h = self.bounds.size.height;
        if y >= self.scroll && y <= self.scroll + h * 0.9 {
            return false;
        }
        self.scroll = (y - h / 3.0).clamp(0.0, self.max_scroll());
        true
    }
    fn hover(&mut self, p: Point) -> bool {
        if !self.bounds.contains(p) {
            self.hover = None;
            return false;
        }
        let local = self.local(p);
        self.hover = Some(local);
        if let Some(bar) = &mut self.hbar {
            bar.hover(p);
        }
        self.vbar.hover(p);
        self.hit_at(local).is_some() || self.copy_button().is_some_and(|(_, b)| b.contains(local))
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        let local = self.local(p);
        let on_selection = self
            .selected()
            .is_some_and(|(a, b)| self.offset_at(local).is_some_and(|at| at > a && at < b));
        let diagram = self.layout.as_ref()?.diagrams.iter().position(|d| d.rect.contains(local));
        let Some(index) = diagram.filter(|_| !on_selection) else {
            self.menu_diagram = None;
            return Some(self.text_menu());
        };
        self.menu_diagram = Some(index);
        let mut menu = vec![
            ui_components::ViewMenuEntry::item("diagram.open", i18n::t("markdown.diagram_open"))
                .with_icon("maximize-2"),
            ui_components::ViewMenuEntry::Separator,
        ];
        menu.extend(view::menu());
        Some(menu)
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "copy" | "markdown.copy" => {
                let Some(text) = self.selected_text() else { return false };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
                return true;
            }
            "markdown.copy_markdown" => {
                let Some((a, b)) = self.selected() else { return false };
                let layout = self.layout.as_ref();
                let Some(text) = layout.and_then(|l| select::markdown(&l.runs, &self.source, a, b))
                else {
                    return false;
                };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
                return true;
            }
            "select_all" | "markdown.select_all" => return self.select_all(),
            _ => {}
        }
        // Opened larger: the one whose menu it was, else the first (asked by name).
        if id == "diagram.open" {
            self.action = self.open_diagram(self.menu_diagram.unwrap_or(0));
            return self.action.is_some();
        }
        let Some(index) = self.menu_diagram else { return false };
        if let Some(path) = id.strip_prefix("picked-save:") {
            let Some(export) = &mut self.export else { return false };
            let done = export.command(&format!("picked-save:{path}"));
            self.settle();
            return done;
        }
        let Some(request) = self.diagram_request(index) else { return false };
        let Some(palette) = &self.built_with else { return false };
        let look = mermaid::Look::of(palette);
        let mut export = view::for_preview(request, look);
        export.set_waker(self.waker.clone());
        let done = export.run(id);
        self.export = Some(export);
        self.settle();
        done
    }

    fn poll(&mut self) -> bool {
        let before = self.action.is_some();
        self.settle();
        self.action.is_some() != before
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.settle();
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn status(&self) -> Option<String> {
        // Counted once per text (the status bar asks at each frame).
        let words = match self.words.get() {
            Some((version, words)) if version == self.version => words,
            _ => {
                let words = self.source.split_whitespace().count();
                self.words.set(Some((self.version, words)));
                words
            }
        };
        Some(i18n::tr!("markdown.words", count = words))
    }
}

impl MarkdownViewer {
    /// A press (`clicks` in a row): a code block's bar or buttons, a checkbox, a fold, a
    /// failing diagram's line act at once; elsewhere a selection starts (by words on a
    /// double click, by blocks on a triple; ⇧ extends the one there is) — on a link or a
    /// diagram, followed at the release if the mouse didn't move.
    fn press_at(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        if !self.bounds.contains(p) {
            return None;
        }
        // The scrollbar's lane takes the press first: its thumb dragged (or jumped to).
        if let Some(to) = self.vbar.press(p) {
            self.scroll = to.clamp(0.0, self.max_scroll());
            return None;
        }
        let local = self.local(p);
        self.hover = Some(local);
        // On a code block's bar across: its thumb dragged (or jumped to).
        if let Some((block, _, m)) = self.hscroll_block()
            && let Some(bar) = &mut self.hbar
            && let Some(to) = bar.begin_drag(p, m)
        {
            self.code_scroll.insert(block, to);
            return None;
        }
        if let Some((i, button)) = self.copy_button()
            && button.contains(local)
        {
            let code = self.layout.as_ref()?.code_blocks[i].1.clone();
            self.notifications
                .push(workbench_model::Notification::success(i18n::t("markdown.code_copied")));
            return Some(ViewerAction::Request(AppRequest::CopyText(code)));
        }
        if let Some((i, button)) = self.zoom_button()
            && button.contains(local)
        {
            return self.open_diagram(i);
        }
        let hit = self.hit_at(local);
        match hit {
            None | Some(Hit::Link(_) | Hit::Diagram(_)) => {
                self.start_selection(p, clicks, mods, hit);
                None
            }
            Some(hit) => self.act(hit),
        }
    }

    /// What a click on `hit` does.
    fn act(&mut self, hit: Hit) -> Option<ViewerAction> {
        match hit {
            Hit::Link(url) => self.open_link(&url),
            Hit::Task(range) => {
                let text = toggle_task(&self.source, range)?;
                Some(ViewerAction::SetText { path: self.path.clone(), text })
            }
            Hit::Fold(index) => {
                if !self.toggled.remove(&index) {
                    self.toggled.insert(index);
                }
                self.dirty = true;
                None
            }
            Hit::Line(line) => {
                Some(ViewerAction::OpenAt { path: self.path.clone(), line: line as u32 })
            }
            Hit::Diagram(index) => self.open_diagram(index),
        }
    }

    /// A selection starts at `p` (window coordinates).
    fn start_selection(
        &mut self,
        p: Point,
        clicks: u32,
        mods: ui_components::PressMods,
        hit: Option<Hit>,
    ) {
        let Some(at) = self.offset_at(self.local(p)) else { return };
        let Some(layout) = &self.layout else { return };
        let runs = &layout.runs;
        let (grain, selection) = match clicks {
            2 => {
                let (a, b) = select::word_at(runs, at);
                (Grain::Word(a, b), Selection { anchor: a, focus: b })
            }
            3.. => {
                let (a, b) = select::block_at(runs, at);
                (Grain::Block(a, b), Selection { anchor: a, focus: b })
            }
            _ => match self.selection {
                Some(s) if mods.shift => (Grain::Char, Selection { anchor: s.anchor, focus: at }),
                _ => (Grain::Char, Selection::at(at)),
            },
        };
        let pending = hit.filter(|_| clicks == 1 && !mods.shift).map(|h| (p, h));
        self.selection = Some(selection);
        self.selecting = Some(Selecting { grain, pending, last: p });
    }

    /// The selection grown to the mouse at `p` (window coordinates; above or below the
    /// view: what's there, out of sight), by its grain. `true` if it changed.
    fn extend_to(&mut self, p: Point) -> bool {
        let Some(grain) = self.selecting.as_ref().map(|s| s.grain) else { return false };
        let b = self.bounds;
        let p = Point::new(p.x.clamp(b.min_x(), b.max_x()), p.y);
        let Some(at) = self.offset_at(self.local(p)) else { return false };
        let Some(layout) = &self.layout else { return false };
        let old = self.selection.unwrap_or_default();
        let grow = |(a, b): (usize, usize), (wa, wb): (usize, usize)| {
            if wa < a {
                Selection { anchor: b, focus: wa }
            } else {
                Selection { anchor: a, focus: wb.max(b) }
            }
        };
        let new = match grain {
            Grain::Char => Selection { anchor: old.anchor, focus: at },
            Grain::Word(a, b) => grow((a, b), select::word_at(&layout.runs, at)),
            Grain::Block(a, b) => grow((a, b), select::block_at(&layout.runs, at)),
        };
        let changed = self.selection != Some(new);
        self.selection = Some(new);
        changed
    }

    /// The right-click menu over the text: Copy and Copy as Markdown (with something
    /// selected), Select All.
    fn text_menu(&self) -> Vec<ui_components::ViewMenuEntry> {
        use ui_components::ViewMenuEntry;
        let keys = |id: &str, entry: ViewMenuEntry| match ui_components::shortcuts::label(id) {
            Some(k) => entry.with_shortcut(k),
            None => entry,
        };
        let mut menu = Vec::new();
        if self.selected().is_some() {
            let copy =
                ViewMenuEntry::item("markdown.copy", i18n::t("markdown.copy")).with_icon("copy");
            menu.push(keys("edit.copy", copy));
            menu.push(ViewMenuEntry::item(
                "markdown.copy_markdown",
                i18n::t("markdown.copy_markdown"),
            ));
            menu.push(ViewMenuEntry::Separator);
        }
        let all = ViewMenuEntry::item("markdown.select_all", i18n::t("markdown.select_all"));
        menu.push(keys("edit.select_all", all));
        menu
    }

    /// The offset under `local` (document coordinates).
    fn offset_at(&self, local: Point) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        let shift = |b: usize| self.code_shift(b);
        Some(select::offset_at(&layout.runs, local, &shift))
    }

    /// How far scrolling code block `block` is moved across.
    fn code_shift(&self, block: usize) -> f32 {
        self.code_scroll.get(&block).copied().unwrap_or(0.0).round()
    }

    /// The selection's offsets, if it holds anything (within the layout's text).
    fn selected(&self) -> Option<(usize, usize)> {
        let total = select::total(&self.layout.as_ref()?.runs);
        let (a, b) = self.selection?.range();
        let (a, b) = (a.min(total), b.min(total));
        (a < b).then_some((a, b))
    }

    /// Selecting with the mouse near the top or bottom (or past them): how fast the
    /// document scrolls there (physical pixels a second, up when negative).
    fn edge_speed(&self) -> Option<f32> {
        let sel = self.selecting.as_ref().filter(|s| s.pending.is_none())?;
        let s = self.layout.as_ref().map_or(1.0, |l| l.scale);
        let (b, y, edge) = (self.bounds, sel.last.y, EDGE * s);
        let depth = if y < b.min_y() + edge {
            -(b.min_y() + edge - y)
        } else if y > b.max_y() - edge {
            y - (b.max_y() - edge)
        } else {
            return None;
        };
        let speed = (depth * 14.0).clamp(-6000.0 * s, 6000.0 * s);
        let room = if speed < 0.0 { self.scroll } else { self.max_scroll() - self.scroll };
        (room > 0.0).then_some(speed)
    }

    /// A frame while a drag holds the mouse near an edge: the document scrolls on, the
    /// selection with it.
    fn edge_scroll(&mut self) {
        let Some(speed) = self.edge_speed() else {
            self.edge_tick = None;
            return;
        };
        let now = std::time::Instant::now();
        let dt =
            self.edge_tick.map_or(1.0 / 60.0, |t| now.duration_since(t).as_secs_f32().min(0.1));
        self.edge_tick = Some(now);
        self.scroll = (self.scroll + speed * dt).clamp(0.0, self.max_scroll());
        if let Some(last) = self.selecting.as_ref().map(|s| s.last) {
            self.extend_to(last);
        }
    }
}

impl MarkdownViewer {
    /// A layout built for `want`: right away when the caches say so (tests) or when there's
    /// nothing to show yet and the text is small (no empty frame on opening); otherwise on
    /// a worker, the layout there is drawn meanwhile. With the caches away (a build on its
    /// way), nothing: what's wanted is built once they're back.
    fn build(&mut self, want: build::Want, text: &mut text_layout::TextSystem) {
        let Some(mut caches) = self.caches.take() else { return };
        self.dirty = false;
        let inline = caches.is_inline()
            || (self.layout.is_none() && self.source.len() <= build::FIRST_INLINE);
        if inline {
            let page = Page {
                source: &self.source,
                version: want.version,
                width: want.width,
                scale: want.scale,
                dir: &self.dir,
                toggled: &self.toggled,
            };
            let layout = Layout::build(page, &mut caches, text, &want.palette);
            let text = self.layout_text.take();
            self.install(build::Built { layout, caches, text }, want.palette);
            return;
        }
        self.caches_ram = caches.memory();
        let input = build::Input {
            source: self.source.clone(),
            dir: self.dir.clone(),
            toggled: self.toggled.clone(),
        };
        let text = self.layout_text.clone();
        self.building = Some(build::start(want, input, caches, text, self.waker.clone()));
    }

    /// The layout built on a worker, put in place once it's there. One that gave nothing
    /// back (it panicked) leaves the caches anew and isn't tried again for the same.
    fn take_built(&mut self) {
        let Some(building) = &self.building else { return };
        let built = match building.rx.try_recv() {
            Ok(built) => Some(built),
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        let Some(building) = self.building.take() else { return };
        match built {
            Some(built) => self.install(built, building.want.palette),
            None => {
                tracing::warn!("a markdown layout could not be built");
                let mut caches = layout::Caches::default();
                caches.set_waker(self.waker.clone());
                self.caches = Some(caches);
                self.failed = Some(building.want);
            }
        }
    }

    /// A layout put in place (built with `palette`): the line at the top stays there (or
    /// the source line asked for comes to the top), what was selected in another text goes,
    /// the pixels of pictures it no longer shows go.
    fn install(&mut self, built: build::Built, palette: ui_components::Palette) {
        let build::Built { layout, caches, text } = built;
        self.caches = Some(caches);
        self.layout_text = text;
        self.failed = None;
        if self.layout.as_ref().is_some_and(|l| l.version != layout.version) {
            self.selection = None;
            self.selecting = None;
        }
        let anchor = self
            .layout
            .as_ref()
            .filter(|_| self.scroll > 0.0)
            .map(|l| l.lines.line_at(self.scroll));
        // Asked for this text: only a layout of it takes it (an older one keeps it waiting).
        let asked = if layout.version == self.version { self.sync_to.take() } else { None };
        if let Some(line) = asked.or(anchor) {
            self.scroll = layout.lines.y_of(line);
        }
        self.drawn.images.keep(&layout);
        self.layout = Some(layout);
        if self.built_with.as_ref() != Some(&palette) {
            self.built_with = Some(palette);
        }
    }
}

/// A block's corner button (document coordinates).
fn corner(r: Rect, s: f32) -> Rect {
    let side = (26.0 * s).round();
    let inset = (6.0 * s).round();
    Rect::new(r.max_x() - side - inset, r.min_y() + inset, side, side)
}

impl MarkdownViewer {
    /// Code block `block` that scrolls across: its view (document coordinates) and its
    /// scroll's measures.
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
