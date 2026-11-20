//! A `.drawio` file's tab: the file read on a worker, its page drawn — the sheet and its
//! grid, the shapes and edges from their tiles, the texts by the text system over them — and
//! its pages as tabs at the bottom. Panned with the wheel (or ⌥ and a drag), zoomed around
//! the mouse (fitted to the view when it opens); edited with the mouse and the keys
//! ([`gesture`]), each edit a step of its history, saved over the file. Read again when the
//! file changes on disk (what it already shows changes nothing; unsaved changes are kept).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use diagram_model::{Diagram, Document};
use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use text_layout::TextSystem;
use ui_components::ds::{self, Ctx};
use ui_components::{PressMods, ViewMemory, Viewer, ViewerAction, ViewerCursor, ViewerPaint};
use ui_kit::draw_text;

use crate::camera::Camera;
use crate::drawing::{Drawing, Text, Theme, draw};
use crate::tiles::{Place, Tiles};

mod clipboard;
mod connect;
mod draw;
mod export;
mod gesture;
mod menu;
mod palette;
mod parts;
mod properties;
mod quick;
mod text;
mod toolbar;

use gesture::Gesture;

type Waker = Arc<dyn Fn() + Send + Sync>;

/// What a page's drawing was made from: the document's version, the page, the theme's colors.
type DrawnFrom = (u64, usize, [[u8; 4]; 4]);

/// A file read on the worker: its text's fingerprint (hashed there: the text never comes to
/// the UI thread) and its document, or why it can't be read.
type Read = Result<(u64, Document), String>;

/// The texts laid out for (a drawing's generation, a zoom): each one's lines and their height.
type LaidTexts = ((u64, u64), Vec<(i32, Vec<Option<Laid>>)>);

/// A text laid out: its lines, their height.
type Laid = Arc<(Vec<crate::texts::Laid>, f32)>;

/// The text steps kept laid out (a zoom back and forth lays nothing out again).
const LAID_STEPS: usize = 3;

/// The page tabs' bar's height (logical pixels).
const TABS_H: f32 = 30.0;

/// A `.drawio` file's view.
pub struct DiagramView {
    path: PathBuf,
    /// The worker's answer, taken at the next poll.
    reading: Arc<Mutex<Option<(u64, Read)>>>,
    /// The last read started (or a save since): an older read arriving later is let go.
    reads: u64,
    /// Being read (the first time, or again).
    loading: bool,
    diagram: Option<Diagram>,
    /// What the file held when last read (read again the same: nothing changes).
    text: Option<u64>,
    error: Option<String>,
    page: usize,
    /// The page drawn, and what it was drawn from.
    drawing: Option<(Arc<Drawing>, DrawnFrom)>,
    generation: u64,
    camera: Camera,
    /// Fitted to the view once its first page is drawn.
    fitted: bool,
    tiles: Tiles,
    waker: Arc<Mutex<Option<Waker>>>,
    bounds: Rect,
    scale: f32,
    /// The page tabs' boxes (from the last paint).
    tabs: Vec<(Rect, usize)>,
    /// The cells chosen (their ids), the first the one acted on.
    selection: Vec<String>,
    /// The mouse pressed and moving.
    gesture: Option<Gesture>,
    /// Locked (read-only): cells are chosen, never changed.
    locked: bool,
    /// A cell's text being edited in place.
    editing: Option<text::Editing>,
    /// The shape under the mouse (its connection points shown).
    hovered: Option<String>,
    /// The shapes palette on the left.
    palette: palette::Palette,
    show_palette: bool,
    /// The properties panel on the right.
    props: properties::Props,
    show_properties: bool,
    /// The bar's buttons (from the last paint).
    tools: Vec<(Rect, toolbar::Tool)>,
    /// The quick style bar as last painted (its area and buttons): what the mouse is over
    /// is read from it, never worked out again at each move.
    quick_bar: Option<(Rect, Vec<(Rect, quick::Quick)>)>,
    /// The drawing tool chosen (Select: the usual), and whether it stays after a shape.
    draw: draw::Draw,
    draw_kept: bool,
    /// The texts laid out, each when first drawn, for (the drawing's generation, the zoom,
    /// the font).
    laid: LaidTexts,
    /// The page's shapes counted (for the status bar): the version and page they were for.
    shapes: std::cell::Cell<Option<(u64, usize, usize)>>,
    /// The model's memory weighed (for the Task Manager): the version it was for.
    weighed: std::cell::Cell<Option<(u64, u64)>>,
    /// What it tells the user (taken by the app).
    notices: Vec<workbench_model::Notification>,
    /// The export asked for (its format), until where to is chosen.
    export_as: Option<crate::export::Format>,
    /// An export's outcome from its worker, taken at the next poll.
    exported: Arc<Mutex<Option<export::Exported>>>,
    /// What's asked of the app (the "Save As" dialog), taken by it.
    action: Option<ViewerAction>,
    /// Every tile in view was there at the last paint.
    whole: bool,
    /// Its parts' state of use (see [`DiagramView::with_controls`]).
    controls: ui_components::control::Controls,
}

