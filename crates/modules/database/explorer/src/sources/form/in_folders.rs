//! The sources' folders: the list's rows by folder, folders made, renamed, colored, moved
//! and taken away, where something dragged can go, what changed in them for Apply.

use super::*;

/// A row of the list on the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListRow {
    /// A list's title (the project's, every project's).
    Section(Scope),
    /// A list with nothing in it (or nothing the filter keeps).
    Empty(Scope),
    Folder {
        id: FolderId,
        depth: u16,
        open: bool,
        count: usize,
    },
    /// Entry `index`, `depth` folders in.
    Source {
        index: usize,
        depth: u16,
    },
}

/// Where something dragged (or moved from a menu) goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// A list's top.
    Top(Scope),
    Folder(FolderId),
}

/// What's selected in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Source(DataSourceId),
    Folder(FolderId),
}

impl Editor {
    /// The list's rows: by list, each folder (by name) before the sources at its level, a
    /// closed folder's insides left out. With a filter: the sources it keeps with the
    /// folders on their way, and every folder named like it with all it holds.
    pub fn list_rows(&self) -> Vec<ListRow> {
        let typed = self.filter.text().trim().to_lowercase();
        let filtering = !typed.is_empty();
        let mut out = Vec::new();
        for scope in [Scope::Project, Scope::Global] {
            out.push(ListRow::Section(scope));
            let before = out.len();
            self.level_rows((scope, None, 0), &typed, filtering, false, &mut out);
            if out.len() == before {
                out.push(ListRow::Empty(scope));
            }
        }
        out
    }

    fn level_rows(
        &self,
        (scope, parent, depth): (Scope, Option<FolderId>, u16),
        typed: &str,
        filtering: bool,
        all: bool,
        out: &mut Vec<ListRow>,
    ) {
        for f in folders::children(&self.folders, scope, parent) {
            let named = all || (filtering && f.name.to_lowercase().contains(typed));
            let open = filtering || !self.collapsed.contains(&f.id);
            let count = self.count_in(f.id);
            let at = out.len();
            out.push(ListRow::Folder { id: f.id, depth, open, count });
            if open {
                self.level_rows((scope, Some(f.id), depth + 1), typed, filtering, named, out);
            }
            // Filtering: a folder neither named like it nor holding a match goes.
            if filtering && !named && out.len() == at + 1 {
                out.truncate(at);
            }
        }
        for (index, e) in self.entries.iter().enumerate() {
            if e.source.scope != scope || self.folder_of(&e.source) != parent {
                continue;
            }
            if filtering && !all && !e.source.name.to_lowercase().contains(typed) {
                continue;
            }
            out.push(ListRow::Source { index, depth });
        }
    }

    /// The folder `source` is shown in (none known of its list: the top).
    pub(super) fn folder_of(&self, source: &DataSource) -> Option<FolderId> {
        source.folder.filter(|id| {
            folders::folder(&self.folders, *id).is_some_and(|f| f.scope == source.scope)
        })
    }

    /// The sources in folder `id` and those inside it.
    pub fn count_in(&self, id: FolderId) -> usize {
        let mut inside = folders::descendants(&self.folders, id);
        inside.push(id);
        self.entries.iter().filter(|e| e.source.folder.is_some_and(|f| inside.contains(&f))).count()
    }

    pub fn folder(&self, id: FolderId) -> Option<&SourceFolder> {
        folders::folder(&self.folders, id)
    }

    /// The selected folder, if a folder is selected.
    pub fn current_folder(&self) -> Option<&SourceFolder> {
        self.folder(self.selected_folder?)
    }

