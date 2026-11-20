//! The project panel, as a module's [`ToolPanel`]: under the header the app draws for it
//! (its buttons: a new file, locate the active file, expand / collapse, hide), the search
//! for files, then the tree — or the search's results while there is text. Everything it
//! knows of the project comes through the project's queries ([`ProjectQueries`]): the
//! folders' entries (the modules' marks included), the paths found, the files' icons.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kernel::{Point, Rect};
use ui_components::{
    KeyReach, Palette, PanelButton, PanelContext, PressMods, ToolPanel, ToolPanelAction,
    ViewerAction, ViewerCursor, ViewerKey, ViewerPaint, tooltip::Tip,
};
use workbench_model::{
    ActionContext, EntriesRequest, MenuPlace, ProjectQueries, ProjectView, Target,
};
use workspace::{DirEntry, FileTree, Mark, NodeId, NodeKind};

use crate::operations;
use crate::{FileSearch, FileSearchStyle, ProjectTreeStyle, ProjectTreeView, TreeAction};

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
    /// The entries changed meanwhile (heard on the project's events), and who to wake for
    /// them; the subscription lives as long as the panel.
    changes: Arc<Mutex<Vec<PathBuf>>>,
    /// A look at everything again was asked: every folder read so far is read again.
    rescan_all: Arc<std::sync::atomic::AtomicBool>,
    /// Batches of folders read on the listing's thread, waiting to be put in the tree.
    listed: Arc<Mutex<Vec<Vec<Listing>>>>,
    /// Batches sent to that thread and not put in yet: while there are, the next go there
    /// too (in order: an older listing never put over a newer one).
    in_flight: usize,
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
            changes: Arc::default(),
            rescan_all: Arc::default(),
            listed: Arc::default(),
            in_flight: 0,
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

    /// The project's queries (a new snapshot when the project or its files change): another
    /// project opens a new tree; the same one keeps it (what's expanded, the selection).
    fn set_project(&mut self, queries: Option<Arc<dyn ProjectQueries>>) {
        let root = queries.as_ref().map(|q| q.root());
        if root.as_deref() != self.root() {
            self.tree = queries.as_deref().and_then(|q| {
                let root = root.clone()?;
                workspace::open_project(root.clone(), &Listed(q))
                    .inspect_err(
                        |e| tracing::warn!(root = %root.display(), error = %e, "can't list"),
                    )
                    .ok()
            });
            self.view =
                ProjectTreeView::new(self.styled.as_ref().map(tree_style).unwrap_or_default());
            // Another project's search is cleared (typed before the first one came: kept).
            if self.queries.is_some() {
                self.search.clear();
                self.keys = Keys::Tree;
            }
        }
        self.queries = queries;
        self.file_icons.get_mut().clear();
        self.refresh_search();
    }

    /// Follows the project's entries changing (made, removed, renamed on disk; a folder
    /// listed again) on `events` (the one given last): heard here, applied on the next poll
    /// (a wake asked for).
    fn follow(&mut self, events: &kernel::EventBus) {
        let heard = |changes: Arc<Mutex<Vec<PathBuf>>>, waker: Arc<Mutex<Option<Waker>>>| {
            move |paths: Vec<PathBuf>| {
                if let Ok(mut changes) = changes.lock() {
                    changes.extend(paths);
                }
                if let Some(wake) = waker.lock().ok().and_then(|w| w.clone()) {
                    wake();
                }
            }
        };
        let rescan_all = Arc::clone(&self.rescan_all);
        self.events = Some(events.clone());
        let away = |left: bool| {
            let (away, waker) = (self.away.clone(), Arc::clone(&self.waker));
            move || {
                away.heard(left);
                if let Some(wake) = waker.lock().ok().and_then(|w| w.clone()) {
                    wake();
                }
            }
        };
        let (left, back) = (away(true), away(false));
        let (changed, deleted, renamed, rescan) = (
            heard(Arc::clone(&self.changes), Arc::clone(&self.waker)),
            heard(Arc::clone(&self.changes), Arc::clone(&self.waker)),
            heard(Arc::clone(&self.changes), Arc::clone(&self.waker)),
            heard(Arc::clone(&self.changes), Arc::clone(&self.waker)),
        );
        // On disk (the watcher), and announced by whoever deleted or moved entries (it
        // shows at once, before the watcher's word).
        self.subscriptions = vec![
            events.subscribe(move |e: &workspace::events::EntriesChanged| changed(e.paths.clone())),
            events.subscribe(move |e: &workspace::events::EntriesDeleted| deleted(e.paths.clone())),
            events.subscribe(move |e: &workspace::events::FileRenamed| {
                renamed(vec![e.from.clone(), e.to.clone()]);
            }),
            // The watcher just started, or the system dropped its events: every folder read
            // so far read again (what changed in an open one meanwhile wasn't heard).
            events.subscribe(move |e: &workspace::events::EntriesRescan| {
                rescan(vec![e.root.clone()]);
                rescan_all.store(true, std::sync::atomic::Ordering::Release);
            }),
            // Left and taken again: what the watcher may have lost meanwhile, checked.
            events.subscribe(move |_: &workspace::events::ProjectLeft| left()),
            events.subscribe(move |_: &workspace::events::ProjectBroughtForward| back()),
        ];
    }

    /// The window left or taken again: the folders read so far noted, or compared, on the
    /// listing's thread; those found changed told on the project's events (as the watcher
    /// would: the tree reads them again — through [`Self::follow`] — and so does whoever
    /// else follows the entries).
    fn check_away(&mut self) {
        let Some(tree) = &self.tree else { return };
        let waker = self.waker.lock().ok().and_then(|w| w.clone());
        let lister = self
            .lister
            .get_or_insert_with(|| kernel::work::Pool::new("tree-list", 1, kernel::work::IDLE));
        let found = self.away.run(
            || tree.loaded_dirs(),
            |job| lister.spawn(kernel::work::Priority::Interactive, None, job),
            waker,
        );
        if !found.is_empty()
            && let Some(events) = &self.events
        {
            events.publish(&workspace::events::EntriesChanged { paths: found });
        }
    }

    /// The changes heard meanwhile, applied: each entry's folder read again (a folder's
    /// own entries too), once however many of its entries changed — at once for a few, on
    /// the listing's thread for many (put in the tree at a later poll). `true`: the tree
    /// changed.
    fn apply_changes(&mut self) -> bool {
        self.check_away();
        let paths = self.changes.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
        let listed = self.listed.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default();
        let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries) else { return false };
        let root = tree.root_path().to_path_buf();
        // Only the folders shown (read before): the others are read when opened. A look at
        // everything again: all of those.
        let mut dirs: Vec<PathBuf> = paths
            .iter()
            .filter(|p| p.starts_with(&root))
            .flat_map(|p| [p.parent().map(Path::to_path_buf), Some(p.clone())])
            .flatten()
            .collect();
        if self.rescan_all.swap(false, std::sync::atomic::Ordering::AcqRel) {
            dirs.extend(tree.loaded_dirs());
        }
        // Each folder looked for once (thousands unpacked into one: not a search each).
        dirs.sort_unstable();
        dirs.dedup();
        dirs.retain(|d| {
            tree.find(d).is_some_and(|n| tree.kind(n) == NodeKind::Directory && tree.is_loaded(n))
        });
        let mut changed = false;
        // Read on the listing's thread (before what's read now, which is newer): put in, if
        // still shown.
        self.in_flight = self.in_flight.saturating_sub(listed.len());
        for (dir, entries) in listed.into_iter().flatten() {
            let Some(node) = tree.find(&dir) else { continue };
            if tree.kind(node) != NodeKind::Directory || !tree.is_loaded(node) {
                continue;
            }
            match entries {
                Ok(entries) => {
                    tree.set_children(node, entries);
                    changed = true;
                }
                Err(e) => tracing::debug!(error = %e, "can't read a changed folder again"),
            }
        }
        if dirs.is_empty() {
            // Nothing shown changed.
        } else if dirs.len() <= LIST_NOW && self.in_flight == 0 {
            let fs = Listed(queries.as_ref());
            for dir in dirs {
                match workspace::refresh_dir(tree, &dir, &fs) {
                    Ok(c) => changed |= c,
                    Err(e) => tracing::debug!(error = %e, "can't read a changed folder again"),
                }
            }
        } else {
            let (queries, into) = (Arc::clone(queries), Arc::clone(&self.listed));
            let waker = self.waker.lock().ok().and_then(|w| w.clone());
            let lister = self
                .lister
                .get_or_insert_with(|| kernel::work::Pool::new("tree-list", 1, kernel::work::IDLE));
            self.in_flight += 1;
            lister.spawn(kernel::work::Priority::Interactive, None, move || {
                let read: Vec<Listing> =
                    dirs.into_iter().map(|d| (d.clone(), queries.list_dir(&d))).collect();
                if let Ok(mut listed) = into.lock() {
                    listed.push(read);
                }
                if let Some(wake) = waker {
                    wake();
                }
            });
        }
        // The selection or the new entry's folder gone: let go of them (their ids are no
        // longer the tree's: their places may hold other entries now).
        let gone = |tree: &FileTree, id: NodeId| !tree.contains(id);
        if self.view.selected().is_some_and(|id| gone(tree, id)) {
            self.view.select(None);
        }
        if self
            .view
            .pending()
            .is_some_and(|p| gone(tree, p.parent) || p.renaming.is_some_and(|id| gone(tree, id)))
        {
            self.view.cancel_new();
            self.keys = Keys::Tree;
        }
        // The entry renamed here, shown under its new name: selected again.
        if let Some(node) = self.renamed.as_deref().and_then(|p| tree.find(p)) {
            self.view.select(Some(node));
            self.renamed = None;
        }
        changed
    }

    /// What the app or a module tells it.
    fn request(&mut self, request: &EntriesRequest) {
        match request {
            EntriesRequest::Reveal { path, expand } => {
                self.search.clear();
                self.keys = Keys::Tree;
                if self.reveal(path)
                    && *expand
                    && let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries)
                    && let Some(node) = tree.find(path)
                    && tree.kind(node) == NodeKind::Directory
                    && !tree.is_expanded(node)
                {
                    let _ = workspace::toggle_directory(tree, node, &Listed(queries.as_ref()));
                }
            }
            EntriesRequest::FocusSearch => {
                self.view.cancel_new();
                self.keys = Keys::Search;
                self.search.input.select_all();
            }
            EntriesRequest::NewEntry { is_dir } => {
                self.new_entry(*is_dir);
            }
            EntriesRequest::Rename { path } => {
                self.rename_entry(path);
            }
        }
    }

    /// The search's results for its text (from the latest queries): asked for on the
    /// search's thread, shown when they come (`poll`); none at once for no text.
    fn refresh_search(&mut self) {
        let text = workspace::split_location(self.search.query().trim()).0.to_owned();
        match &self.queries {
            Some(q) if !text.is_empty() => {
                let wake = self.waker.lock().ok().and_then(|w| w.clone());
                let limits = (FOLDER_RESULTS, FILE_RESULTS);
                self.finder.request(Arc::clone(q), text, limits, wake);
            }
            _ => {
                self.finder.cancel();
                self.search.set_results(workspace::PathResults::default());
            }
        }
    }

    /// Starts a new file / folder in the tree: in the selected folder (or the selected file's,
    /// or the root), its name typed in place.
    pub fn new_entry(&mut self, is_dir: bool) -> bool {
        let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries) else { return false };
        let parent = match self.view.selected() {
            Some(id) if tree.kind(id) == NodeKind::Directory => id,
            Some(id) => {
                tree.find(tree.path(id).parent().unwrap_or(tree.root_path())).unwrap_or(tree.root())
            }
            None => tree.root(),
        };
        if !tree.is_expanded(parent)
            && let Err(e) = workspace::toggle_directory(tree, parent, &Listed(queries.as_ref()))
        {
            tracing::warn!(path = %tree.path(parent).display(), error = %e, "can't open");
        }
        self.search.clear();
        self.view.begin_new(parent, is_dir, tree);
        self.keys = Keys::NewEntry;
        true
    }

    /// Renames `path` in place: shown (its folders opened), its name edited in its row.
    pub fn rename_entry(&mut self, path: &Path) -> bool {
        self.search.clear();
        if !self.reveal(path) {
            return false;
        }
        let Some(tree) = &mut self.tree else { return false };
        let Some(node) = tree.find(path) else { return false };
        if !self.view.begin_rename(node, tree) {
            return false;
        }
        self.keys = Keys::NewEntry;
        true
    }

    /// The typed name is unusable or taken: its border turns red. A rename may keep the
    /// entry's own name, or change only its case.
    fn validate_entry(&mut self) {
        let (Some(tree), Some(pending)) = (&self.tree, self.view.pending()) else { return };
        let name = pending.input.text().trim();
        let error = match pending.renaming {
            Some(id) => {
                let own = tree.name(id);
                let other = name.to_lowercase() != own.to_lowercase();
                workspace::entries::rename_problem(name)
                    || (other && tree.path(pending.parent).join(name).exists())
            }
            None => {
                let taken = !name.is_empty() && tree.path(pending.parent).join(name).exists();
                workspace::entries::name_problem(name) || taken
            }
        };
        if let Some(pending) = self.view.pending_mut() {
            pending.error = error;
        }
    }

    /// Makes the typed entry, shows it in the tree and opens it (a file); or asks for the
    /// rename typed (the app renames it: its tabs follow, a class's file as a refactoring).
    fn commit_entry(&mut self) {
        self.validate_entry();
        let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries) else { return };
        let Some(pending) = self.view.pending().filter(|p| !p.error) else { return };
        if let Some(id) = pending.renaming {
            let (from, name) = (tree.path(id), pending.input.text().trim().to_owned());
            self.view.cancel_new();
            self.keys = Keys::Tree;
            if tree.name(id) != name {
                let to = from.with_file_name(&name);
                self.renamed = Some(to.clone());
                self.action = Some(ToolPanelAction::Rename { from, to });
            }
            return;
        }
        let (parent, name, is_dir) =
            (tree.path(pending.parent), pending.input.text(), pending.is_dir);
        let store = fs_local::entries::LocalEntries;
        let path = match workspace::entries::create(&store, &parent, name, is_dir) {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!(name = %name.trim(), parent = %parent.display(), error = %e, "can't create");
                if let Some(pending) = self.view.pending_mut() {
                    pending.error = true;
                }
                return;
            }
        };
        self.view.cancel_new();
        self.keys = Keys::Tree;
        // Shown now (the watcher would, a bit later).
        let fs = Listed(queries.as_ref());
        let _ = workspace::refresh_path(tree, &path, &fs);
        if let Ok(Some(node)) = workspace::reveal_path(tree, &path, &fs) {
            self.view.reveal(node, tree);
        }
        if !is_dir {
            self.action = Some(open(path, None));
        }
    }

    fn cancel_entry(&mut self) {
        self.view.cancel_new();
        self.keys = Keys::Tree;
    }

    /// The new entry's (or the rename's) name left by a click elsewhere or the keyboard going
    /// away: kept when it can be (as ↵ does), else — empty, unusable, taken — given up.
    fn settle_entry(&mut self) {
        let Some(pending) = self.view.pending() else { return };
        if !pending.input.text().trim().is_empty() {
            self.commit_entry();
        }
        if self.view.pending().is_some() {
            self.cancel_entry();
        }
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

    /// Search result `index` taken: a folder is shown open in the tree, a file opened (at
    /// the place typed after its name, `User.php:12`).
    fn take_result(&mut self, index: usize) {
        let Some(root) = self.root().map(Path::to_path_buf) else { return };
        let Some(found) = self.search.result(index) else { return };
        let path = root.join(&found.path);
        let folder = self.search.is_folder(index);
        let place = workspace::split_location(self.search.query().trim()).1;
        self.search.clear();
        self.keys = Keys::Tree;
        self.hand_over = !folder;
        if self.reveal(&path)
            && folder
            && let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries)
            && let Some(node) = tree.find(&path)
            && !tree.is_expanded(node)
        {
            let _ = workspace::toggle_directory(tree, node, &Listed(queries.as_ref()));
        }
        if !folder {
            self.action = Some(open(path, place));
        }
    }

    /// A key for the search field.
    fn search_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Escape if self.search.is_active() => self.search.clear(),
            // Back to the tree (the app moves the keyboard on).
            ViewerKey::Escape => {
                self.keys = Keys::Tree;
                return false;
            }
            ViewerKey::Enter => {
                self.take_result(self.search.selected_index());
                return true;
            }
            ViewerKey::Up => self.search.move_selection(-1),
            ViewerKey::Down => self.search.move_selection(1),
            ViewerKey::PageUp => self.search.move_selection(-10),
            ViewerKey::PageDown => self.search.move_selection(10),
            ViewerKey::Tab => return false,
            _ => {
                if !reach.edit(&mut self.search.input, key) {
                    return false;
                }
                self.refresh_search();
            }
        }
        true
    }

    /// A key for the new entry's name.
    fn entry_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(pending) = self.view.pending_mut() else {
            self.keys = Keys::Tree;
            return false;
        };
        match key {
            ViewerKey::Escape => self.cancel_entry(),
            ViewerKey::Enter => self.commit_entry(),
            ViewerKey::Tab
            | ViewerKey::Up
            | ViewerKey::Down
            | ViewerKey::PageUp
            | ViewerKey::PageDown => {}
            _ => {
                reach.edit(&mut pending.input, key);
                self.validate_entry();
            }
        }
        true
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