impl std::fmt::Debug for DiagramView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagramView")
            .field("path", &self.path)
            .field("page", &self.page)
            .finish_non_exhaustive()
    }
}

/// The colors a diagram's defaults take in `palette`.
fn theme_of(palette: &ui_components::Palette) -> Theme {
    Theme {
        canvas: palette.editor_background,
        fill: palette.input_background,
        stroke: palette.text_muted,
        text: palette.text,
    }
}

/// A text's fingerprint.
fn fingerprint(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// `path` read and parsed (the size checked first: a huge file isn't read whole).
fn read(path: &std::path::Path) -> Read {
    let len = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > diagram_model::MAX_FILE as u64 {
        return Err(diagram_model::Error::TooLarge.to_string());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let doc = Document::read(&text).map_err(|e| e.to_string())?;
    Ok((fingerprint(&text), doc))
}

impl DiagramView {
    /// The view of `path`, its reading started.
    pub fn open(path: PathBuf) -> Self {
        let mut view = Self {
            path,
            reading: Arc::default(),
            reads: 0,
            loading: false,
            diagram: None,
            text: None,
            error: None,
            page: 0,
            drawing: None,
            generation: 0,
            camera: Camera::default(),
            fitted: false,
            tiles: Tiles::default(),
            waker: Arc::default(),
            bounds: Rect::default(),
            scale: 1.0,
            tabs: Vec::new(),
            selection: Vec::new(),
            gesture: None,
            locked: false,
            editing: None,
            hovered: None,
            palette: palette::Palette::default(),
            show_palette: true,
            draw: draw::Draw::default(),
            quick_bar: None,
            draw_kept: false,
            props: properties::Props::default(),
            show_properties: true,
            tools: Vec::new(),
            shapes: std::cell::Cell::new(None),
            weighed: std::cell::Cell::new(None),
            notices: Vec::new(),
            laid: ((0, 0), Vec::new()),
            export_as: None,
            exported: Arc::default(),
            action: None,
            whole: true,
            controls: ui_components::control::Controls::default(),
        };
        view.start_reading();
        view
    }

    /// The file read on the worker (its answer taken at the next poll).
    fn start_reading(&mut self) {
        self.loading = true;
        self.reads += 1;
        let (path, into, waker) =
            (self.path.clone(), Arc::clone(&self.reading), Arc::clone(&self.waker));
        let this = self.reads;
        kernel::work::run("diagram-read", move || {
            let read = read(&path);
            {
                // Never over a newer read's answer not taken yet.
                let mut slot = into.lock().unwrap_or_else(PoisonError::into_inner);
                if slot.as_ref().is_none_or(|(at, _)| *at < this) {
                    *slot = Some((this, read));
                }
            }
            if let Some(wake) = waker.lock().unwrap_or_else(PoisonError::into_inner).clone() {
                wake();
            }
        });
    }

    /// The read file taken. `true`: something to draw.
    fn take_read(&mut self) -> bool {
        let Some((at, read)) = self.reading.lock().unwrap_or_else(PoisonError::into_inner).take()
        else {
            return false;
        };
        // A read older than the last one started (or than a save): what it holds is old.
        if at != self.reads {
            return false;
        }
        self.loading = false;
        match read {
            Ok((print, doc)) => {
                if self.text == Some(print) {
                    return false;
                }
                // Changed on disk under unsaved changes: they're kept (a save writes over it).
                if self.diagram.as_ref().is_some_and(Diagram::is_dirty) {
                    let name = self
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.notices.push(workbench_model::Notification::warning(tr!(
                        "diagram_editor.changed_on_disk",
                        name = name
                    )));
                    return false;
                }
                self.text = Some(print);
                self.page = self.page.min(doc.pages.len().saturating_sub(1));
                self.diagram = Some(Diagram::new(doc));
                self.error = None;
                self.drawing = None;
            }
            Err(why) => {
                tracing::warn!(path = %self.path.display(), error = %why, "a diagram can't be read");
                self.error = Some(why);
            }
        }
        true
    }

    /// The page drawn for the palette (made again when the document, the page or the theme
    /// changed).
    fn drawing(&mut self, palette: &ui_components::Palette) -> Option<Arc<Drawing>> {
        let diagram = self.diagram.as_ref()?;
        let page = diagram.doc.pages.get(self.page)?;
        let theme = theme_of(palette);
        let colors = [theme.canvas, theme.fill, theme.stroke, theme.text].map(Rgba::to_rgba8);
        let key = (diagram.version, self.page, colors);
        if let Some((drawing, made_for)) = &self.drawing
            && *made_for == key
        {
            return Some(Arc::clone(drawing));
        }
        let drawing = Arc::new(draw(page, &theme));
        // What's chosen and gone (deleted, undone) let go.
        if !self.selection.is_empty() {
            let there: std::collections::HashSet<&str> = drawing
                .boxes
                .iter()
                .map(|(b, _)| b.as_str())
                .chain(drawing.ways.iter().map(|(w, _)| w.as_str()))
                .collect();
            self.selection.retain(|id| there.contains(id.as_str()));
        }
        // A new generation: its tiles made as it's drawn, the last whole set shown meanwhile;
        // after an edit (the same page, the same colors), only the tiles it touches.
        if let Some((before, (_, page, colors))) = &self.drawing
            && (*page, *colors) == (key.1, key.2)
        {
            self.tiles.carry(before, self.generation, &drawing, self.generation + 1);
        }
        self.generation += 1;
        self.drawing = Some((Arc::clone(&drawing), key));
        Some(drawing)
    }

    /// The sheet's area (above the page tabs).
    fn sheet(&self) -> Rect {
        let tabs = TABS_H * self.scale;
        let top = self.toolbar_area().max_y();
        let left =
            if self.show_palette { self.palette_area().max_x() } else { self.bounds.min_x() };
        let right = self.properties_area().min_x();
        Rect::new(left, top, (right - left).max(0.0), (self.bounds.max_y() - tabs - top).max(0.0))
    }

    /// The pages (their names).
    fn page_names(&self) -> Vec<String> {
        self.diagram.as_ref().map_or_else(Vec::new, |d| {
            d.doc
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    if p.name().is_empty() {
                        tr!("diagram_editor.page", n = (i + 1) as i64)
                    } else {
                        p.name().to_owned()
                    }
                })
                .collect()
        })
    }

    /// Page `page` shown.
    pub fn show_page(&mut self, page: usize) -> bool {
        let count = self.diagram.as_ref().map_or(0, |d| d.doc.pages.len());
        if page >= count || page == self.page {
            return false;
        }
        self.end_text();
        self.page = page;
        self.fitted = false;
        true
    }

    fn paint_message(&self, ctx: &mut ViewerPaint<'_>, message: &str) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let font = ds.ui_font(0.0);
        let layer = ctx.scene.push_layer(Some(self.bounds));
        let shaped = ctx.text.shape_line(message, &font);
        let x = (self.bounds.center().x - shaped.width / 2.0).round();
        let y = (self.bounds.center().y + font.size / 2.0).round();
        draw_text(layer, &shaped, x, y, ctx.palette.text_muted);
    }

    /// The grid, its lines `grid` diagram units apart (none when they'd be too close).
    fn paint_grid(&self, layer: &mut render_api::Layer, sheet: Rect, grid: f32, color: Rgba) {
        let k = self.camera.k(self.scale);
        let mut step = grid * k;
        if step <= 0.0 {
            return;
        }
        // Too dense: every few lines.
        while step < 8.0 * self.scale {
            step *= 4.0;
        }
        let origin = self.camera.to_screen(Point::new(0.0, 0.0), sheet, self.scale);
        let line = self.scale.max(1.0);
        let mut x = sheet.min_x() + (origin.x - sheet.min_x()).rem_euclid(step);
        while x < sheet.max_x() {
            layer.quad(Quad::filled(
                Rect::new(x.round(), sheet.min_y(), line, sheet.size.height),
                color,
            ));
            x += step;
        }
        let mut y = sheet.min_y() + (origin.y - sheet.min_y()).rem_euclid(step);
        while y < sheet.max_y() {
            layer.quad(Quad::filled(
                Rect::new(sheet.min_x(), y.round(), sheet.size.width, line),
                color,
            ));
            y += step;
        }
    }

    /// A text drawn at its place (its lines wrapped to its box when it wraps).
    fn paint_text(
        &mut self,
        (font, index): (u64, usize),
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        sheet: Rect,
        family: &text_layout::FontFamily,
        t: &Text,
    ) {
        let k = self.camera.k(self.scale);
        let corner = self.camera.to_screen(t.area.origin, sheet, self.scale);
        let area = Rect::new(corner.x, corner.y, t.area.size.width * k, t.area.size.height * k);
        // Out of view: not laid out. How far past its box a text can reach: at most its
        // characters side by side (an em and a bit each), its words one under another —
        // never more than it was allowed before.
        let size = t.size * k;
        let (across, down) = ((t.reach.0 * size).min(2000.0), (t.reach.1 * size).min(500.0));
        if area.max_x() + across < sheet.min_x()
            || area.min_x() - across > sheet.max_x()
            || area.max_y() + down < sheet.min_y()
            || area.min_y() - down > sheet.max_y()
        {
            return;
        }
        // Laid out once per drawing and zoom step (a pan moves it; a zoom lays it out again
        // only when it crosses a step), the last few steps kept.
        let key = (self.generation, font);
        if self.laid.0 != key {
            self.laid = (key, Vec::new());
        }
        let (step, kq) = crate::texts::text_step(k);
        let steps = &mut self.laid.1;
        match steps.iter().position(|(s, _)| *s == step) {
            // The latest first.
            Some(at) => steps[..=at].rotate_right(1),
            None => {
                steps.insert(0, (step, Vec::new()));
                steps.truncate(LAID_STEPS);
            }
        }
        let cache = &mut steps[0].1;
        if cache.len() <= index {
            cache.resize_with(index + 1, || None);
        }
        let laid = cache[index].get_or_insert_with(|| {
            let at_step = Rect::new(0.0, 0.0, t.area.size.width * kq, t.area.size.height * kq);
            Arc::new(crate::texts::lay(text, family, t, at_step, kq))
        });
        let (lines, line_h) = &**laid;
        crate::texts::draw(layer, lines, *line_h, t, area.origin, (kq, k / kq));
    }

    fn paint_tabs(&mut self, ctx: &mut ViewerPaint<'_>, area: Rect) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let layer = ctx.scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.tab_bar_background));
        layer.quad(Quad::filled(
            Rect::new(area.min_x(), area.min_y(), area.size.width, ctx.scale.max(1.0)),
            p.border,
        ));
        let font = ds.ui_font(-1.0);
        let pad = ds.px(12.0);
        let mut x = area.min_x() + ds.px(8.0);
        self.tabs.clear();
        for (i, name) in self.page_names().into_iter().enumerate() {
            let shaped = ctx.text.shape_line(&name, &font);
            let r = Rect::new(
                x,
                area.min_y() + ds.px(3.0),
                shaped.width + 2.0 * pad,
                area.size.height - ds.px(6.0),
            );
            if r.min_x() > area.max_x() {
                break;
            }
            let on = i == self.page;
            if on {
                layer.quad(Quad::filled(r, p.tab_active_background).rounded(ds.px(6.0)));
            }
            let color = if on { p.text } else { p.text_muted };
            draw_text(
                layer,
                &shaped,
                (r.min_x() + pad).round(),
                (r.center().y + font.size * 0.35).round(),
                color,
            );
            self.tabs.push((r, i));
            x = r.max_x() + ds.px(4.0);
        }
    }
}

