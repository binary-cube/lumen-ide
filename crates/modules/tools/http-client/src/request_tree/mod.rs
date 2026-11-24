//! The tree of collections and requests (plan 77l), as the HTTP window shows the whole
//! project's and a collection's tab shows its own: folders (collections, sub-collections:
//! only organization, as the database window's) and their requests, searched, walked with
//! the keys, its footer telling which.
//!
//! As in the project's tree, a new collection is named on a row of its own where it goes,
//! a rename on the folder's row: Enter (or the keyboard going elsewhere, a click away) keeps
//! the name, Esc gives it up. Nothing opens by itself: a collection made is just there, a
//! request made is selected (and, where the tree asks for it, opened beside it). Renames go
//! through the app (its tabs follow); what's made, copied, moved or removed is written on a
//! worker, then the list read again at once (not only when the disk's watch tells).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use i18n::t;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::control::{Control, Press};
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx};
use ui_components::search_field::SearchField;
use ui_components::tool_panel::KeyReach;
use ui_components::tree::{self as tree_row, TreeList};
use ui_components::{
    EdgeScrollbar, ViewMenuEntry, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint, WithHost,
};
use ui_kit::{TextInput, centered_baseline, draw_icon, draw_text};
use workbench_model::{ProjectQueries, WorkbenchHost};

use crate::collections::{self, Listing, NameFor, Row, Slot};
use crate::pending::Pending;

mod changes;
mod input;
mod menus;
mod paint;

/// The worker the list is read on.
pub const THREAD: &str = "http-list";
const ROW_HEIGHT: f32 = 26.0;
const INDENT: f32 = 16.0;
const PADDING: f32 = 12.0;
const FIELD_TOP: f32 = 8.0;
const FIELD_HEIGHT: f32 = 30.0;
const LIST_TOP: f32 = FIELD_TOP + FIELD_HEIGHT + 6.0;
const FOOTER: f32 = 36.0;
/// A button beside the search (a new request, a new collection).
const BUTTON: f32 = ds::button_size::ROOMY;
/// The edit command choosing a whole field (it changes nothing found).
const SELECT_ALL: &str = "select_all";

/// A new request, as written at a file's end.
pub const TEMPLATE: &str = "### A request\nGET https://example.com\nAccept: application/json\n";

/// A request chosen in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    /// Its file (absolute) and its place in it.
    pub file: PathBuf,
    pub index: usize,
    pub label: String,
    /// The collection it's in (relative: see [`collections::collection_of`]).
    pub collection: String,
}

/// What the tree asks of whoever shows it.
#[derive(Debug)]
pub enum TreeAction {
    /// What a view asks (a file opened, work for the host).
    View(ViewerAction),
    /// A request chosen (a click, ↵).
    Open(Opened),
    /// A collection opened (a double click, its menu): its folder, relative.
    OpenCollection(String),
    /// Another client's collection to import (its file picked), one from a cURL command on
    /// the clipboard.
    Import,
    ImportCurl,
}

/// A button beside the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    NewRequest,
    NewCollection,
    Environments,
    Import,
}

impl Button {
    const ALL: [Self; 4] =
        [Self::NewRequest, Self::NewCollection, Self::Environments, Self::Import];

    fn icon(self) -> &'static str {
        match self {
            Self::NewRequest => "plus",
            Self::NewCollection => "folder-plus",
            Self::Environments => "file-json",
            Self::Import => "download",
        }
    }

    fn tooltip(self) -> String {
        t(match self {
            Self::NewRequest => "http.new_request",
            Self::NewCollection => "http.new_collection",
            Self::Environments => "http.edit_environments",
            Self::Import => "http.import_collection",
        })
    }
}

/// What a name typed on a row is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Naming {
    /// A new collection in folder `parent` (relative).
    NewCollection { parent: String },
    /// Collection `path` renamed.
    Rename { path: String },
}

/// What the tree made, to be found once it's read again: a request (its file, relative, and
/// its place in it) or a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Made {
    Request(String, usize),
    Folder(String),
}

#[derive(Default)]
pub struct RequestTree {
    /// The folder it shows (`""`: the whole project).
    within: String,
    /// Buttons beside the search (a collection's tab has no header of its own).
    buttons: bool,
    /// A request made is opened too (the tab opens it beside the tree).
    open_made: bool,
    queries: Option<Arc<dyn ProjectQueries>>,
    project: Option<kernel::ProjectId>,
    generation: Option<u64>,
    /// Something was written by it: read again whatever the generation.
    stale: Arc<AtomicBool>,
    made: Arc<Mutex<Option<Made>>>,
    listing: Arc<Listing>,
    reading: Pending<(u64, Listing)>,
    collapsed: BTreeSet<String>,
    /// The search (the app's search field: ×, ⌘A, a drag selecting), typed in while
    /// `typing` (its caret only then, and only while the tree has the keys).
    filter: SearchField,
    typing: bool,
    /// A press in the search holds the pointer: the moves select.
    selecting: bool,
    rows: Vec<Row>,
    slots: Vec<Slot>,
    scroll: f32,
    scale: f32,
    field: Rect,
    list: Rect,
    buttons_at: Vec<(Rect, Button)>,
    hover: Option<usize>,
    hover_button: Option<Button>,
    bar: EdgeScrollbar,
    action: Option<TreeAction>,
    /// The row pressed (where, on its arrow, a double click); dragged once it moved: where.
    pressed: Option<(usize, Point, bool, bool)>,
    dragged_to: Option<Point>,
    pub(crate) naming: Option<(Naming, TextInput)>,
    pub(crate) naming_error: bool,
    naming_field: Rect,
    /// The row the keys are on, and the request open beside it (lit).
    selected: Option<usize>,
    current: Option<(PathBuf, usize)>,
    /// The row its menu is for (`None`: the empty area's).
    menu_row: Option<usize>,
}

