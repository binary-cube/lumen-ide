//! "Canvas" module: the dependency graph around a file, as a canvas you can
//! pan and zoom. On the left, who uses the file; in the center, the file with its classes and
//! functions; on the right, the code of each declaration (the selected one large, with where
//! it is called from) and the files it depends on. Edges link exactly the rows involved.
//!
//! Modes: "Functions" (declarations and their code), "Files" (files only) and "Full
//! code" (files with their first lines of code). Click a file: the canvas centers on
//! it; click a declaration: selects it; click the selected code: opens it in the editor.
//!
//! Drawing is just quads and text (edges are dense dots along a Bézier
//! curve); layout is computed once per graph / mode, not every frame.
//!
//! The module does it all: its command (⌘⇧D) opens the tab, its graph is built here from
//! the project's queries ([`graph`]), a click on a file re-centers it without the app.

mod edit;
mod graph;
mod layout;
mod search;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use ui_components::ds::Pen;

use command_model::{CommandId, CommandSpec};
use workbench_model::{CommandContribution, CommandHandler, ViewAction, WorkbenchHost};

pub use graph::CanvasStart;

use code_intelligence::DepGraph;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, Ctx};
use ui_components::{
    Dock, ToolContent, ToolWindow, ToolWindowProvider, Viewer, ViewerAction, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;

use layout::{Card, CardKind, Layout, Mode};

mod paint;
mod search_paint;
mod view;

/// The files the graph reads (the Task Manager's disk column).
pub(crate) static DISK: workbench_model::DiskIo = workbench_model::DiskIo::new();

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("view.canvas"),
    name: "Canvas",
    description: "The dependency graph between files, classes and functions, with their code",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct CanvasModule;

impl Module for CanvasModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["graph"]));
        ctx.contribute(workbench_model::ModuleCounters { disk: Some(&DISK), traffic: None });
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: OPEN,
                label: "Dependency Canvas",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::NAVIGATE_HIERARCHY,
                    30,
                )),
                shortcuts: &["CmdOrCtrl+Shift+D"],
            },
            icon: Some("graph"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::Project],
                Arc::new(Open),
            ),
        });
        // The zed and sublime presets duplicate the line with ⌘⇧D: no key of the canvas's
        // there.
        for preset in ["zed", "sublime"] {
            ctx.contribute(command_model::bindings::PresetShortcuts {
                preset,
                command: OPEN,
                shortcuts: &[],
            });
        }
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SEARCH,
                label: "Jump on Canvas",
                menu: None,
                shortcuts: &["CmdOrCtrl+K"],
            },
            icon: Some("search"),
            handler: Arc::new(Search),
        });
        for (id, label, shortcuts, action) in ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(ViewAction { view: VIEW, action }),
            });
        }
        // Its button on the right: the canvas opens as a tab (lit while it's the active one).
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: "canvas",
            icon: "graph",
            title_key: "tooltip.canvas",
            dock: Dock::Right,
            order: 30,
            strip_end: false,
            group: "code",
            toggle: Some(OPEN.0),
            content: ToolContent::Command { command: OPEN.0, tab: Some(EXTENSION) },
        }));
        Ok(())
    }
}

/// The canvas's viewer (where its actions' keys work).
const VIEW: &str = "canvas";

/// The canvas's actions (remappable; the keys they always had): the command, its label, its
/// keys, what the canvas is sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 6] = [
    (CommandId("canvas.zoom_in"), "Zoom In (Canvas)", &["=", "Shift+="], "zoom_in"),
    (CommandId("canvas.zoom_out"), "Zoom Out (Canvas)", &["-"], "zoom_out"),
    (CommandId("canvas.fit"), "Fit (Canvas)", &["0"], "fit"),
    (CommandId("canvas.functions"), "Functions (Canvas)", &["1"], "functions"),
    (CommandId("canvas.files"), "Files (Canvas)", &["2"], "files"),
    (CommandId("canvas.code"), "Code (Canvas)", &["3"], "code"),
];

/// Extension of the canvas's virtual path.
const EXTENSION: &str = "view-canvas";
/// Opens the canvas, centered on the active file (its declaration at the cursor).
pub const OPEN: CommandId = CommandId("view.canvas");
/// "Jump to file or function" on the canvas.
pub const SEARCH: CommandId = CommandId("view.canvas_search");

/// Opens the canvas (with a project): its tab, the graph around the active file.
struct Open;