impl DiagramView {
    /// What a press does once its part is known (`None`: none took it, the sheet's): a zone's
    /// own, a number typed let go past the bar, the palette's search past the quick bar.
    fn press_part(
        &mut self,
        part: Option<usize>,
        p: Point,
        clicks: u32,
        mods: PressMods,
    ) -> Option<ViewerAction> {
        match part {
            Some(parts::BAR) => {
                self.press_toolbar(p, clicks);
                return None;
            }
            Some(parts::PANEL) => {
                self.press_properties(p);
                return None;
            }
            Some(parts::QUICK) => {
                self.end_typing(false);
                self.press_quick(p);
                return None;
            }
            Some(parts::PALETTE_BUTTONS) => {
                self.end_typing(false);
                self.press_palette(p, clicks);
                return None;
            }
            Some(parts::TABS) => {
                self.leave_fields();
                if let Some(&(_, page)) = self.tabs.iter().find(|(r, _)| r.contains(p))
                    && self.show_page(page)
                {
                    self.selection.clear();
                }
                return None;
            }
            // A field's (the text edited in place).
            Some(_) => return None,
            None => self.leave_fields(),
        }
        if !self.sheet().contains(p) {
            return None;
        }
        // A double click on a cell: its text edited.
        let near = 5.0 / self.camera.k(self.scale);
        let at = self.camera.to_diagram(p, self.sheet(), self.scale);
        if clicks >= 2
            && let Some(d) = self.drawing.as_ref().map(|(d, _)| Arc::clone(d))
            && let Some(id) = crate::edit::hit(&d, at, near)
        {
            self.gesture = None;
            self.edit_text(&id);
            return None;
        }
        // A second click on an arrow (its first made a shape): nothing more.
        if clicks >= 2 && self.arrow_at(p).is_some() {
            return None;
        }
        // On the sheet, where nothing is: a text there (with Select: a drawing tool draws).
        if clicks >= 2 && !self.locked && self.draw == draw::Draw::Select {
            self.gesture = None;
            self.new_text_at(at);
            return None;
        }
        self.press_sheet(p, mods);
        None
    }

