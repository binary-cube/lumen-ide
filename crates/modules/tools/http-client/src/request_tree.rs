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

    /// Work for the host's worker: what it gives back runs on the UI thread (the list read
    /// again first).
    fn job(&mut self, work: impl FnOnce() -> workbench_model::Completion + Send + 'static) {
        let stale = Arc::clone(&self.stale);
        self.action = Some(TreeAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
            host.spawn(Box::new(move || {
                let then = work();
                stale.store(true, Ordering::Release);
                then
            }));
        }))));
    }

    /// A name to be typed for `naming` on its row, `text` first in it (selected).
    fn start_naming(&mut self, naming: Naming, text: &str) {
        if let Naming::NewCollection { parent } = &naming {
            self.collapsed.remove(&collections::collapse_key(parent));
        }
        let mut input = TextInput::new(text);
        input.select_all();
        self.naming = Some((naming, input));
        self.naming_error = false;
        self.typing = false;
        self.filtered();
        if let Some(slot) = self.slots.iter().position(|s| matches!(s, Slot::Naming(_))) {
            let h = self.row_h();
            let top = slot as f32 * h;
            if top < self.scroll || top + h > self.scroll + self.list.size.height {
                self.scroll = (top - h).max(0.0);
            }
        }
    }

    /// A new collection to be named, in `parent` (`None`: where the tree is: its own
    /// folder, or the project's collections).
    pub fn new_collection(&mut self, parent: Option<String>) {
        let parent = parent.unwrap_or_else(|| {
            if self.within.is_empty() {
                collections::COLLECTIONS.to_owned()
            } else {
                self.within.clone()
            }
        });
        self.start_naming(Naming::NewCollection { parent }, "");
    }

    fn cancel_naming(&mut self) {
        self.naming = None;
        self.lay_slots();
    }

    /// The name typed, done (Enter, or `settled`: the keyboard gone elsewhere). Not a name:
    /// asked again on Enter, given up when settled.
    fn finish_naming(&mut self, settled: bool) {
        let Some((naming, input)) = self.naming.take() else { return };
        self.lay_slots();
        let typed = input.text().to_owned();
        let unchanged = matches!(&naming, Naming::Rename { path } if collections::base_name(path) == typed.trim());
        if typed.trim().is_empty() || unchanged {
            return;
        }
        let Some(name) = collections::valid_name(&typed) else {
            self.action = Some(notice(t("http.bad_name")));
            if !settled {
                self.naming = Some((naming, input));
                self.naming_error = true;
                self.lay_slots();
            }
            return;
        };
        match naming {
            Naming::NewCollection { parent } => self.make_folder(&parent, &name),
            Naming::Rename { path } => self.rename(&path, &name),
        }
    }

    /// Folder `name` made in `parent` (nothing else: no file, nothing opened).
    fn make_folder(&mut self, parent: &str, name: &str) {
        let Some(root) = self.root() else { return };
        let path = if parent.is_empty() { name.to_owned() } else { format!("{parent}/{name}") };
        let folder = root.join(&path);
        *self.made.lock().unwrap_or_else(PoisonError::into_inner) = Some(Made::Folder(path));
        self.job(move || {
            let made = if folder.exists() {
                Err(i18n::tr!("http.name_taken", name = folder.display().to_string().as_str()))
            } else {
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())
            };
            Box::new(move |host: &mut dyn WorkbenchHost| match made {
                Ok(()) => {
                    if let Some(dir) = folder.parent() {
                        host.relist(dir);
                    }
                }
                Err(why) => host.notify(workbench_model::Notification::error(why)),
            })
        });
    }

    /// A new request in folder `folder` (relative): at the end of its `requests.http`
    /// (made if it has none), then selected.
    pub fn new_request(&mut self, folder: Option<String>) {
        let Some(root) = self.root() else { return };
        let folder = folder.unwrap_or_else(|| self.within.clone());
        let relative = if folder.is_empty() {
            "requests.http".to_owned()
        } else {
            format!("{folder}/requests.http")
        };
        let file = root.join(&relative);
        let made = Arc::clone(&self.made);
        self.job(move || {
            let text = std::fs::read_to_string(&file).unwrap_or_default();
            let mut out = text.trim_end().to_owned();
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(TEMPLATE);
            let index = http_requests::parse(&out).requests.len().saturating_sub(1);
            let written = file
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| files::write_atomic(&file, out.as_bytes()))
                .map_err(|e| e.to_string());
            if written.is_ok() {
                *made.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(Made::Request(relative, index));
            }
            Box::new(move |host: &mut dyn WorkbenchHost| match written {
                Ok(()) => {
                    if let Some(dir) = file.parent() {
                        host.relist(dir);
                    }
                }
                Err(why) => host.notify(workbench_model::Notification::error(why)),
            })
        });
    }

    /// `path` (relative) renamed `name`, by the app (its tabs follow).
    fn rename(&mut self, path: &str, name: &str) {
        let Some(root) = self.root() else { return };
        let from = root.join(path);
        let Some(to) = from.parent().map(|dir| dir.join(name)) else { return };
        let stale = Arc::clone(&self.stale);
        self.action = Some(TreeAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
            if let Err(why) = host.rename_entry(&from, &to) {
                host.notify(workbench_model::Notification::error(why));
            }
            stale.store(true, Ordering::Release);
        }))));
    }

    /// Request `request` of file `file` moved to the end of `into` (relative; on a worker).
    fn move_request(&mut self, file: usize, request: usize, into: &str) {
        let (Some(root), Some(from)) = (self.root(), self.listing.files.get(file)) else {
            return;
        };
        let Some(hash) = from.requests.get(request).map(|r| r.hash) else { return };
        let (from, into) = (root.join(&from.path), root.join(into));
        if from == into {
            return;
        }
        self.job(move || {
            let source = std::fs::read_to_string(&from).unwrap_or_default();
            let target = std::fs::read_to_string(&into).unwrap_or_default();
            // Found by its text (the list may be older than the file); moved as it is.
            let moved = http_requests::draft::find_by_hash(&source, hash)
                .and_then(|at| http_requests::draft::move_raw(&source, at, &target));
            let Some((left, into_text)) = moved else { return changed_outside() };
            // Written where it goes first: never lost between the two.
            let made = into.parent().map_or(Ok(()), std::fs::create_dir_all);
            if made.is_ok() && files::write_atomic(&into, into_text.as_bytes()).is_ok() {
                let _ = files::write_atomic(&from, left.as_bytes());
            }
            Box::new(|_: &mut dyn WorkbenchHost| {})
        });
    }

    /// Request `request` of file `file` changed in it (on a worker): removed, or copied at
    /// its end.
    fn change_request(&mut self, file: usize, request: usize, duplicate: bool) {
        let (Some(root), Some(listed)) = (self.root(), self.listing.files.get(file)) else {
            return;
        };
        let Some(hash) = listed.requests.get(request).map(|r| r.hash) else { return };
        let path = root.join(&listed.path);
        self.job(move || {
            let Ok(text) = std::fs::read_to_string(&path) else {
                return Box::new(|_: &mut dyn WorkbenchHost| {});
            };
            let Some(at) = http_requests::draft::find_by_hash(&text, hash) else {
                return changed_outside();
            };
            let changed = if duplicate {
                http_requests::draft::duplicate_raw(&text, at, &t("http.copy"))
            } else {
                Some(http_requests::draft::remove_from(&text, at))
            };
            if let Some(changed) = changed {
                let _ = files::write_atomic(&path, changed.as_bytes());
            }
            Box::new(|_: &mut dyn WorkbenchHost| {})
        });
    }

    /// The environments of folder `folder` (relative) in their tab.
    fn edit_environments(&mut self, folder: &str) {
        let Some(root) = self.root() else { return };
        // A request file of the folder stands for it (its environments are the nearest).
        let file = root.join(folder).join("requests.http");
        let data = crate::env_view::EnvData { file, root: Some(root) };
        self.action = Some(TreeAction::View(ViewerAction::OpenView(ui_components::OpenView::new(
            t("http.edit_environments"),
            crate::env_view::EXTENSION,
            data,
        ))));
    }

    fn slot_at(&self, p: Point) -> Option<usize> {
        if !self.list.contains(p) {
            return None;
        }
        let h = self.row_h();
        let y = p.y - self.list.min_y() - self.px(6.0) + self.scroll;
        TreeList::new(h, self.slots.len()).row_at(y)
    }

    fn row_at(&self, p: Point) -> Option<usize> {
        match self.slots.get(self.slot_at(p)?)? {
            Slot::Row(i) => Some(*i),
            Slot::Naming(_) => None,
        }
    }

    /// The folder a request being dragged would go into, if it moves (`None`: not dragging,
    /// over nothing, or over its own folder).
    fn drop_folder(&self) -> Option<String> {
        let at = self.dragged_to?;
        let (i, ..) = self.pressed?;
        let Some(Row::Request { file, .. }) = self.rows.get(i) else { return None };
        let target = self.folder_of(Some(self.row_at(at)?));
        let own = self.listing.files.get(*file).map(|f| f.folder())?;
        (own != target).then_some(target)
    }

    /// Whether row `i` is in `folder` (it, or under it; everything under the root).
    fn in_folder(&self, i: usize, folder: &str) -> bool {
        let path = match self.rows.get(i) {
            Some(Row::Folder { path, .. }) => path.as_str(),
            Some(Row::Request { file, .. }) => {
                self.listing.files.get(*file).map_or("", |f| f.folder())
            }
            None => return false,
        };
        folder.is_empty()
            || path == folder
            || path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
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

    pub fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint {
            bounds: b, scale, text, scene, palette: p, icons, waker, focused, ..
        } = ctx;
        self.scale = scale;
        self.reading.set_waker(&waker);
        self.take_read();
        // Seen: read again if the project's files changed (cheap when they didn't).
        self.read_again();
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let base = scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.sidebar_background));
        let buttons =
            if self.buttons { Button::ALL.len() as f32 * (px(BUTTON) + px(2.0)) } else { 0.0 };
        self.field = Rect::new(
            b.min_x() + px(PADDING),
            b.min_y() + px(FIELD_TOP),
            (b.size.width - 2.0 * px(PADDING) - buttons).max(0.0),
            px(FIELD_HEIGHT),
        );
        let typing = focused && self.typing && self.naming.is_none();
        let placeholder = t("http.search_requests");
        self.filter.paint(base, text, icons, &ds, (self.field, typing), &placeholder);
        self.buttons_at.clear();
        if self.buttons {
            let size = px(BUTTON);
            let mut x = self.field.max_x() + px(4.0);
            for button in Button::ALL {
                let r = Rect::new(x, self.field.min_y(), size, size);
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .icon(button.icon())
                    .rest_tone(ds::Tone::Neutral)
                    .hovered(self.hover_button == Some(button));
                ds::button(base, text, icons, &ds, r, spec);
                self.buttons_at.push((r, button));
                x = r.max_x() + px(2.0);
            }
        }
        let footer = Rect::new(b.min_x(), b.max_y() - px(FOOTER), b.size.width, px(FOOTER));
        self.paint_footer(base, text, &ds, footer);
        let top = b.min_y() + px(LIST_TOP);
        self.list = Rect::new(b.min_x(), top, b.size.width, (footer.min_y() - top).max(0.0));
        self.naming_field = Rect::default();
        if self.slots.is_empty() {
            let (title, detail) = if !self.filter.text().trim().is_empty() {
                (t("http.no_match"), None)
            } else if self.within.is_empty() {
                (t("http.no_files"), Some(t("http.no_files_detail")))
            } else {
                (t("http.empty_collection"), Some(t("http.empty_collection_detail")))
            };
            ds::empty_state(base, text, &ds, self.list, &title, detail.as_deref());
            return;
        }
        let h = px(ROW_HEIGHT);
        let content = self.slots.len() as f32 * h + px(12.0);
        self.scroll = self.scroll.clamp(0.0, (content - self.list.size.height).max(0.0));
        let range = TreeList::new(h, self.slots.len()).visible(self.scroll, self.list.size.height);
        let origin = self.list.min_y() + px(6.0) - self.scroll;
        let layer = scene.push_layer(Some(self.list));
        let font = Font::new(p.ui_font.clone(), px(p.ui_font_size - 0.5));
        let small = ds.code_font(-1.5).with_weight(Font::SEMIBOLD);
        let metrics = text.metrics(&font);
        let inset = px(8.0);
        let chevron = px(12.0);
        let guide = p.text.with_alpha(0.08);
        // A request being dragged: the folder it would go into (not its own: nothing moves),
        // lit with what's in it, so where it lands is plain.
        let dragged = self.dragged_to.and_then(|_| self.pressed.map(|(i, ..)| i));
        let target = self.drop_folder();
        if let Some(target) = &target {
            let slots = range.clone().filter(
                |&slot| matches!(self.slots[slot], Slot::Row(i) if self.in_folder(i, target)),
            );
            let (first, last) =
                slots.fold((usize::MAX, 0), |(a, b), slot| (a.min(slot), b.max(slot)));
            let zone = if first == usize::MAX {
                // The root, or a folder scrolled away: the whole list.
                Rect::new(
                    self.list.min_x() + px(2.0),
                    self.list.min_y() + px(2.0),
                    (self.list.size.width - px(4.0)).max(0.0),
                    (self.list.size.height - px(4.0)).max(0.0),
                )
            } else {
                Rect::new(
                    self.list.min_x() + inset - px(2.0),
                    origin + first as f32 * h - px(1.0),
                    self.list.size.width - 2.0 * inset + px(4.0),
                    (last + 1 - first) as f32 * h + px(2.0),
                )
            };
            layer.quad(
                Quad::filled(zone, p.accent.with_alpha(0.10))
                    .rounded(px(8.0))
                    .bordered(ds.line().max(px(1.5)), p.accent.with_alpha(0.85)),
            );
        }
        for slot in range {
            let row_top = origin + slot as f32 * h;
            let center = |size: f32| (row_top + (h - size) / 2.0).round();
            let depth = match self.slots[slot] {
                Slot::Row(i) => self.rows[i].depth(),
                Slot::Naming(depth) => depth,
            };
            // The guides of what holds it, as the project's tree draws them.
            let guide_x = |level: usize| {
                self.list.min_x() + px(PADDING) + level as f32 * px(INDENT) + chevron / 2.0
            };
            tree_row::guides(layer, 0..usize::from(depth), guide_x, (row_top, h), ds.line(), guide);
            let x = self.list.min_x() + px(PADDING) + f32::from(depth) * px(INDENT);
            let i = match self.slots[slot] {
                Slot::Row(i) => i,
                Slot::Naming(_) => {
                    let Some((_, input)) = &self.naming else { continue };
                    let size = px(15.0);
                    let icon_rect =
                        Rect::new((x + chevron + px(4.0)).round(), center(size), size, size);
                    if let Some(id) = icons.id("folder") {
                        draw_icon(layer, icons, id, icon_rect, p.text_muted);
                    }
                    let field = Rect::new(
                        icon_rect.max_x() + px(5.0),
                        row_top + px(2.0),
                        (self.list.max_x() - inset - icon_rect.max_x() - px(5.0)).max(px(40.0)),
                        h - px(4.0),
                    );
                    let border = if self.naming_error { p.danger } else { p.accent };
                    layer.quad(
                        Quad::filled(field, p.input_background)
                            .rounded(px(5.0))
                            .bordered(ds.line(), border),
                    );
                    let mut style = ds::field_style(&ds, font.clone(), false);
                    style.background = Rgba::TRANSPARENT;
                    style.border = Rgba::TRANSPARENT;
                    style.border_focused = Rgba::TRANSPARENT;
                    style.padding = px(6.0);
                    input.paint(layer, text, field, focused, &t("http.collection_name"), &style);
                    self.naming_field = field;
                    continue;
                }
            };
            let row = &self.rows[i];
            let pill = Rect::new(
                self.list.min_x() + inset,
                row_top + ds.line(),
                self.list.size.width - 2.0 * inset,
                h - 2.0 * ds.line(),
            );
            let current = match row {
                Row::Request { file, request, .. } => {
                    self.current.as_ref().is_some_and(|(path, index)| {
                        index == request
                            && self
                                .listing
                                .files
                                .get(*file)
                                .is_some_and(|f| path.ends_with(&f.path))
                    })
                }
                Row::Folder { .. } => false,
            };
            let moving = dragged == Some(i);
            if target.is_some() || moving {
                // Dragging: only the zone it would land in is lit (the hover would mislead).
            } else if (self.selected == Some(i) && focused) || current {
                tree_row::highlight(layer, pill, p.tree_selection, px(6.0));
            } else if self.hover == Some(i) || self.selected == Some(i) {
                tree_row::highlight(layer, pill, p.tree_hover, px(6.0));
            }
            // The row being dragged, faded where it was.
            let ink = |c: Rgba| if moving { c.with_alpha(c.a * 0.4) } else { c };
            let baseline = centered_baseline(row_top, h, &metrics);
            let end = self.list.max_x() - px(14.0);
            match row {
                Row::Folder { path, expanded, .. } => {
                    let at = Rect::new(x.round(), center(chevron), chevron, chevron);
                    tree_row::chevron(layer, icons, at, *expanded, p.text_muted);
                    let size = px(15.0);
                    let icon_rect =
                        Rect::new((x + chevron + px(4.0)).round(), center(size), size, size);
                    if let Some(id) = icons.id("folder") {
                        draw_icon(layer, icons, id, icon_rect, p.text_muted);
                    }
                    let lx = (icon_rect.max_x() + px(7.0)).round();
                    let label = collections::base_name(path);
                    let shown = ds::ellipsize(text, label, &font, (end - lx).max(0.0));
                    let shaped = text.shape_line(&shown, &font);
                    draw_text(layer, &shaped, lx, baseline, ink(p.text));
                }
                Row::Request { file, request, .. } => {
                    let request = &self.listing.files[*file].requests[*request];
                    let shaped = text.shape_line(&request.method, &small);
                    let mx = x + chevron + px(4.0);
                    draw_text(
                        layer,
                        &shaped,
                        mx.round(),
                        baseline,
                        ink(crate::format::method_color(&request.method, p)),
                    );
                    let lx = (mx + px(46.0).max(shaped.width + px(8.0))).round();
                    let shown = ds::ellipsize(text, &request.label, &font, (end - lx).max(0.0));
                    let shaped = text.shape_line(&shown, &font);
                    draw_text(layer, &shaped, lx, baseline, ink(p.text));
                }
            }
        }
        self.bar.paint(scene, p, scale, self.list, content, self.scroll);
        // What's dragged, under the pointer: its method and name on a card.
        if let (Some(i), Some(at)) = (dragged, self.dragged_to)
            && let Some(Row::Request { file, request, .. }) = self.rows.get(i)
        {
            let request = &self.listing.files[*file].requests[*request];
            let lead = ui_components::drag_card::Lead::Tag(
                &request.method,
                crate::format::method_color(&request.method, p),
            );
            let top = scene.push_layer(Some(b));
            ui_components::drag_card::paint(top, &ds, text, icons, lead, &request.label, at);
        }
    }

    /// The keys' footer: how the rows are walked (or, a name being typed, how it's kept).
    fn paint_footer(&self, layer: &mut Layer, text: &mut TextSystem, ds: &Ctx<'_>, footer: Rect) {
        let p = ds.palette;
        layer.quad(Quad::filled(footer, ds.fade(p.text.with_alpha(0.022))));
        ds::divider(layer, ds, footer.min_x(), footer.max_x(), footer.min_y());
        let naming = self.naming.is_some();
        let labels: Vec<String> = if naming {
            vec![t("http.keys_keep"), t("http.keys_cancel")]
        } else {
            vec![t("http.keys_navigate"), t("http.keys_fold"), t("http.keys_open")]
        };
        let hints: Vec<ui_kit::KeyHint<'_>> = if naming {
            vec![(&["↵"], &labels[0]), (&["esc"], &labels[1])]
        } else {
            vec![(&["↑", "↓"], &labels[0]), (&["←", "→"], &labels[1]), (&["↵"], &labels[2])]
        };
        let row = Rect::new(
            footer.min_x() + ds.px(14.0),
            footer.min_y(),
            (footer.size.width - ds.px(28.0)).max(0.0),
            footer.size.height,
        );
        ui_kit::paint_key_hints(layer, text, &hints, row, &p.key_hints(ds.scale));
    }

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

    pub fn press(&mut self, p: Point, clicks: u32) {
        if let Some((_, input)) = &mut self.naming {
            if self.naming_field.contains(p) {
                input.press(p.x, clicks);
                return;
            }
            // Pressed elsewhere: the name kept, as Enter (the tree's way).
            self.finish_naming(true);
        }
        if let Some(button) = self.button_at(p) {
            match button {
                Button::NewRequest => self.new_request(None),
                Button::NewCollection => self.new_collection(None),
                Button::Environments => {
                    let within = self.within.clone();
                    self.edit_environments(&within);
                }
                Button::Import => self.action = Some(TreeAction::Import),
            }
            return;
        }
        self.typing = self.field.contains(p);
        if self.typing {
            self.selecting = Control::press(&mut self.filter, p, clicks) == Press::Held;
            // × emptied it: every row again.
            if self.filter.take_cleared() {
                self.scroll = 0.0;
                self.filtered();
            }
            self.pressed = None;
            return;
        }
        self.dragged_to = None;
        self.pressed = self.row_at(p).map(|i| {
            let depth = f32::from(self.rows.get(i).map_or(0, Row::depth));
            let arrow_end =
                self.list.min_x() + self.px(PADDING) + depth * self.px(INDENT) + self.px(16.0);
            (i, p, p.x < arrow_end, clicks >= 2)
        });
        if let Some((i, ..)) = self.pressed {
            self.selected = Some(i);
        }
    }

    /// A request dragged: where it's over (it moves once it went a few pixels).
    pub fn drag(&mut self, p: Point) -> bool {
        if self.selecting {
            return Control::drag(&mut self.filter, p);
        }
        let Some((i, from, ..)) = self.pressed else { return false };
        if !matches!(self.rows.get(i), Some(Row::Request { .. })) {
            return false;
        }
        if self.dragged_to.is_none() && (p.x - from.x).hypot(p.y - from.y) < self.px(4.0) {
            return false;
        }
        self.dragged_to = Some(p);
        self.hover = self.row_at(p);
        true
    }

    /// Released: a click chooses its row; a request dropped on a folder (or another
    /// request's folder) moves into that folder's `requests.http`.
    pub fn release(&mut self) -> bool {
        if std::mem::take(&mut self.selecting) {
            return false;
        }
        let Some((i, _, on_arrow, double)) = self.pressed.take() else { return false };
        match self.dragged_to.take() {
            None => self.activate(i, on_arrow, double),
            Some(at) => {
                let target = self.row_at(at).map(|t| self.folder_of(Some(t)));
                if let (Some(Row::Request { file, request, .. }), Some(folder)) =
                    (self.rows.get(i).cloned(), target)
                {
                    let same = self.listing.files.get(file).is_some_and(|f| f.folder() == folder);
                    if !same {
                        let into = if folder.is_empty() {
                            "requests.http".to_owned()
                        } else {
                            format!("{folder}/requests.http")
                        };
                        self.move_request(file, request, &into);
                    }
                }
            }
        }
        true
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.row_at(p);
        let button = self.button_at(p);
        // The search's × lit under the mouse.
        let clear = Control::hover(&mut self.filter, Some(p));
        let changed = clear || self.hover != hover || self.hover_button != button;
        self.hover = hover;
        self.hover_button = button;
        changed | self.bar.hover(p)
    }

    fn button_at(&self, p: Point) -> Option<Button> {
        self.buttons_at.iter().find(|(r, _)| r.contains(p)).map(|(_, b)| *b)
    }

    pub fn clickable(&self, p: Point) -> bool {
        self.row_at(p).is_some() || self.button_at(p).is_some()
    }

    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // The search's: the text's, a hand on its ×.
        if self.field.contains(p) {
            return Control::cursor(&self.filter, p);
        }
        if self.naming_field.contains(p) {
            return Some(ViewerCursor::Text);
        }
        self.clickable(p).then_some(ViewerCursor::Pointer)
    }

    pub fn tooltip(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        let (r, button) = self.buttons_at.iter().find(|(r, _)| r.contains(p))?;
        Some(ui_components::tooltip::Tip::new(*r, button.tooltip()))
    }

    pub fn wants_text(&self) -> bool {
        self.typing || self.naming.is_some()
    }

    pub fn text_input(&mut self, text: &str) -> bool {
        if let Some((_, input)) = &mut self.naming {
            let name: String = text.chars().filter(|c| !c.is_control()).collect();
            input.insert(&name);
            self.naming_error = false;
            return true;
        }
        self.typing = true;
        // On one line, the search field's own way (its breaks as spaces).
        Control::text(&mut self.filter, text);
        self.scroll = 0.0;
        self.filtered();
        true
    }

    pub fn selected_text(&self) -> Option<String> {
        let selected = match &self.naming {
            Some((_, input)) => input.selected_text(),
            None if self.typing => self.filter.input.selected_text(),
            None => "",
        };
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    /// ⌘A in the name being typed, or in the search.
    pub fn select_all(&mut self) -> bool {
        self.edit_command(SELECT_ALL)
    }

    /// An edit command (⌘A, ⌘X, ⌘Z, ⌘⇧Z) in the name being typed, or in the search (what
    /// it changes filtered again).
    pub fn edit_command(&mut self, id: &str) -> bool {
        if let Some((_, input)) = &mut self.naming {
            self.naming_error = false;
            return Control::command(input, id);
        }
        if !self.typing || !Control::command(&mut self.filter, id) {
            return false;
        }
        if id != SELECT_ALL {
            self.scroll = 0.0;
            self.filtered();
        }
        true
    }

    pub fn edit_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if let Some((_, input)) = &mut self.naming {
            match key {
                ViewerKey::Escape => self.cancel_naming(),
                ViewerKey::Enter => self.finish_naming(false),
                _ => {
                    self.naming_error = false;
                    return reach.edit(input, key);
                }
            }
            return true;
        }
        if key == ViewerKey::Escape {
            let had = !self.filter.text().is_empty();
            self.filter.set_text("");
            self.typing = false;
            self.filtered();
            return had;
        }
        // The rows walked with the keys (from the search too: ↑ ↓ and ↵ are the list's).
        match key {
            ViewerKey::Up => self.select_by(-1),
            ViewerKey::Down => self.select_by(1),
            ViewerKey::PageUp | ViewerKey::PageDown if !self.typing => {
                let page = (self.list.size.height / self.row_h()).floor().max(1.0) as isize;
                self.select_by(if key == ViewerKey::PageUp { -page } else { page });
            }
            ViewerKey::Home | ViewerKey::End if !self.typing => {
                self.select_by(if key == ViewerKey::Home { isize::MIN } else { isize::MAX });
            }
            ViewerKey::Left | ViewerKey::Right if !self.typing => {
                self.fold(key == ViewerKey::Right);
            }
            ViewerKey::Enter => {
                let first = (self.typing && !self.rows.is_empty()).then_some(0);
                let Some(i) = self.selected.or(first) else { return false };
                self.activate(i, false, false);
            }
            _ => {
                if self.typing && reach.edit(&mut self.filter.input, key) {
                    self.filtered();
                    return true;
                }
                return false;
            }
        }
        true
    }

    /// The keyboard went elsewhere: the search stops, a name being typed is kept.
    pub fn focus_lost(&mut self) {
        self.typing = false;
        self.finish_naming(true);
    }

    /// Its menu at `p`: a folder's, a request's, else the empty area's (`window`: the
    /// project's tree, where a collection opens in its tab).
    pub fn menu_at(&mut self, p: Point, window: bool) -> Option<Vec<ViewMenuEntry>> {
        if !self.list.contains(p) {
            return None;
        }
        let row = self.row_at(p);
        self.menu_row = row;
        let item = |id: &str, key: &str, icon: &'static str| {
            ViewMenuEntry::item(id, t(key)).with_icon(icon)
        };
        Some(match row.and_then(|i| self.rows.get(i)) {
            Some(Row::Folder { .. }) => {
                let mut entries = Vec::new();
                if window {
                    entries.push(item(
                        "row-open-collection",
                        "http.open_collection",
                        "folder-open",
                    ));
                }
                entries.extend([
                    item("row-new-request", "http.new_request", "plus"),
                    item("row-new-collection", "http.new_collection_in", "folder-plus"),
                    item("row-rename", "http.rename_collection", "pencil"),
                    item("row-environments", "http.edit_environments", "file-json"),
                ]);
                entries
            }
            Some(Row::Request { .. }) => vec![
                item("row-edit", "http.edit_request", "file-pen"),
                item("row-open-text", "http.open_as_text", "file"),
                item("row-duplicate", "http.duplicate", "copy"),
                item("row-delete", "http.delete_request", "trash"),
            ],
            None => vec![
                item("row-new-request", "http.new_request", "plus"),
                item("row-new-collection", "http.new_collection", "folder-plus"),
                item("row-import-curl", "http.import_curl", "terminal"),
                item("row-import", "http.import_collection", "download"),
            ],
        })
    }

    /// A row of its menu chosen.
    pub fn menu_command(&mut self, id: &str) -> bool {
        let row = self.menu_row.and_then(|i| self.rows.get(i)).cloned();
        match (id, row) {
            ("row-open-collection", Some(Row::Folder { path, .. })) => {
                self.action = Some(TreeAction::OpenCollection(path));
            }
            ("row-new-request", row) => {
                let folder = match &row {
                    Some(_) => self.folder_of(self.menu_row),
                    None => self.within.clone(),
                };
                self.new_request(Some(folder));
            }
            ("row-new-collection", Some(Row::Folder { path, .. })) => {
                self.new_collection(Some(path));
            }
            ("row-new-collection", None) => self.new_collection(None),
            ("row-rename", Some(Row::Folder { path, .. })) => {
                let name = collections::base_name(&path).to_owned();
                self.start_naming(Naming::Rename { path }, &name);
            }
            ("row-environments", Some(Row::Folder { path, .. })) => self.edit_environments(&path),
            ("row-import", None) => self.action = Some(TreeAction::Import),
            ("row-import-curl", None) => self.action = Some(TreeAction::ImportCurl),
            ("row-edit", Some(Row::Request { .. })) => {
                if let Some(i) = self.menu_row {
                    self.activate(i, false, false);
                }
            }
            ("row-open-text", Some(Row::Request { file, request, .. })) => {
                let (Some(root), Some(listed)) = (self.root(), self.listing.files.get(file)) else {
                    return true;
                };
                let line = listed.requests.get(request).map_or(0, |r| r.line);
                self.action = Some(TreeAction::View(ViewerAction::OpenAt {
                    path: root.join(&listed.path),
                    line,
                }));
            }
            ("row-duplicate", Some(Row::Request { file, request, .. })) => {
                self.change_request(file, request, true);
            }
            ("row-delete", Some(Row::Request { file, request, .. })) => {
                self.change_request(file, request, false);
            }
            _ => return false,
        }
        true
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

/// A message for the user, as the tree's action.
fn notice(text: String) -> TreeAction {
    TreeAction::View(ViewerAction::WithHost(WithHost::once(move |host| {
        host.notify(workbench_model::Notification::error(text));
    })))
}

/// What a change found its request changed (or gone) meanwhile answers: nothing written,
/// the user told.
fn changed_outside() -> workbench_model::Completion {
    Box::new(|host: &mut dyn WorkbenchHost| {
        host.notify(workbench_model::Notification::error(t("http.changed_outside")))
    })
}

#[cfg(test)]
#[path = "../tests/unit/request_tree.rs"]
mod tests;