/// Opens file `path` (at `place`: a 0-based line and column).
fn open(path: PathBuf, place: Option<(u32, u32)>) -> ToolPanelAction {
    ToolPanelAction::View(ViewerAction::OpenFile {
        path,
        line: place.map(|(l, _)| l + 1),
        column: place.map(|(_, c)| c + 1),
    })
}

impl ToolPanel for ProjectPanel {
    fn update(&mut self, ctx: &PanelContext<'_>) {
        self.active = ctx.path.map(Path::to_path_buf);
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(view) = data.downcast_ref::<ProjectView>() {
            self.follow(&view.events);
            self.search.indexing = view.indexing;
            self.set_project(view.queries.clone());
        } else if let Some(request) = data.downcast_ref::<EntriesRequest>() {
            self.request(request);
        }
    }

    fn poll(&mut self) -> bool {
        let found = self.finder.take().map(|results| self.search.set_results(results)).is_some();
        self.apply_changes() | found
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let (Some(waker), Ok(mut slot)) = (&ctx.waker, self.waker.lock())
            && slot.is_none()
        {
            *slot = Some(Arc::clone(waker));
        }
        self.restyle(ctx.palette);
        self.layout(ctx.bounds, ctx.scale);
        let Some(tree) = &mut self.tree else { return };
        // The panel grown (or rows gone) since the last scroll: kept within the new limit,
        // or the rows above stay hidden until the next scroll.
        self.view.scroll_by(0.0, 0.0, tree);
        self.search.scroll_by(0.0);
        let (palette, icons) = (ctx.palette, ctx.icons);
        let queries = self.queries.clone();
        let cache = &self.file_icons;
        cache.borrow_mut().stamp(icons);
        // The icons' registry decides (the chosen set's, else ours by language).
        let file_icon =
            |name: &str| cache.borrow_mut().icon(name, icons, palette, queries.as_deref());
        let folder_color = |name: &str| {
            let key = queries.as_ref()?.folder_color(name)?;
            Some(palette.folder_tint(key))
        };
        self.view.set_path_marks(queries.as_ref().and_then(|q| q.path_marks()));
        let search_focused = ctx.focused && self.keys == Keys::Search;
        self.search.paint_field(search_focused, icons, ctx.text, ctx.scene);
        if self.search.is_active() {
            self.search.paint_results(icons, &file_icon, ctx.text, ctx.scene);
        } else {
            self.view.paint(tree, ctx.text, ctx.scene, icons, &file_icon, &folder_color);
        }
        let mut ctx = ctx;
        self.paint_drag(&mut ctx, &file_icon);
    }