    /// The field at part `part`'s place, lent read-only while it's open (the parts
    /// themselves hold it to change it).
    fn field_part(&self, part: usize) -> Option<&dyn ui_components::control::Control> {
        use ui_components::control::Control;
        match part {
            parts::TYPING => self.props.typing.as_ref().map(|(_, input)| input as &dyn Control),
            parts::PALETTE => {
                self.palette.searching.then_some(&self.palette.search as &dyn Control)
            }
            parts::TEXT => self.editing.as_ref().map(text::Editing::field_control),
            _ => None,
        }
    }

    /// A press beside the panel and the palette: the number typed let go, and the palette's
    /// search while it's shown.
    fn leave_fields(&mut self) {
        self.end_typing(false);
        if self.show_palette {
            self.palette.searching = false;
        }
    }
}

impl Viewer for DiagramView {
    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        if let (Some(w), Ok(mut slot)) = (&ctx.waker, self.waker.lock()) {
            *slot = Some(Arc::clone(w));
        }
        self.tiles.set_waker(ctx.waker.as_ref());
        self.bounds = ctx.bounds;
        self.scale = ctx.scale;
        let canvas = ctx.palette.editor_background;
        ctx.scene
            .push_layer(Some(ctx.bounds))
            .quad(Quad::filled(ctx.bounds, canvas).rounded(ds::radius::CARD * ctx.scale));
        if let Some(why) = self.error.clone().filter(|_| self.diagram.is_none()) {
            self.paint_message(&mut ctx, &tr!("diagram_editor.unreadable", reason = why));
            return;
        }
        let Some(drawing) = self.drawing(ctx.palette) else {
            let message =
                if self.loading { t("diagram_editor.loading") } else { t("diagram_editor.empty") };
            self.paint_message(&mut ctx, &message);
            return;
        };
        let sheet = self.sheet();
        if !self.fitted && sheet.size.width > 0.0 {
            self.camera.fit(drawing.bounds, sheet, self.scale);
            self.fitted = true;
        }
        let page = self.diagram.as_ref().and_then(|d| d.doc.pages.get(self.page));
        let (grid, background) = page.map_or((None, None), |p| {
            (p.grid().then(|| p.grid_size() as f32), p.background().and_then(crate::look::color))
        });
        let layer = ctx.scene.push_layer(Some(sheet));
        if let Some(bg) = background {
            layer.quad(Quad::filled(sheet, bg));
        }
        if let Some(grid) = grid {
            self.paint_grid(layer, sheet, grid, ctx.palette.indent_guide);
        }
        let corner = self.camera.to_screen(drawing.bounds.origin, sheet, self.scale);
        let at = Place { corner, k: self.camera.k(self.scale), view: sheet };
        let (masks, whole) = self.tiles.visible(&drawing, self.generation, at);
        self.whole = whole;
        for (tile, stretch, corner) in masks {
            for (image, r) in tile.iter() {
                let place = Rect::new(
                    corner.x + r.min_x() * stretch,
                    corner.y + r.min_y() * stretch,
                    r.size.width * stretch,
                    r.size.height * stretch,
                );
                layer.image(image, place);
            }
        }
        // The texts over the shapes and lines (a layer of their own: its images are drawn
        // before its quads, an edge's label's patch included).
        let family = ctx.palette.ui_font.clone();
        let layer = ctx.scene.push_layer(Some(sheet));
        // The font's identity, once a paint (the texts' layouts are kept per font).
        let font = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            family.hash(&mut h);
            h.finish()
        };
        for (i, text) in drawing.texts.iter().enumerate() {
            self.paint_text((font, i), layer, ctx.text, sheet, &family, text);
        }
        let layer = ctx.scene.push_layer(Some(sheet));
        self.paint_editing(layer, ctx.palette);
        self.paint_connections(layer, ctx.palette, ctx.icons);
        self.paint_quick(ctx.scene, &Ctx::new(ctx.palette, ctx.scale), ctx.icons);
        if self.editing.is_some() {
            let ds = Ctx::new(ctx.palette, ctx.scale);
            self.paint_text_field(ctx.scene, ctx.text, &ds);
        }
        let tabs = Rect::new(
            sheet.min_x(),
            sheet.max_y(),
            sheet.size.width,
            self.bounds.max_y() - sheet.max_y(),
        );
        self.paint_tabs(&mut ctx, tabs);
        let theme = theme_of(ctx.palette);
        if self.show_palette {
            self.paint_palette(&mut ctx, &theme);
        }
        self.paint_properties(&mut ctx);
        self.paint_toolbar(&mut ctx);
    }

    fn poll(&mut self) -> bool {
        self.take_read() | self.tiles.poll() | self.palette.thumbs.poll() | self.take_exported()
    }

    /// Reading the file, or making the tiles in view.
    fn working(&self) -> bool {
        self.loading || !self.whole
    }

    /// The wheel: the part under the mouse's (the palette's sections), else the sheet panned.
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.with_controls(|c, parts| c.scroll_hovered(parts, dx, dy)).is_some() {
            return true;
        }
        if self.diagram.is_none() {
            return false;
        }
        self.camera.pan(-dx, -dy);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.diagram.is_some() && self.camera.zoom_at(factor, at, self.sheet(), self.scale)
    }

    fn zoom_level(&self) -> Option<f32> {
        self.diagram.as_ref().map(|_| self.camera.zoom)
    }

    fn reset_zoom(&mut self) -> bool {
        let center = self.sheet().center();
        self.diagram.is_some()
            && self.camera.zoom_at(1.0 / self.camera.zoom, center, self.sheet(), self.scale)
    }

    /// A press: its parts first (the text edited in place; a press elsewhere ends it), then
    /// the bar, the panel, the quick bar, the palette, the page tabs (each done by the view),
    /// the sheet. Past the bar, the number typed is let go; past the palette, its search.
    fn pointer_down(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        let pressed = self.with_controls(|c, parts| {
            let pressed = c.press_with(parts, p, clicks, mods);
            // A zone's press is beside every field: the text edited in place ends.
            if pressed.is_some_and(|(part, _)| part >= parts::BAR) {
                c.focus(parts, None);
            }
            pressed
        });
        let action = self.press_part(pressed.map(|(part, _)| part), p, clicks, mods);
        // What the press opened or closed (a field, a text) has the keys from now.
        self.sync_focus();
        action
    }

    /// The part holding the pointer follows it, else the gesture.
    fn drag(&mut self, p: Point) -> bool {
        match self.with_controls(|c, parts| c.drag(parts, p)) {
            Some((_, changed)) => changed,
            None => self.drag_to(p),
        }
    }

    fn release(&mut self) -> bool {
        match self.with_controls(|c, parts| c.release(parts)) {
            Some((_, changed)) => changed,
            None => self.finish(),
        }
    }

    /// While a text is edited: its typing.
    fn wants_text(&self) -> bool {
        self.editing.is_some() || self.palette.searching || self.props.typing.is_some()
    }

    /// A text being edited takes several lines (a paste of them).
    fn field_takes_lines(&self) -> bool {
        true
    }

    /// Typing: the part with the keys'.
    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed)).is_some()
    }

    /// The field with the keys: its selection, or nothing (never the cells under it); with
    /// none open, the cells chosen.
    fn selected_text(&self) -> Option<String> {
        if !self.wants_text() {
            return self.copy_chosen();
        }
        self.controls.selected_text_with(|part| self.field_part(part))
    }

    /// A paste: into the part with the keys, else cells.
    fn paste(&mut self, text: &str) -> bool {
        let (taken, focused) = self.with_controls(|c, parts| (c.text(parts, text), c.focused()));
        if focused.is_some() {
            return taken.is_some();
        }
        self.paste_cells(text)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // The page tabs, the quick bar's buttons: the hand.
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        if self.arrow_at(p).is_some() || self.outline_at(p).is_some() {
            return Some(ViewerCursor::Pointer);
        }
        use crate::edit::Handle;
        match self.handle_at(p)? {
            Handle::E | Handle::W => Some(ViewerCursor::ResizeHorizontal),
            Handle::N | Handle::S => Some(ViewerCursor::ResizeVertical),
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn edit_key_with(
        &mut self,
        key: ui_components::ViewerKey,
        reach: ui_components::KeyReach,
    ) -> bool {
        // A part with the keys takes them all (used or not); else they edit the cells.
        let (used, focused) =
            self.with_controls(|c, parts| (c.key(parts, key, reach), c.focused()));
        match (used, focused) {
            (Some(_), _) => true,
            (None, Some(_)) => false,
            (None, None) => self.key_edit(key, reach),
        }
    }

    /// ⌘Z / ⇧⌘Z, ⌘A, ⌘X (after the app copied), ⌘D — a text being edited: its own.
    fn command(&mut self, id: &str) -> bool {
        // A field with the keys (a number typed, the search, a text edited): its own,
        // never the cells'.
        let (used, focused) = self.with_controls(|c, parts| (c.command(parts, id), c.focused()));
        if focused.is_some() {
            return used.is_some();
        }
        if let Some(done) = self.export_command(id) {
            return done;
        }
        match id {
            "cut" => self.delete_chosen(),
            "duplicate" => self.duplicate_chosen(),
            // The first chosen cell's text edited.
            "edit" => self.selection.first().cloned().is_some_and(|id| self.edit_text(&id)),
            "select_all" => self.select_all(),
            "undo" | "redo" if self.locked => false,
            "undo" => self.diagram.as_mut().is_some_and(Diagram::undo),
            "redo" => self.diagram.as_mut().is_some_and(Diagram::redo),
            // A row of its right-click menu.
            other => self.menu_command(other),
        }
    }

    /// ⌘A: every shape and edge of the page.
    fn select_all(&mut self) -> bool {
        let Some(d) = self.drawing.as_ref().map(|(d, _)| Arc::clone(d)) else { return false };
        self.selection = d
            .boxes
            .iter()
            .map(|(id, _)| id.clone())
            .chain(d.ways.iter().map(|(id, _)| id.clone()))
            .collect();
        true
    }

    fn is_dirty(&self) -> bool {
        self.diagram.as_ref().is_some_and(Diagram::is_dirty)
    }

    /// Written over the file (through a temporary file renamed over it); what was written is
    /// what the file holds now (its report from the disk changes nothing).
    fn moved(&mut self, path: &std::path::Path) {
        path.clone_into(&mut self.path);
    }

    fn save(&mut self) -> std::io::Result<()> {
        // A text being typed in place is part of what's saved.
        if self.editing.is_some() {
            self.end_text();
        }
        let Some(d) = self.diagram.as_mut().filter(|d| d.is_dirty()) else { return Ok(()) };
        let text = d.doc.write();
        files::write_atomic(&self.path, text.as_bytes())?;
        d.mark_saved();
        // A read in flight read the file before this save: let go.
        self.reads += 1;
        self.loading = false;
        self.text = Some(fingerprint(&text));
        Ok(())
    }

    fn lockable(&self) -> bool {
        true
    }

    fn set_locked(&mut self, locked: bool) {
        if locked {
            self.end_text();
            self.gesture = None;
        }
        self.locked = locked;
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notices)
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.menu_at(p)
    }

    /// Over a shape: its connection points.
    fn hover(&mut self, p: Point) -> bool {
        let parts = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        self.hover_shape(p) || parts
    }

    /// A palette's shape: its name; a button of the bar or of the quick bar: what it does.
    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        self.controls.tooltip_at(p)
    }

    fn file_changed(&mut self) {
        self.start_reading();
    }

    fn status(&self) -> Option<String> {
        let d = self.diagram.as_ref()?;
        // Counted once per version and page (the bar asks at each of its builds).
        let shapes = match self.shapes.get() {
            Some((version, page, n)) if (version, page) == (d.version, self.page) => n,
            _ => {
                let n = d.doc.pages.get(self.page).map_or(0, |p| {
                    p.cells.iter().filter(|c| c.kind == diagram_model::CellKind::Vertex).count()
                });
                self.shapes.set(Some((d.version, self.page, n)));
                n
            }
        };
        Some(tr!("diagram_editor.status", pages = d.doc.pages.len() as i64, shapes = shapes as i64))
    }

    /// The model (weighed once per version) and the tiles: in memory until they're on the
    /// GPU, then there.
    fn memory(&self) -> ViewMemory {
        let (ram, gpu) = self.tiles.memory();
        let thumbs = self.palette.thumbs.bytes();
        let model = self.diagram.as_ref().map_or(0, |d| match self.weighed.get() {
            Some((version, bytes)) if version == d.version => bytes,
            _ => {
                let bytes = d.doc.pages.iter().map(diagram_model::Page::weight).sum();
                self.weighed.set(Some((d.version, bytes)));
                bytes
            }
        });
        ViewMemory { ram: ram + model, gpu: gpu + thumbs }
    }

    /// Every cache let go (made again as it's drawn): the tiles (their textures), the
    /// drawing, the texts laid out, the palette's thumbnails.
    fn purge(&mut self) {
        self.tiles.clear();
        self.drawing = None;
        self.laid = ((0, 0), Vec::new());
        self.palette.thumbs.clear();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view.rs"]
pub(crate) mod tests;