impl CommandHandler for Open {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_root().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(queries) = host.queries() else { return false };
        let focus = host
            .active_file()
            .filter(|f| f.path.extension().is_none_or(|e| e != EXTENSION))
            .map(|f| (f.path, Some(f.line as u32)));
        host.open_view(&t("canvas.title"), EXTENSION, &CanvasStart { queries, focus })
    }
}

/// ⌘K on the canvas: its search.
struct Search;

impl CommandHandler for Search {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.view_command("search")
    }
}

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(Canvas::default()))
    }
}

/// What is under the mouse.
#[derive(Debug, Clone, PartialEq)]
enum Hit {
    Mode(Mode),
    ZoomOut,
    ZoomIn,
    Fit,
    /// Any card: moved by dragging it (a simple click does nothing else).
    Card(CardKind),
    /// The ↗ button of a file card (not the center one): centers the canvas on it.
    File(usize),
    /// The ↗ button of the center file: opens it.
    FocusHeader,
    /// A declaration (row in the center file or its code card).
    Symbol(usize),
    /// The ↗ button of the selected declaration: opens it in the editor.
    Open {
        line: u32,
    },
    /// Code of the selected declaration: editing on the canvas.
    Edit,
    /// The "Jump to file or function" field (⌘K).
    SearchField,
    /// The search × button: clears the text.
    SearchClear,
    /// A search result (index in the list).
    SearchResult(usize),
}

/// Where a file card's rows go: the card, their left edge and height, the code's font.
struct CardLines {
    r: Rect,
    x0: f32,
    row_h: f32,
    code_font: Font,
}

/// Where a function card's code was drawn: its top, left edge and bottom, the rows' height
/// and font, whether it's the text being edited.
struct CodeLines {
    top: f32,
    x: f32,
    bottom: f32,
    row_h: f32,
    font: Font,
    editing: bool,
}

/// Side length of the ↗ button in card headers (at zoom 1).
const NAV: f32 = 26.0;

#[derive(Debug)]
struct Canvas {
    graph: Option<Arc<DepGraph>>,
    mode: Mode,
    selected: Option<usize>,
    layout: Option<Layout>,
    /// Top-left corner of the view, in canvas coordinates.
    pan: Point,
    zoom: f32,
    fit_pending: bool,
    bounds: Rect,
    scale: f32,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    press: Option<Point>,
    /// The card being dragged: last point and whether it moved (else, on release, a click).
    moving: Option<(CardKind, Point, bool)>,
    /// Each edge's dots and bounding box, kept under its shape (its ends, width and dashes
    /// at the zoom drawn): dragging a card makes again only the dots of its own edges.
    edge_dots: Vec<(EdgeShape, EdgeDots)>,
    /// How far the user moved each card (canvas coordinates).
    offsets: HashMap<CardKind, Point>,
    search: Option<search::Search>,
    editing: Option<edit::CodeEdit>,
    /// Action produced by keys, picked up by the app.
    pending: Option<ViewerAction>,
    /// Builds its graphs.
    builder: graph::Builder,
    /// An edit was sent to the app: the graph is built again once it's applied (the next
    /// look), centered there.
    rebuild: Option<(PathBuf, Option<u32>)>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            graph: None,
            mode: Mode::Functions,
            selected: None,
            layout: None,
            pan: Point::new(0.0, 0.0),
            zoom: 1.0,
            fit_pending: true,
            bounds: Rect::default(),
            scale: 1.0,
            hits: Vec::new(),
            hover: None,
            press: None,
            moving: None,
            edge_dots: Vec::new(),
            offsets: HashMap::new(),
            search: None,
            editing: None,
            pending: None,
            builder: graph::Builder::default(),
            rebuild: None,
        }
    }
}

impl Canvas {
    /// Canvas point → screen (physical pixels).
    fn to_screen(&self, p: Point) -> Point {
        let k = self.zoom * self.scale;
        Point::new(
            self.bounds.min_x() + (p.x - self.pan.x) * k,
            self.bounds.min_y() + (p.y - self.pan.y) * k,
        )
    }

    fn rect_to_screen(&self, r: Rect) -> Rect {
        let a = self.to_screen(Point::new(r.min_x(), r.min_y()));
        let k = self.zoom * self.scale;
        Rect::new(a.x.round(), a.y.round(), (r.size.width * k).round(), (r.size.height * k).round())
    }

    fn relayout(&mut self) {
        let lines = self.editing.as_ref().map(edit::CodeEdit::line_count);
        self.layout = self.graph.as_ref().map(|g| {
            let mut layout = Layout::new(g, self.mode, self.selected, lines);
            layout.shift(&self.offsets);
            layout
        });
    }