    /// Folder `id` opened or closed in the list.
    pub fn toggle_folder(&mut self, id: FolderId) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
    }

    /// Folder `id` selected (its name and color shown on the right).
    pub fn select_folder(&mut self, id: FolderId) {
        if self.folder(id).is_none() {
            return;
        }
        self.select(None);
        self.selected_folder = Some(id);
        self.folder_name =
            TextInput::new(&self.folder(id).map(|f| f.name.clone()).unwrap_or_default());
    }

    /// Where a new folder goes: inside the selected folder, beside the selected source
    /// (in its folder), else at the project's top.
    pub fn new_folder_place(&self) -> Place {
        if let Some(f) = self.current_folder() {
            return Place::Folder(f.id);
        }
        match self.current() {
            Some(e) => match self.folder_of(&e.source) {
                Some(id) => Place::Folder(id),
                None => Place::Top(e.source.scope),
            },
            None => Place::Top(Scope::Project),
        }
    }

    /// A new folder at `place` (its name unique beside it), selected; its id.
    pub fn add_folder(&mut self, place: Place, name: &str) -> FolderId {
        let (scope, parent) = match place {
            Place::Top(scope) => (scope, None),
            Place::Folder(id) => match self.folder(id) {
                Some(f) => (f.scope, Some(id)),
                None => (Scope::Project, None),
            },
        };
        let taken: Vec<String> = folders::children(&self.folders, scope, parent)
            .iter()
            .map(|f| f.name.clone())
            .collect();
        let name = database::unique_name(name, taken.iter().map(String::as_str));
        let mut folder = SourceFolder::new(name, scope);
        folder.parent = parent;
        let id = folder.id;
        self.folders.push(folder);
        if let Some(parent) = parent {
            self.collapsed.remove(&parent);
        }
        self.select_folder(id);
        // Its name selected: what's typed replaces it.
        self.folder_name.select_all();
        id
    }

    /// The selected folder's name as typed.
    pub(super) fn store_folder_name(&mut self) {
        let name = self.folder_name.text().trim().to_owned();
        let Some(id) = self.selected_folder else { return };
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.name = name;
        }
    }

    pub fn set_folder_color(&mut self, id: FolderId, color: Option<SourceColor>) {
        if let Some(f) = self.folders.iter_mut().find(|f| f.id == id) {
            f.color = color;
        }
    }

    /// The selected source's own color (`None`: its folder's).
    pub fn set_color(&mut self, color: Option<SourceColor>) {
        if let Some(entry) = self.current_mut() {
            entry.source.color = color;
        }
    }

    /// The color entry `index` is shown in (its own, else its folder's).
    pub fn color_of(&self, index: usize) -> Option<SourceColor> {
        let source = &self.entries.get(index)?.source;
        source.color.or_else(|| {
            self.folder_of(source).and_then(|f| folders::folder_color(&self.folders, f))
        })
    }

    /// Folder `id`'s color as shown (its own, else its parent's).
    pub fn folder_color(&self, id: FolderId) -> Option<SourceColor> {
        folders::folder_color(&self.folders, id)
    }

    /// Source `id` moved to `place` (another list's: it goes to that list too).
    pub fn move_source(&mut self, id: DataSourceId, place: Place) -> bool {
        let (scope, folder) = match place {
            Place::Top(scope) => (scope, None),
            Place::Folder(f) => match self.folder(f) {
                Some(folder) => (folder.scope, Some(f)),
                None => return false,
            },
        };
        let Some(entry) = self.entries.iter_mut().find(|e| e.source.id == id) else { return false };
        if entry.source.scope == scope && entry.source.folder == folder {
            return false;
        }
        entry.source.folder = folder;
        entry.source.scope = scope;
        if let Some(folder) = folder {
            self.collapsed.remove(&folder);
        }
        self.sort();
        true
    }

    /// Folder `id` moved to `place` (its own list only: never into itself nor one inside).
    pub fn move_folder(&mut self, id: FolderId, place: Place) -> bool {
        let Some(scope) = self.folder(id).map(|f| f.scope) else { return false };
        let parent = match place {
            Place::Top(s) if s == scope => None,
            Place::Top(_) => return false,
            Place::Folder(p) => Some(p),
        };
        let moved = folders::move_folder(&mut self.folders, id, parent);
        if moved && let Some(p) = parent {
            self.collapsed.remove(&p);
        }
        moved
    }

    /// The places `item` can be moved to: each list's top, then every folder it may go in.
    pub fn places_for(&self, item: Item) -> Vec<Place> {
        let mut out = Vec::new();
        let scopes: Vec<Scope> = match item {
            Item::Source(_) => vec![Scope::Project, Scope::Global],
            Item::Folder(id) => self.folder(id).map(|f| vec![f.scope]).unwrap_or_default(),
        };
        for scope in scopes {
            out.push(Place::Top(scope));
            let mut stack: Vec<FolderId> =
                folders::children(&self.folders, scope, None).iter().rev().map(|f| f.id).collect();
            while let Some(f) = stack.pop() {
                let fine = match item {
                    Item::Source(_) => true,
                    Item::Folder(id) => folders::can_move_folder(&self.folders, id, Some(f)),
                };
                if fine {
                    out.push(Place::Folder(f));
                }
                for c in folders::children(&self.folders, scope, Some(f)).iter().rev() {
                    stack.push(c.id);
                }
            }
        }
        out
    }

    /// Folder `id` taken away: what it held moves up (no source goes with it).
    pub fn delete_folder(&mut self, id: FolderId) {
        let mut sources: Vec<DataSource> = self.entries.iter().map(|e| e.source.clone()).collect();
        if !folders::delete_folder(&mut self.folders, &mut sources, id) {
            return;
        }
        for (entry, source) in self.entries.iter_mut().zip(sources) {
            entry.source.folder = source.folder;
        }
        if self.selected_folder == Some(id) {
            self.selected_folder = None;
        }
        self.collapsed.remove(&id);
    }

    /// The folders changed here, by list (Apply sets them).
    pub(super) fn changed_folders(&self) -> Vec<(Scope, Vec<SourceFolder>)> {
        [Scope::Project, Scope::Global]
            .into_iter()
            .filter_map(|scope| {
                let now: Vec<SourceFolder> =
                    self.folders.iter().filter(|f| f.scope == scope).cloned().collect();
                let was: Vec<SourceFolder> =
                    self.original_folders.iter().filter(|f| f.scope == scope).cloned().collect();
                (now != was).then_some((scope, now))
            })
            .collect()
    }
}
