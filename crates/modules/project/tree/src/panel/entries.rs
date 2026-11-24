//! A new file or folder named in place, or an entry renamed: started, checked as typed,
//! made or renamed, cancelled or settled by a click elsewhere, its name's keys.

use std::path::Path;

use ui_components::{KeyReach, ToolPanelAction, ViewerKey};
use workspace::NodeKind;

use super::{Keys, Listed, ProjectPanel, open};

impl ProjectPanel {
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
    pub(super) fn validate_entry(&mut self) {
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
    pub(super) fn settle_entry(&mut self) {
        let Some(pending) = self.view.pending() else { return };
        if !pending.input.text().trim().is_empty() {
            self.commit_entry();
        }
        if self.view.pending().is_some() {
            self.cancel_entry();
        }
    }

    /// A key for the new entry's name.
    pub(super) fn entry_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
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
}