    fn header(&self) -> Option<Vec<PanelButton>> {
        let button =
            |id, icon, key: &str, group| PanelButton { id, icon, tooltip: i18n::t(key), group };
        Some(vec![
            button("new-file", "plus", "tooltip.new_file", 0),
            button("locate", "crosshair", "tooltip.locate", 0),
            button("expand", "chevrons-expand", "tooltip.expand", 1),
            button("collapse", "chevrons-collapse", "tooltip.collapse", 1),
            button("hide", "minus", "tooltip.hide", 2),
        ])
    }

    fn header_icon(&self) -> Option<&'static str> {
        Some("folder")
    }

    fn header_action(&mut self, id: &str) -> Option<ToolPanelAction> {
        match id {
            "new-file" => {
                self.new_entry(false);
            }
            "locate" => {
                if let Some(path) = self.active.clone() {
                    self.search.clear();
                    self.reveal(&path);
                }
            }
            "expand" => {
                if let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries)
                    && let Err(e) = workspace::expand_more(tree, &Listed(queries.as_ref()))
                {
                    tracing::warn!(error = %e, "the folders can't be read");
                }
            }
            "collapse" => {
                if let Some(tree) = &mut self.tree {
                    tree.collapse_all();
                    self.view.scroll_by_pixels(f32::MIN, tree);
                }
            }
            "hide" => return Some(ToolPanelAction::View(ViewerAction::RunCommand(TOGGLE.into()))),
            _ => {}
        }
        None
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ToolPanelAction> {
        let action = self.click(p);
        // In a text field: the cursor where it was clicked; a double click selects it all.
        let input = match self.keys {
            Keys::Search => Some(&mut self.search.input),
            Keys::NewEntry => self.view.pending_mut().map(|p| &mut p.input),
            Keys::Tree => None,
        };
        if let Some(input) = input.filter(|i| i.contains(p)) {
            if clicks >= 2 { input.select_all() } else { input.click(p.x) }
        }
        action
    }

    /// ⌘ / Ctrl or ⇧ on a row: the selection grown, nothing opened.
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ToolPanelAction> {
        let on_rows = !self.search.is_active() && self.view.bounds().contains(p);
        if (mods.primary || mods.shift) && on_rows && self.view.pending().is_none() {
            let tree = self.tree.as_mut()?;
            if self.view.select_more(p, tree, mods.primary) {
                self.keys = Keys::Tree;
                return None;
            }
        }
        self.press(p, clicks)
    }

    fn click(&mut self, p: Point) -> Option<ToolPanelAction> {
        // The tree's scrollbar (when shown): the rows scrolled there.
        if !self.search.is_active() && self.view.bar_press(p) {
            return None;
        }
        if self.search.clear_button().is_some_and(|r| r.contains(p)) {
            self.search.clear();
            self.keys = Keys::Search;
            return None;
        }
        if self.search.field_contains(p) {
            self.settle_entry();
            if let Some(action) = self.action.take() {
                return Some(action);
            }
            self.keys = Keys::Search;
            return Some(ToolPanelAction::RefreshProject);
        }
        if self.search.list_contains(p) {
            if let Some(index) = self.search.result_at(p) {
                self.take_result(index);
            }
            return self.action.take();
        }
        let action = self.view.click(p, self.tree.as_mut()?)?;
        // A click elsewhere keeps the name typed (as ↵), or gives up an empty one; a rename
        // asked for goes first (the row clicked waits for the next click).
        self.settle_entry();
        if let Some(asked) = self.action.take() {
            return Some(asked);
        }
        self.keys = Keys::Tree;
        let (tree, queries) = (self.tree.as_mut()?, self.queries.as_ref()?);
        let (TreeAction::Toggle(id) | TreeAction::Open(id)) = action;
        self.press = Some((tree.path(id), p));
        match action {
            TreeAction::Toggle(dir) => {
                if let Err(e) = workspace::toggle_directory(tree, dir, &Listed(queries.as_ref())) {
                    tracing::warn!(path = %tree.path(dir).display(), error = %e, "can't read");
                }
                None
            }
            TreeAction::Open(file) => Some(open(tree.path(file), None)),
        }
    }

    fn hover(&mut self, p: Point) -> bool {
        let mut changed = self.search.hover(p);
        if let Some(tree) = &mut self.tree {
            changed |= self.view.hover(p, tree);
        }
        if !self.search.is_active() {
            changed |= self.view.bar_hover(p);
        }
        changed
    }

    fn clickable(&self, p: Point) -> bool {
        let row = !self.search.is_active() && self.view.bounds().contains(p);
        row || self.search.clear_button().is_some_and(|r| r.contains(p))
            || self.search.list_contains(p)
    }

    fn focus_lost(&mut self) {
        if self.keys == Keys::NewEntry {
            self.settle_entry();
        }
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        (self.search.field_contains(p) && !self.clickable(p)).then_some(ViewerCursor::Text)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let tree = self.tree.as_ref()?;
        if self.search.is_active() {
            return None;
        }
        // A mark: what it means.
        if let Some((rect, id)) = self.view.mark_at(p) {
            let key = match tree.mark(id) {
                _ if tree.is_absent(id) => "absent",
                Mark::None => "absent",
                Mark::Local => "local",
                Mark::Changed => "changed",
                Mark::Behind => "behind",
                Mark::Conflict => "conflict",
            };
            return Some(Tip::new(rect, i18n::t(&format!("files.mark_{key}"))));
        }
        // A note: all of it.
        if let Some((rect, id)) = self.view.note_at(p)
            && let Some(note) = tree.note(id)
        {
            return Some(Tip::new(rect, note.text.to_string()));
        }
        // A name cut short: the whole of it.
        let (rect, id) = self.view.cut_name_at(p)?;
        Some(Tip::new(rect, tree.name(id)))
    }

    fn scroll(&mut self, dy: f32) -> bool {
        if self.search.is_active() {
            return self.search.scroll_by(dy);
        }
        let Some(tree) = &mut self.tree else { return false };
        self.view.scroll_by(0.0, dy, tree)
    }

    fn wants_text(&self) -> bool {
        matches!(self.keys, Keys::Search | Keys::NewEntry)
    }

    /// Its rows have the keys: its commands' own (Rename's ⇧F6) work here, before the
    /// editor's.
    fn keyed(&self) -> bool {
        self.keys == Keys::Tree
    }

    fn text_input(&mut self, text: &str) -> bool {
        match self.keys {
            Keys::Search => {
                self.search.input.insert(text);
                self.refresh_search();
            }
            Keys::NewEntry => {
                let Some(pending) = self.view.pending_mut() else { return false };
                pending.input.insert(text);
                self.validate_entry();
            }
            Keys::Tree => return false,
        }
        true
    }

    /// Typing on the tree's rows searches: the search field takes the keyboard, the text
    /// typed replacing what it held (spaces alone don't start it).
    fn text_on_rows(&mut self, text: &str) -> bool {
        if self.keys != Keys::Tree || text.trim().is_empty() || text.chars().any(char::is_control) {
            return false;
        }
        self.view.cancel_new();
        self.keys = Keys::Search;
        self.search.input.set_text("");
        self.text_input(text)
    }

    fn selected_text(&self) -> Option<String> {
        let input = match self.keys {
            Keys::Search => &self.search.input,
            Keys::NewEntry => &self.view.pending()?.input,
            Keys::Tree => return None,
        };
        Some(input.selected_text().to_owned()).filter(|s| !s.is_empty())
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match self.keys {
            Keys::Search => self.search_key(key, reach),
            Keys::NewEntry => self.entry_key(key, reach),
            // Delete (Explorer's key), ⌫ (a Mac's "delete"; ⌘⌫ comes as a command): the
            // selection to the trash, asked first as from the menu.
            Keys::Tree if matches!(key, ViewerKey::Delete | ViewerKey::Backspace) => {
                self.command("delete")
            }
            Keys::Tree => false,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        let input = match self.keys {
            Keys::Search => &mut self.search.input,
            Keys::NewEntry => match self.view.pending_mut() {
                Some(pending) => &mut pending.input,
                None => return false,
            },
            // The tree's entries: the module's commands, on its selection.
            Keys::Tree => {
                let command = match id {
                    "copy" => operations::COPY,
                    "cut" => operations::CUT,
                    "paste" => operations::PASTE,
                    "delete" => operations::DELETE,
                    "rename" => operations::RENAME,
                    _ => return false,
                };
                if self.view.selected().is_none() {
                    return false;
                }
                self.action =
                    Some(ToolPanelAction::View(ViewerAction::RunCommand(command.0.into())));
                return true;
            }
        };
        match id {
            "select_all" => input.select_all(),
            _ => return false,
        }
        true
    }

    fn context_at(&mut self, p: Point) -> Option<ActionContext> {
        if self.search.is_active() {
            return None;
        }
        let tree = self.tree.as_mut()?;
        // On a row: that one (selected), or every row selected when it's one of them. In the
        // empty room under the rows: what's selected, else the project's root — the
        // menu shows there too.
        let id = match self.view.node_at(p, tree) {
            Some(id) if self.view.is_selected(id) && self.view.selection(tree).len() > 1 => {
                self.keys = Keys::Tree;
                return Self::context_of(tree, &self.view.selection(tree));
            }
            Some(id) => {
                self.view.select(Some(id));
                id
            }
            None if self.view.bounds().contains(p) => {
                // What's selected and shown (a selection hidden since — its folder collapsed —
                // isn't what's shown), else the root.
                tree.refresh_visible();
                let selected = self.view.selection(tree);
                self.keys = Keys::Tree;
                if selected.is_empty() {
                    return Self::context_of(tree, &[tree.root()]);
                }
                return Self::context_of(tree, &selected);
            }
            None => return None,
        };
        self.keys = Keys::Tree;
        Self::context_of(tree, &[id])
    }

    fn context(&self) -> Option<ActionContext> {
        if self.keys != Keys::Tree {
            return None;
        }
        let tree = self.tree.as_ref()?;
        Self::context_of(tree, &self.view.selection(tree))
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.view.bar_drag(p) {
            return true;
        }
        let scale = self.view.scale();
        self.drag_to(p, scale)
    }

    /// Its scrollbar fading: a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.view.next_frame()
    }

    fn release(&mut self) -> bool {
        if self.view.bar_release() {
            return true;
        }
        self.press = None;
        let Some(drag) = self.drag.take() else { return false };
        self.view.set_drop_target(None);
        if let Some(into) = drag.into {
            self.action = Some(ToolPanelAction::Move { from: drag.path, into });
        }
        true
    }

    fn take_action(&mut self) -> Option<ToolPanelAction> {
        self.action.take()
    }

    fn keeps_keyboard(&mut self) -> bool {
        !std::mem::take(&mut self.hand_over)
    }
}

#[cfg(test)]
#[path = "../tests/unit/panel.rs"]
mod tests;
