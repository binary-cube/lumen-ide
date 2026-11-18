//! Diagrams module: "Show Diagram" draws the type at the cursor (class, interface, trait,
//! enum) with its members, what it extends / implements / uses up the chain, its direct
//! children and the types its constructor takes — like a UML class diagram, in a tab.
//!
//! The diagram is built on a background thread by the project's queries (the symbol index)
//! and laid out once; drawing is quads and text. Click a type: the diagram centers on it
//! (← goes back); click a member or the centered type's name: its code opens. Drag or
//! scroll to pan, pinch / ⌘-scroll to zoom, `0` fits it in view.

mod layout;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use ui_components::ds::Pen;

use code_intelligence::{ClassDiagram, DiagramNode, DiagramRole, Relation, SymbolKind, Visibility};
use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use ui_components::diagram::{Card, CardFonts, History, ROW_H, Release, Viewport};
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{
    Palette, Viewer, ViewerAction, ViewerCursor, ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;
use workbench_model::{
    CommandContribution, CommandHandler, ContextMenuItem, MenuPlace, MenuSection, ProjectQueries,
    ViewAction, WorkbenchHost,
};

use crate::layout::Layout;

mod paint;
mod view;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("view.diagrams"),
    name: "Diagrams",
    description: "The type at the cursor with its parents, interfaces, traits, children and dependencies, as a diagram",
    depends_on: &[],
};

pub const SHOW_DIAGRAM: CommandId = workbench_model::SHOW_TYPE_DIAGRAM;

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-diagram";

#[derive(Debug, Default)]
pub struct DiagramsModule;

impl Module for DiagramsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["diagram"]));
        ctx.contribute(languages::IconSvg {
            name: "diagram",
            svg: include_str!("../svg/diagram.svg"),
        });
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SHOW_DIAGRAM,
                label: "Show Diagram",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::NAVIGATE_HIERARCHY,
                    20,
                )),
                shortcuts: &["CmdOrCtrl+Alt+Shift+U"],
            },
            icon: Some("diagram"),
            handler: workbench_model::Requiring::new(
                &[command_model::Requires::TextFile, command_model::Requires::FileOnDisk],
                Arc::new(ShowDiagram),
            ),
        });
        // Where the file's language has types to draw.
        ctx.contribute(
            ContextMenuItem::new(SHOW_DIAGRAM, &[MenuPlace::Editor], MenuSection::Navigate, 30)
                .when(workbench_model::MenuWhen::Provides(
                    code_intelligence::LanguageFeature::TypeDiagram,
                )),
        );
        for (id, label, shortcuts, action) in ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(ViewAction { view: VIEW, action }),
            });
        }
        Ok(())
    }
}

/// The diagram's viewer (where its actions' keys work).
const VIEW: &str = "diagram";

/// The diagram's actions (remappable): the command, its label, its keys, what the view is
/// sent. "Back" had a key that never reached it (⌫); it has it now.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 2] = [
    (CommandId("diagram.fit"), "Fit (Diagram)", &["0"], "fit"),
    (CommandId("diagram.back"), "Back (Diagram)", &["Backspace"], "back"),
];

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
        Ok(Box::new(DiagramView::default()))
    }
}

/// What the view is opened with: the project's queries and the type to draw.
#[derive(Clone)]
pub struct DiagramData {
    pub queries: Arc<dyn ProjectQueries>,
    pub fqn: String,
}

impl std::fmt::Debug for DiagramData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagramData").field("fqn", &self.fqn).finish_non_exhaustive()
    }
}

struct ShowDiagram;

impl CommandHandler for ShowDiagram {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(fqn) = host.type_at_cursor() else {
            host.notify(workbench_model::Notification::info(t("diagram.no_type")));
            return false;
        };
        let Some(queries) = host.queries() else { return false };
        let name = code_intelligence::short_name(&fqn).to_owned();
        host.open_view(&tr!("diagram.tab", name = name), EXTENSION, &DiagramData { queries, fqn })
    }
}

/// What a point in the view is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Back,
    /// A type's header (its card, for the centered one: its name).
    Card(usize),
    /// A member row: node, member.
    Member(usize, usize),
}

