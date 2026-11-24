//! The panel's search: its results asked for on the search's thread, one taken (a folder
//! shown, a file opened at the place typed), its field's press and keys.

use std::path::Path;
use std::sync::Arc;

use kernel::Point;
use ui_components::{KeyReach, ToolPanelAction, ViewerKey};

use super::{FILE_RESULTS, FOLDER_RESULTS, Keys, ProjectPanel, open};

impl ProjectPanel {
    /// The search's results for its text (from the latest queries): asked for on the
    /// search's thread, shown when they come (`poll`); none at once for no text.
    pub(super) fn refresh_search(&mut self) {
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

    /// Search result `index` taken: a folder is shown open in the tree, a file opened (at
    /// the place typed after its name, `User.php:12`).
    pub(super) fn take_result(&mut self, index: usize) {
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
            && let Some(tree) = &self.tree
            && let Some(node) = tree.find(&path)
            && !tree.is_expanded(node)
        {
            self.toggle_dir(node);
        }
        if !folder {
            self.action = Some(open(path, place));
        }
    }

    /// A press on the search field: the keys to it (a name being typed in the tree kept as
    /// ↵ would first); its caret put and held for a drag, or its × emptying it (no results).
    pub(super) fn press_search(&mut self, p: Point, clicks: u32) -> Option<ToolPanelAction> {
        self.settle_entry();
        if let Some(action) = self.action.take() {
            return Some(action);
        }
        self.keys = Keys::Search;
        if self.search.press(p, clicks) == Some(true) {
            self.refresh_search();
            return None;
        }
        self.selecting = true;
        Some(ToolPanelAction::RefreshProject)
    }

    /// A key for the search field.
    pub(super) fn search_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
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
                let (used, edited) = self.search.key(key, reach);
                if edited {
                    self.refresh_search();
                }
                return used;
            }
        }
        true
    }
}
