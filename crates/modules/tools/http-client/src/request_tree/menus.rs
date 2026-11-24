//! The tree's right-click menus (a folder's, a request's, the window's) and their
//! commands.

use super::*;

impl RequestTree {
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
}
