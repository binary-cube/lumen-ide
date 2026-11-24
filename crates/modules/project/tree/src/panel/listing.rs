//! The project's folders kept current: its queries taken, its entries' changes followed and
//! read again on the listing's thread (fewer while the window is away), folders opened,
//! "Expand all", what the app or a module asks.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use workbench_model::{EntriesRequest, ProjectQueries};
use workspace::{FileTree, NodeId, NodeKind};

use crate::ProjectTreeView;

use super::{Keys, LIST_NOW, Listed, Listing, Opening, ProjectPanel, Waker, tree_style};

impl ProjectPanel {
    /// The project's queries (a new snapshot when the project or its files change): another
    /// project opens a new tree; the same one keeps it (what's expanded, the selection).
    pub(super) fn set_project(&mut self, queries: Option<Arc<dyn ProjectQueries>>) {
        let root = queries.as_ref().map(|q| q.root());
        if root.as_deref() != self.root() {
            // Another project: what was being opened was the other one's.
            self.opening.clear();
            self.reread.clear();
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
    pub(super) fn follow(&mut self, events: &kernel::EventBus) {
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
    pub(super) fn apply_changes(&mut self) -> bool {
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
        // A folder being read to open changed: what's being read may be older, read again.
        self.reread.extend(dirs.iter().filter(|d| self.opening.contains(*d)).cloned());
        dirs.retain(|d| {
            tree.find(d).is_some_and(|n| tree.kind(n) == NodeKind::Directory && tree.is_loaded(n))
        });
        let mut changed = false;
        // Many folders to read again: on the listing's thread, once the tree is let go.
        let mut later = Vec::new();
        // Read on the listing's thread (before what's read now, which is newer): put in, if
        // still shown.
        self.in_flight = self.in_flight.saturating_sub(listed.len());
        for (opening, batch) in listed {
            for (dir, entries) in batch {
                let Some(node) = tree.find(&dir) else { continue };
                // A folder to open is put in whether read before or not; a change only where
                // it's shown.
                if tree.kind(node) != NodeKind::Directory || !(opening || tree.is_loaded(node)) {
                    continue;
                }
                // Still wanted open (not clicked closed meanwhile).
                let wanted = opening && self.opening.remove(&dir);
                if opening && self.reread.remove(&dir) {
                    later.push(dir.clone());
                }
                match entries {
                    Ok(entries) => {
                        if opening && tree.is_loaded(node) {
                            // Read meanwhile (a change heard): that reading is as new.
                        } else {
                            tree.set_children(node, entries);
                        }
                        if wanted {
                            tree.set_expanded(node, true);
                        }
                        changed = true;
                    }
                    Err(e) => tracing::warn!(path = %dir.display(), error = %e, "can't read"),
                }
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
            later.extend(dirs);
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
        if !later.is_empty() {
            self.list_later(later, false);
        }
        changed
    }

    /// `dirs` read on the listing's thread (in order with the batches before), put in the
    /// tree at a later poll — opened then when `opening`.
    fn list_later(&mut self, dirs: Vec<PathBuf>, opening: Opening) {
        let Some(queries) = &self.queries else { return };
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
                listed.push((opening, read));
            }
            if let Some(wake) = waker {
                wake();
            }
        });
    }

    /// Folder `dir` opened or closed: one read before at once, one never read read on the
    /// listing's thread and opened once it's there (a click never reads the disk).
    pub(super) fn toggle_dir(&mut self, dir: NodeId) {
        let (Some(tree), Some(queries)) = (&mut self.tree, &self.queries) else { return };
        if tree.kind(dir) != NodeKind::Directory {
            return;
        }
        if tree.is_expanded(dir) || !tree.needs_loading(dir) {
            // Nothing to read: closed, or opened with what was read before.
            let _ = workspace::toggle_directory(tree, dir, &Listed(queries.as_ref()));
            return;
        }
        let path = tree.path(dir);
        // Clicked again before it came: the user closes it (read, but left closed).
        if self.opening.remove(&path) {
            return;
        }
        self.opening.insert(path.clone());
        self.list_later(vec![path], true);
    }

    /// "Expand all": the folders read before opened at once; else the closed folders shown
    /// read on the listing's thread and opened when they come (a level per click, as
    /// [`workspace::expand_more`]).
    pub(super) fn expand_more(&mut self) {
        let Some(tree) = &mut self.tree else { return };
        if tree.expand_loaded() {
            return;
        }
        let shown: Vec<NodeId> = tree.visible_rows().iter().map(|r| r.id).collect();
        let closed = shown
            .into_iter()
            .filter(|&id| tree.kind(id) == NodeKind::Directory && !tree.is_expanded(id));
        let mut unread = Vec::new();
        for id in closed.collect::<Vec<_>>() {
            if tree.needs_loading(id) {
                unread.push(tree.path(id));
            } else {
                tree.set_expanded(id, true);
            }
        }
        if !unread.is_empty() {
            self.opening.extend(unread.iter().cloned());
            self.list_later(unread, true);
        }
    }

    /// What the app or a module tells it.
    pub(super) fn request(&mut self, request: &EntriesRequest) {
        match request {
            EntriesRequest::Reveal { path, expand } => {
                self.search.clear();
                self.keys = Keys::Tree;
                if self.reveal(path)
                    && *expand
                    && let Some(tree) = &self.tree
                    && let Some(node) = tree.find(path)
                    && tree.kind(node) == NodeKind::Directory
                    && !tree.is_expanded(node)
                {
                    self.toggle_dir(node);
                }
            }
            EntriesRequest::FocusSearch => {
                self.view.cancel_new();
                self.keys = Keys::Search;
                self.search.field.input.select_all();
            }
            EntriesRequest::NewEntry { is_dir } => {
                self.new_entry(*is_dir);
            }
            EntriesRequest::Rename { path } => {
                self.rename_entry(path);
            }
        }
    }
}
