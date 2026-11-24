//! The project panel, as a module's [`ToolPanel`]: under the header the app draws for it
//! (its buttons: a new file, locate the active file, expand / collapse, hide), the search
//! for files, then the tree — or the search's results while there is text. Everything it
//! knows of the project comes through the project's queries ([`ProjectQueries`]): the
//! folders' entries (the modules' marks included), the paths found, the files' icons.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kernel::{Point, Rect};
use ui_components::{Palette, ToolPanelAction, ViewerAction, ViewerPaint};
use workbench_model::{ActionContext, MenuPlace, ProjectQueries, Target};
use workspace::{DirEntry, FileTree, NodeId, NodeKind};

use crate::{FileSearch, FileSearchStyle, ProjectTreeStyle, ProjectTreeView};

mod entries;
mod listing;
mod search;
mod tool_panel;

/// How many folders / files the search shows.
const FOLDER_RESULTS: usize = 6;
/// Up to this many folders changed are read again at once (an entry made, a save); more (a
/// checkout, an install) are read on the listing's thread, the UI not waiting for them.
const LIST_NOW: usize = 4;
const FILE_RESULTS: usize = 200;

/// The command its "hide" button runs (its registration's toggle).
pub const TOGGLE: &str = workbench_model::TOGGLE_PROJECT_PANEL.0;

/// The project's folders as the tree reads them: through the queries (the disk and what
/// modules know).
struct Listed<'a>(&'a dyn ProjectQueries);

impl workspace::FileSystem for Listed<'_> {
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<DirEntry>> {
        self.0.list_dir(path)
    }

    fn read_file(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

/// Which of its parts has the keyboard (while the panel has it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Keys {
    /// The tree's rows (its files are copied / pasted, deleted).
    #[default]
    Tree,
    Search,
    /// The new entry's name (or an entry's new name), typed in the tree.
    NewEntry,
}

pub struct ProjectPanel {
    queries: Option<Arc<dyn ProjectQueries>>,
    tree: Option<FileTree>,
    view: ProjectTreeView,
    search: FileSearch,
    keys: Keys,
    /// The search's matching, off the UI thread.
    finder: crate::path_search::PathSearch,
    /// The active tab's file (what "locate" shows).
    active: Option<PathBuf>,
    /// The theme its styles were made for.
    styled: Option<Palette>,
    /// What a key or a command asked for (taken by the app).
    action: Option<ToolPanelAction>,
    /// The row pressed (it may be dragged) and where.
    press: Option<(PathBuf, Point)>,
    drag: Option<Drag>,
    /// A press on the search field holds the pointer: a drag selects its text.
    selecting: bool,
    /// The entries changed meanwhile (heard on the project's events), and who to wake for
    /// them; the subscription lives as long as the panel.
    changes: Arc<Mutex<Vec<PathBuf>>>,
    /// A look at everything again was asked: every folder read so far is read again.
    rescan_all: Arc<std::sync::atomic::AtomicBool>,
    /// Batches of folders read on the listing's thread, waiting to be put in the tree (a
    /// batch to open: folders clicked, never read before).
    listed: Arc<Mutex<Vec<Batch>>>,
    /// Batches sent to that thread and not put in yet: while there are, the next go there
    /// too (in order: an older listing never put over a newer one).
    in_flight: usize,
    /// Folders asked to open, still being read (a second click before they come: not
    /// opened after all).
    opening: std::collections::HashSet<PathBuf>,
    /// Folders being read to open that changed on disk meanwhile: read again once in.
    reread: std::collections::HashSet<PathBuf>,
    /// That thread (made at the first large batch; it ends when idle).
    lister: Option<kernel::work::Pool>,
    /// The folders shown, checked when the user comes back to the window.
    away: crate::away::AwayCheck,
    /// The project's events (the folders found changed are told there, as the watcher would).
    events: Option<kernel::EventBus>,
    waker: Arc<Mutex<Option<Waker>>>,
    subscriptions: Vec<kernel::Subscription>,
    /// A search result was taken: the editor gets the keyboard (a row clicked keeps it).
    hand_over: bool,
    /// The new path of an entry renamed here: selected once the tree shows it.
    renamed: Option<PathBuf>,
    /// The files' icons by name: asked for every visible row on every frame.
    file_icons: std::cell::RefCell<ui_components::tree::FileIcons>,
}

type Waker = Arc<dyn Fn() + Send + Sync>;

/// A folder read again, with what it holds now.
type Listing = (PathBuf, std::io::Result<Vec<DirEntry>>);

/// Whether a batch read on the listing's thread opens its folders once put in (a click on
/// one never read), or only reads them again (changes on disk).
type Opening = bool;

/// Folders read together on the listing's thread, and whether they open.
type Batch = (Opening, Vec<Listing>);

/// An entry dragged: by the cursor, onto the folder it would go into.
#[derive(Debug)]
struct Drag {
    path: PathBuf,
    title: String,
    is_dir: bool,
    at: Point,
    into: Option<PathBuf>,
}

impl std::fmt::Debug for ProjectPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectPanel")
            .field("root", &self.tree.as_ref().map(FileTree::root_path))
            .field("keys", &self.keys)
            .finish_non_exhaustive()
    }
}

impl Default for ProjectPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// The tree's look on a side panel's card.
fn tree_style(palette: &Palette) -> ProjectTreeStyle {
    ProjectTreeStyle {
        background: palette.sidebar_background,
        ..ProjectTreeStyle::from_palette(palette)
    }
}

/// Opens file `path` (at `place`: a 0-based line and column).
fn open(path: PathBuf, place: Option<(u32, u32)>) -> ToolPanelAction {
    ToolPanelAction::View(ViewerAction::OpenFile {
        path,
        line: place.map(|(l, _)| l + 1),
        column: place.map(|(_, c)| c + 1),
    })
}