    /// Fits the graph in the view (zoom between 0.35 and 1).
    fn fit(&mut self) {
        let Some(layout) = &self.layout else { return };
        let b = layout.bounds;
        let view_w = self.bounds.size.width / self.scale;
        let view_h = (self.bounds.size.height / self.scale - 80.0).max(100.0);
        let zoom =
            ((view_w - 80.0) / b.size.width).min((view_h - 40.0) / b.size.height).clamp(0.35, 1.0);
        self.zoom = zoom;
        self.pan = Point::new(
            b.min_x() + b.size.width / 2.0 - view_w / zoom / 2.0,
            b.min_y() + b.size.height / 2.0 - (view_h / zoom) / 2.0 - 10.0 / zoom,
        );
        self.fit_pending = false;
    }

    /// Zoom around screen point `at`.
    fn zoom_at(&mut self, factor: f32, at: Point) -> bool {
        let before = self.zoom;
        let zoom = (self.zoom * factor).clamp(0.25, 2.0);
        if (zoom - before).abs() < f32::EPSILON {
            return false;
        }
        let k0 = before * self.scale;
        let k1 = zoom * self.scale;
        let gx = self.pan.x + (at.x - self.bounds.min_x()) / k0;
        let gy = self.pan.y + (at.y - self.bounds.min_y()) / k0;
        self.pan = Point::new(
            gx - (at.x - self.bounds.min_x()) / k1,
            gy - (at.y - self.bounds.min_y()) / k1,
        );
        self.zoom = zoom;
        true
    }

    fn center(&self) -> Point {
        Point::new(
            self.bounds.min_x() + self.bounds.size.width / 2.0,
            self.bounds.min_y() + self.bounds.size.height / 2.0,
        )
    }

    /// Jump to entry `i` in the catalog: the canvas centers on its file.
    fn jump(&mut self, i: usize) -> Option<ViewerAction> {
        let entry = self.graph.as_ref()?.catalog.get(i)?;
        let (path, line) = (entry.path.clone(), entry.line);
        self.builder.request(path, line);
        None
    }

    /// Shows `graph`: the declaration at the cursor (or the first method) selected.
    fn show(&mut self, graph: Arc<DepGraph>) {
        let graph = &graph;
        let focus = &graph.files[graph.focus];
        self.selected = graph
            .cursor_line
            .and_then(|line| {
                focus
                    .symbols
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| {
                        s.line <= line
                            && line <= s.end_line
                            && matches!(
                                s.kind,
                                code_intelligence::SymbolKind::Method
                                    | code_intelligence::SymbolKind::Function
                            )
                    })
                    .min_by_key(|(_, s)| s.end_line - s.line)
                    .map(|(i, _)| i)
            })
            .or_else(|| {
                focus.symbols.iter().position(|s| {
                    matches!(
                        s.kind,
                        code_intelligence::SymbolKind::Method
                            | code_intelligence::SymbolKind::Function
                    )
                })
            });
        self.editing = None;
        self.offsets.clear();
        self.graph = Some(Arc::clone(graph));
        self.relayout();
        self.fit_pending = true;
    }

    /// Ends editing: the new text (if changed) becomes an action for the app.
    fn finish_edit(&mut self) {
        if let Some(edit) = self.editing.take() {
            self.pending = edit.finish();
            self.relayout();
        }
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        // The most recently added are on top (bottom bar, front cards).
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone())
    }
}

/// An edge's dots (zoomed canvas space) and their bounding box.
type EdgeDots = (Rect, Vec<Point>);

/// What an edge's dots are made from, in zoomed canvas space: its two ends, its width and
/// whether it's dashed (as bits: compared exactly).
type EdgeShape = [u32; 6];

/// The dots of edges `shapes`, those of `kept` reused where an edge's shape is the same at
/// the same place (a drag moves a few edges; the rest stay).
pub(crate) fn update_dots(kept: &mut Vec<(EdgeShape, EdgeDots)>, shapes: &[EdgeShape]) {
    kept.truncate(shapes.len());
    for (i, shape) in shapes.iter().enumerate() {
        if kept.get(i).is_some_and(|(s, _)| s == shape) {
            continue;
        }
        let f = |i: usize| f32::from_bits(shape[i]);
        let dots =
            paint::curve_dots(Point::new(f(0), f(1)), Point::new(f(2), f(3)), f(4), shape[5] != 0);
        match kept.get_mut(i) {
            Some(slot) => *slot = (*shape, dots),
            None => kept.push((*shape, dots)),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