pub struct DiagramView {
    queries: Option<Arc<dyn ProjectQueries>>,
    fqn: String,
    diagram: Option<Arc<ClassDiagram>>,
    layout: Layout,
    building: Option<Receiver<Option<ClassDiagram>>>,
    cancel: Arc<AtomicBool>,
    /// The types centered before (← goes back).
    history: History<String>,
    /// Nothing to show: the type isn't indexed.
    missing: bool,
    /// Pan, zoom, fit, the press told from a drag.
    view: Viewport,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    /// What the last click decided (taken by the app).
    pending: Option<ViewerAction>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for DiagramView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagramView").field("fqn", &self.fqn).finish_non_exhaustive()
    }
}

impl Drop for DiagramView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// The toolbar's height (logical pixels).
const TOOLBAR_H: f32 = 52.0;

impl Default for DiagramView {
    fn default() -> Self {
        Self {
            queries: None,
            fqn: String::new(),
            diagram: None,
            layout: Layout::default(),
            building: None,
            cancel: Arc::default(),
            history: History::default(),
            missing: false,
            view: Viewport::new(TOOLBAR_H),
            hits: Vec::new(),
            hover: None,
            pending: None,
            waker: None,
        }
    }
}

impl DiagramView {
    /// The diagram shown (tests, snapshots).
    pub fn diagram(&self) -> Option<&ClassDiagram> {
        self.diagram.as_deref()
    }

    /// Shows `diagram` (from the background build, or directly in tests).
    pub fn set_diagram(&mut self, diagram: Option<ClassDiagram>) {
        self.missing = diagram.is_none();
        self.layout = diagram.as_ref().map(Layout::new).unwrap_or_default();
        self.diagram = diagram.map(Arc::new);
        self.view.fit_pending = true;
    }

    /// Builds type `fqn`'s diagram in the background.
    fn build(&mut self, fqn: String) {
        self.fqn.clone_from(&fqn);
        let Some(queries) = self.queries.clone() else { return };
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("diagram", move || {
            let diagram = queries.class_diagram(&fqn);
            if !cancel.load(Ordering::Relaxed)
                && tx.send(diagram).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.building = Some(rx);
    }

    /// Centers the diagram on another type (the current one goes in the history).
    fn refocus(&mut self, fqn: String) {
        if fqn.eq_ignore_ascii_case(&self.fqn) {
            return;
        }
        let current = std::mem::take(&mut self.fqn);
        self.history.push(current);
        self.build(fqn);
    }

    fn back(&mut self) -> bool {
        let Some(previous) = self.history.pop() else { return false };
        self.build(previous);
        true
    }

    fn k(&self) -> f32 {
        self.view.k()
    }

    /// The area under the toolbar.
    fn area(&self) -> Rect {
        self.view.area()
    }

    fn rect_to_screen(&self, r: Rect) -> Rect {
        self.view.rect_to_screen(r)
    }

    /// Fits the diagram in view (at most its natural size); one too large to stay readable
    /// is shown around the centered type instead.
    fn fit(&mut self) {
        let focus = self.layout.cards.first().map(|c| c.rect);
        self.view.fit(self.layout.bounds, focus, 0.6, 1.0);
    }

    fn zoom_at(&mut self, factor: f32, at: Point) -> bool {
        self.view.zoom_at(factor, at, 0.2, 2.5)
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// What a click on `hit` does.
    fn activate(&mut self, hit: Hit) -> Option<ViewerAction> {
        let diagram = self.diagram.clone()?;
        match hit {
            Hit::Back => {
                self.back();
                None
            }
            Hit::Card(i) => {
                let node = diagram.nodes.get(i)?;
                if i == 0 {
                    // The centered type: its code.
                    let path = node.path.clone()?;
                    return Some(ViewerAction::OpenAt { path, line: node.line });
                }
                // Another indexed type: the diagram centers on it.
                node.path.is_some().then(|| self.refocus(node.fqn.clone()));
                None
            }
            Hit::Member(i, m) => {
                let node = diagram.nodes.get(i)?;
                let member = node.members.get(m)?;
                Some(ViewerAction::OpenAt { path: node.path.clone()?, line: member.line })
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