impl std::fmt::Debug for RequestTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestTree")
            .field("within", &self.within)
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}

impl RequestTree {
    /// The whole project's tree.
    pub fn project() -> Self {
        Self { scale: 1.0, ..Self::default() }
    }

    /// Collection `within`'s tree: its own buttons, a request made opened beside it.
    pub fn collection(within: &str) -> Self {
        Self { within: within.to_owned(), buttons: true, open_made: true, ..Self::project() }
    }

    pub fn within(&self) -> &str {
        &self.within
    }

    /// The project shown (another: read again from nothing).
    pub fn set_project(
        &mut self,
        queries: Option<Arc<dyn ProjectQueries>>,
        project: Option<kernel::ProjectId>,
    ) {
        let other = match (&self.queries, &queries) {
            (Some(a), Some(b)) => a.root() != b.root(),
            (a, b) => a.is_some() != b.is_some(),
        };
        if other {
            self.generation = None;
            self.listing = Arc::default();
            self.collapsed.clear();
            self.filtered();
        }
        self.queries = queries;
        self.project = project;
    }

    pub fn root(&self) -> Option<PathBuf> {
        self.queries.as_ref().map(|q| q.root())
    }

    pub fn queries(&self) -> Option<Arc<dyn ProjectQueries>> {
        self.queries.clone()
    }

    pub fn project_id(&self) -> Option<kernel::ProjectId> {
        self.project
    }

    /// The request open beside it (lit in the tree).
    pub fn set_current(&mut self, current: Option<(PathBuf, usize)>) {
        self.current = current;
    }