impl ProjectPanel {
    pub fn new() -> Self {
        Self {
            queries: None,
            tree: None,
            view: ProjectTreeView::new(ProjectTreeStyle::default()),
            search: FileSearch::new(FileSearchStyle::default()),
            keys: Keys::Tree,
            finder: Default::default(),
            active: None,
            styled: None,
            action: None,
            press: None,
            drag: None,
            selecting: false,
            changes: Arc::default(),
            rescan_all: Arc::default(),
            listed: Arc::default(),
            in_flight: 0,
            opening: Default::default(),
            reread: Default::default(),
            lister: None,
            away: Default::default(),
            events: None,
            waker: Arc::default(),
            subscriptions: Vec::new(),
            hand_over: false,
            renamed: None,
            file_icons: std::cell::RefCell::default(),
        }
    }

    /// The project shown (its root), if one is open.
    pub fn root(&self) -> Option<&Path> {
        self.tree.as_ref().map(FileTree::root_path)
    }

    /// The tree (for what reads it: tests, the menu's target).
    pub fn tree(&self) -> Option<&FileTree> {
        self.tree.as_ref()
    }

    /// The path of the selected row.
    pub fn selection(&self) -> Option<PathBuf> {
        Some(self.tree.as_ref()?.path(self.view.selected()?))
    }

    /// Rows `ids` as what an action acts on (each with its mark), in the place of the first:
    /// a file's or a folder's. `None`: no rows.
    fn context_of(tree: &FileTree, ids: &[NodeId]) -> Option<ActionContext> {
        let target = |id: NodeId| {
            let is_dir = tree.kind(id) == NodeKind::Directory;
            Target::new(tree.path(id), is_dir).with_mark(tree.mark(id), tree.is_absent(id))
        };
        let first = *ids.first()?;
        let place = if tree.kind(first) == NodeKind::Directory {
            MenuPlace::TreeFolder
        } else {
            MenuPlace::TreeFile
        };
        Some(ActionContext::new(place, ids.iter().map(|id| target(*id)).collect()))
    }

    /// Shows `path` in the tree (its folders opened), selected. `false`: not in it.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries) else { return false };
        match workspace::reveal_path(tree, path, &Listed(queries.as_ref())) {
            Ok(Some(node)) => {
                self.view.reveal(node, tree);
                true
            }
            _ => false,
        }
    }

    /// The parts' bounds in `content` (below the header).
    /// The pressed entry dragged to `p`: past a few pixels it follows the cursor, the folder
    /// under it (a file's own folder) lit as where it would go — not into itself, nor where
    /// it is. `true`: a redraw.
    fn drag_to(&mut self, p: Point, scale: f32) -> bool {
        let Some((path, start)) = self.press.clone() else { return false };
        let moved = (p.x - start.x).abs().max((p.y - start.y).abs());
        if self.drag.is_none() && moved < 6.0 * scale {
            return false;
        }
        let Some(tree) = &mut self.tree else { return false };
        if path == tree.root_path() {
            return false;
        }
        let node = self.view.node_at(p, tree).map(|id| {
            let at = tree.path(id);
            if tree.kind(id) == NodeKind::Directory {
                return (id, at);
            }
            let dir = at.parent().map(Path::to_path_buf).unwrap_or(at);
            (tree.find(&dir).unwrap_or(id), dir)
        });
        let node =
            node.filter(|(_, dir)| !dir.starts_with(&path) && path.parent() != Some(dir.as_path()));
        self.view.set_drop_target(node.as_ref().map(|(id, _)| *id));
        let is_dir = tree.find(&path).is_some_and(|id| tree.kind(id) == NodeKind::Directory);
        let title = kernel::paths::file_name(&path);
        self.drag = Some(Drag { path, title, is_dir, at: p, into: node.map(|(_, dir)| dir) });
        true
    }

    /// The dragged entry's pill by the cursor.
    fn paint_drag(
        &self,
        ctx: &mut ViewerPaint<'_>,
        file_icon: &dyn Fn(&str) -> (icons::IconId, kernel::Rgba),
    ) {
        use ui_components::{drag_card as ds_card, ds};
        let Some(drag) = &self.drag else { return };
        let c = ds::Ctx::new(ctx.palette, ctx.scale);
        let layer = ctx.scene.push_layer(None);
        let (id, color) = if drag.is_dir {
            (ctx.icons.id_or_file("folder"), ctx.palette.folder_icon)
        } else {
            file_icon(&drag.title)
        };
        let lead = ds_card::Lead::Icon(id, color);
        ds_card::paint(layer, &c, ctx.text, ctx.icons, lead, &drag.title, drag.at);
    }

    fn layout(&mut self, content: Rect, scale: f32) {
        let search_h = self.search.field_area_height(scale);
        let area = Rect::new(content.min_x(), content.min_y(), content.size.width, search_h);
        // Clear of the card's rounded bottom.
        let radius = (8.0 * scale).round();
        let list = Rect::new(
            content.min_x(),
            area.max_y(),
            content.size.width,
            (content.size.height - search_h - radius).max(0.0),
        );
        self.search.set_bounds(area, list, scale);
        self.view.set_bounds(list, scale);
    }

    fn restyle(&mut self, palette: &Palette) {
        if self.styled.as_ref() == Some(palette) {
            return;
        }
        self.view.set_style(tree_style(palette));
        self.view.set_palette(palette);
        self.search.set_style(FileSearchStyle::from_palette(palette));
        self.styled = Some(palette.clone());
        self.file_icons.get_mut().clear();
    }
}

#[cfg(test)]
#[path = "../../tests/unit/panel.rs"]
mod tests;