    /// The list read again (on a worker) if the project's files changed since, or it wrote
    /// something itself.
    fn read_again(&mut self) {
        let Some(queries) = self.queries.clone() else { return };
        let generation = queries.generation();
        let stale = self.stale.swap(false, Ordering::AcqRel);
        if !stale && self.generation == Some(generation) {
            return;
        }
        // One read at a time (a burst of shows and writes: not a whole read each, queued):
        // again once this one is taken.
        if self.reading.is_busy() {
            self.stale.store(true, Ordering::Release);
            return;
        }
        self.generation = Some(generation);
        // The file it just made: in the list whatever the project's list says yet (it's
        // made again in the background; one outside the collections' folder isn't walked).
        let made = match &*self.made.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(Made::Request(path, _)) => Some(path.clone()),
            _ => None,
        };
        self.reading.start(THREAD, move || {
            let root = queries.root();
            // Only the requests' files of the project's list (not a copy of all its paths),
            // each once.
            let requests = |f: &str| {
                Path::new(f)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| crate::EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)))
            };
            let mut list: Vec<String> =
                queries.file_list().iter().filter(|f| requests(f)).cloned().collect();
            let mut seen: std::collections::HashSet<String> = list.iter().cloned().collect();
            // The file just made, and the collections' folder from the disk: an empty
            // collection, or a file just made, isn't in the index yet.
            let (folders, files) = collections::walk(&root, collections::COLLECTIONS);
            for file in made.into_iter().chain(files) {
                if seen.insert(file.clone()) {
                    list.push(file);
                }
            }
            let files = collections::read(&list, crate::EXTENSIONS, &|p| {
                queries.read_source(p).or_else(|| std::fs::read_to_string(root.join(p)).ok())
            });
            (generation, Listing { files, folders })
        });
    }

    fn take_read(&mut self) -> bool {
        let Some((generation, listing)) = self.reading.take() else { return false };
        if Some(generation) != self.generation {
            return true;
        }
        self.listing = Arc::new(listing);
        self.filtered();
        // What it made: found, selected (a request opened, where asked).
        let made = self.made.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(made) = made {
            let at = self.rows.iter().position(|r| match (r, &made) {
                (Row::Request { file, request, .. }, Made::Request(path, index)) => {
                    self.listing.files.get(*file).is_some_and(|f| f.path == *path)
                        && request == index
                }
                (Row::Folder { path, .. }, Made::Folder(made)) => path == made,
                _ => false,
            });
            if let Some(at) = at {
                self.selected = Some(at);
                self.reveal(at);
                if self.open_made && matches!(made, Made::Request(..)) {
                    self.activate(at, false, false);
                }
            }
        }
        true
    }

    fn filtered(&mut self) {
        self.rows =
            collections::rows(&self.listing, &self.collapsed, self.filter.text(), &self.within);
        self.hover = None;
        self.selected = self.selected.filter(|i| *i < self.rows.len());
        self.lay_slots();
    }

    fn lay_slots(&mut self) {
        let within = self.within.as_str();
        let naming = self.naming.as_ref().map(|(n, _)| match n {
            Naming::NewCollection { parent } if parent == within || parent.is_empty() => {
                NameFor::Top
            }
            Naming::NewCollection { parent } => NameFor::In(parent),
            Naming::Rename { path } => NameFor::Folder(path),
        });
        self.slots = collections::slots(&self.rows, naming);
    }

    fn slot_of(&self, row: usize) -> Option<usize> {
        self.slots.iter().position(|s| *s == Slot::Row(row))
    }

    fn select_by(&mut self, by: isize) {
        let Some(last) = self.rows.len().checked_sub(1) else { return };
        let next = match self.selected {
            None if by >= 0 => 0,
            None => last,
            Some(i) => i.saturating_add_signed(by).min(last),
        };
        self.selected = Some(next);
        self.reveal(next);
    }

    fn reveal(&mut self, row: usize) {
        let Some(slot) = self.slot_of(row) else { return };
        let h = self.row_h();
        let top = slot as f32 * h;
        let room = (self.list.size.height - self.px(12.0)).max(h);
        if top < self.scroll {
            self.scroll = top;
        } else if top + h > self.scroll + room {
            self.scroll = top + h - room;
        }
    }

    /// ← / →: a folder closed (else the keys to what holds it), or opened.
    fn fold(&mut self, open: bool) {
        let Some(i) = self.selected else { return };
        let Some(row) = self.rows.get(i).cloned() else { return };
        match (&row, open) {
            (Row::Folder { path, expanded, .. }, _) if *expanded != open => {
                let path = path.clone();
                self.toggle(&path);
                self.selected = self
                    .rows
                    .iter()
                    .position(|r| matches!(r, Row::Folder { path: p, .. } if *p == path));
            }
            (_, false) => {
                let depth = row.depth();
                if let Some(up) = (0..i).rev().find(|j| self.rows[*j].depth() < depth) {
                    self.selected = Some(up);
                }
            }
            (_, true) => self.select_by(1),
        }
        if let Some(i) = self.selected {
            self.reveal(i);
        }
    }

    /// Row `i` chosen: a folder opened or closed (a double click: the collection opened), a
    /// request asked for.
    fn activate(&mut self, i: usize, on_arrow: bool, double: bool) {
        let Some(row) = self.rows.get(i).cloned() else { return };
        match row {
            Row::Folder { path, .. } if double && !on_arrow => {
                self.action = Some(TreeAction::OpenCollection(collections::collection_of(&path)));
            }
            Row::Folder { path, .. } => self.toggle(&path),
            Row::Request { file, request, .. } => {
                let (Some(root), Some(listed)) = (self.root(), self.listing.files.get(file)) else {
                    return;
                };
                let Some(r) = listed.requests.get(request) else { return };
                self.action = Some(TreeAction::Open(Opened {
                    file: root.join(&listed.path),
                    index: request,
                    label: r.label.clone(),
                    collection: collections::collection_of(listed.folder()),
                }));
            }
        }
    }

    fn toggle(&mut self, path: &str) {
        let key = collections::collapse_key(path);
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        self.filtered();
    }

    fn row_h(&self) -> f32 {
        self.px(ROW_HEIGHT)
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// The folder row `row` is in (a folder: itself), relative.
    fn folder_of(&self, row: Option<usize>) -> String {
        match row.and_then(|i| self.rows.get(i)) {
            Some(Row::Folder { path, .. }) => path.clone(),
            Some(Row::Request { file, .. }) => {
                self.listing.files.get(*file).map(|f| f.folder().to_owned()).unwrap_or_default()
            }
            None => self.within.clone(),
        }
    }

    // What it does as a view: painted, pressed, keyed.

    pub fn poll(&mut self) -> bool {
        // What was read taken first: something it wrote meanwhile is read again now (not
        // at the next paint, which may not come).
        let taken = self.take_read();
        if self.stale.load(Ordering::Acquire) {
            self.read_again();
        }
        taken
    }

    pub fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.bar.next_frame()
    }

    pub fn scroll(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).max(0.0);
        self.scroll != before
    }

    pub fn take_action(&mut self) -> Option<TreeAction> {
        self.action.take()
    }

    /// It wrote something not read yet (its owner may ask for fresher project data).
    pub fn is_stale(&self) -> bool {
        self.stale.load(Ordering::Acquire)
    }

    /// Read again at the next poll (something changed it from outside: an import).
    pub fn refresh(&self) {
        self.stale.store(true, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/request_tree.rs"]
mod tests;
