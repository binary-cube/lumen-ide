//! Changed files as a tree (git: a commit's in the Log, a file's history's, the working
//! tree's in Changes; a remote project's files to upload): the project tree's rows (the
//! files' icons by type, folders with their chevron, the indentation guides, hover and
//! selection: [`crate::tree`]), and
//! more — folders with a single child folded onto one row (`FrontOffice/AI/Service/Chat`)
//! with how many files they hold, each file's state (its letter, its name colored: added
//! green, modified blue, deleted grey and struck through, renamed with its old name), the
//! count on top, a search over names and paths (fuzzy, as Go to File: the folders with
//! something found stay, what matched lit) and a Tree / List switch (kept in the project).
//! Keys as in the project tree: ← → fold and open, ↵ the change; Expand / Collapse All.
//! Several files can be chosen (⌘ / Ctrl-click one more, ⇧-click a range, ⌘A all shown):
//! a folder chosen is the files under it.
//!
//! The rows are worked out when the files, the search or what's open change, never per
//! frame; a frame draws only the rows in view.

use std::collections::HashSet;

use i18n::{t, tr};
use kernel::{FuzzyStyle, Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{TextInput, draw_text};

use crate::ds::{self, Ctx, radius};
use crate::tree::{self, EntryIcon, EntryLayout, FileIcons, TreeList};
use crate::{EdgeScrollbar, Palette, PressMods, ViewerKey};

/// Paths: words start after `/ . _ -` (as Go to File).
const PATHS: FuzzyStyle = FuzzyStyle {
    separators: &['/', '.', '_', '-', ' '],
    consecutive_bonus: 15,
    boundary_bonus: 10,
    gap_penalty: true,
    prefer_boundaries: true,
};

/// How a file changed: its letter and its name's color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeState {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// New, not tracked yet.
    Untracked,
    /// Changed on both sides (a merge's, a server's).
    Conflicted,
    /// Couldn't go (offline, refused): its note says why.
    Failed,
}

/// A changed file (relative to the root, with `/`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    /// Where it was, for a rename.
    pub old_path: Option<String>,
    pub state: ChangeState,
    /// Said after its name, muted (why it couldn't go).
    pub note: Option<String>,
}

impl ChangedFile {
    pub fn new(path: impl Into<String>, state: ChangeState) -> Self {
        Self { path: path.into(), old_path: None, state, note: None }
    }
}

/// What a row is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowKind {
    /// A folder (its path, the folders folded onto it in its label) holding `count` files.
    Folder { key: String, label: String, count: usize, expanded: bool },
    /// File `index` (of the list given); the bytes of its label the search matched.
    File { index: usize, lit: Vec<usize> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub depth: usize,
    pub kind: RowKind,
}

/// What a press or a key asks of the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeEvent {
    /// File `i`'s change (a double click, ↵).
    Open(usize),
}

/// The parts that take a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Row(usize),
    Search,
    Mode,
    ExpandAll,
    CollapseAll,
}

/// The header's height (the count and its buttons, the search), logical pixels.
const HEADER_H: f32 = 34.0;
const SEARCH_H: f32 = 28.0;
/// The narrowest the search may be in a head of one row; narrower, it goes under.
const ROW_SEARCH_MIN: f32 = 140.0;

/// A head of one row: the count, the search, the buttons — aligned with what's beside the
/// tree (the log's filter bar). Logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadRow {
    /// The row's height (what it's centered in).
    pub row: f32,
    /// The whole head's (the rows start under it).
    pub total: f32,
}

pub struct FilesTree {
    files: Vec<ChangedFile>,
    /// The header's count: a message taking `{count}` (`changed_files.changed` unless set).
    pub count_key: &'static str,
    /// Its head in one row, as tall as given (`None`: the count with the buttons, the search
    /// under them).
    pub head_row: Option<HeadRow>,
    /// At the last paint its head was in one row (there was room for the search in it).
    in_row: bool,
    /// The files chosen (indexes), and the row a ⇧-click's range starts from.
    marked: HashSet<usize>,
    anchor: Option<RowKind>,
    /// The files the search found (all without one): what a folder chosen holds.
    found: Vec<usize>,
    /// A plain list (sorted by path) rather than a tree.
    pub flat: bool,
    collapsed: HashSet<String>,
    pub query: TextInput,
    /// The search has the keys; it was pressed (a drag selects in it).
    pub searching: bool,
    selecting: bool,
    rows: Vec<Row>,
    /// The row chosen, by what it is (it stays itself when rows change).
    selected: Option<RowKind>,
    scroll: f32,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    list: Rect,
    row_h: f32,
    icons: FileIcons,
}

impl Default for FilesTree {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            count_key: "changed_files.changed",
            head_row: None,
            in_row: false,
            marked: HashSet::new(),
            anchor: None,
            found: Vec::new(),
            flat: false,
            collapsed: HashSet::new(),
            query: TextInput::default(),
            searching: false,
            selecting: false,
            rows: Vec::new(),
            selected: None,
            scroll: 0.0,
            bar: EdgeScrollbar::default(),
            hits: Vec::new(),
            hover: None,
            list: Rect::default(),
            row_h: 0.0,
            icons: FileIcons::default(),
        }
    }
}

impl std::fmt::Debug for FilesTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilesTree").field("files", &self.files.len()).finish_non_exhaustive()
    }
}

/// A folder of the tree being built: its subfolders and files (indexes), by name.
#[derive(Default)]
struct Folder {
    folders: std::collections::BTreeMap<String, Folder>,
    files: Vec<(String, usize)>,
}

impl Folder {
    fn count(&self) -> usize {
        self.files.len() + self.folders.values().map(Folder::count).sum::<usize>()
    }
}

impl FilesTree {
    /// The files shown (the selection — by path —, what's open and the search kept).
    pub fn set_files(&mut self, files: Vec<ChangedFile>) {
        if files == self.files {
            return;
        }
        let chosen = self.selected_file().map(|i| self.files[i].path.clone());
        let marked: HashSet<String> =
            self.marked.iter().filter_map(|&i| self.files.get(i)).map(|f| f.path.clone()).collect();
        self.files = files;
        self.selected = None;
        self.anchor = None;
        self.marked =
            (0..self.files.len()).filter(|&i| marked.contains(&self.files[i].path)).collect();
        self.rebuild();
        if let Some(path) = chosen
            && let Some(i) = self.files.iter().position(|f| f.path == path)
        {
            self.selected = Some(RowKind::File { index: i, lit: Vec::new() });
        }
    }

    pub fn files(&self) -> &[ChangedFile] {
        &self.files
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The files chosen (indexes, in the files' order): the rows marked — a folder's files —
    /// else the row chosen's.
    pub fn chosen(&self) -> Vec<usize> {
        let mut out: Vec<usize> = if self.marked.is_empty() {
            self.selected_row().map(|i| self.files_of(i)).unwrap_or_default()
        } else {
            self.marked.iter().copied().collect()
        };
        out.sort_unstable();
        out
    }

    /// Every file shown chosen (⌘A). `false`: none shown.
    pub fn choose_all(&mut self) -> bool {
        self.marked = self.found.iter().copied().collect();
        !self.marked.is_empty()
    }

    /// The files row `i` stands for: a file, or the files found under a folder.
    fn files_of(&self, i: usize) -> Vec<usize> {
        match self.rows.get(i).map(|r| &r.kind) {
            Some(RowKind::File { index, .. }) => vec![*index],
            Some(RowKind::Folder { key, .. }) => {
                let prefix = format!("{key}/");
                self.found
                    .iter()
                    .copied()
                    .filter(|&f| self.files[f].path.starts_with(&prefix))
                    .collect()
            }
            None => Vec::new(),
        }
    }

    /// Row `i` is chosen: its file is (a folder: all of its).
    fn is_marked(&self, i: usize) -> bool {
        let files = self.files_of(i);
        !files.is_empty() && files.iter().all(|f| self.marked.contains(f))
    }

    /// Row `i` chosen as a press with `mods` says: alone, one more (⌘ / Ctrl: or one less),
    /// or the range from the last one chosen (⇧).
    fn choose(&mut self, i: usize, mods: PressMods) {
        let Some(kind) = self.rows.get(i).map(|r| r.kind.clone()) else { return };
        let from =
            self.anchor.as_ref().and_then(|a| self.rows.iter().position(|r| same(&r.kind, a)));
        if mods.shift
            && let Some(from) = from
        {
            let (a, b) = if from <= i { (from, i) } else { (i, from) };
            self.marked = (a..=b).flat_map(|r| self.files_of(r)).collect();
        } else if mods.primary {
            let files = self.files_of(i);
            if self.is_marked(i) {
                for f in files {
                    self.marked.remove(&f);
                }
            } else {
                // The one chosen before counts too (it was chosen alone).
                if self.marked.is_empty()
                    && let Some(before) = self.selected_row()
                {
                    self.marked.extend(self.files_of(before));
                }
                self.marked.extend(files);
            }
            self.anchor = Some(kind.clone());
        } else {
            self.marked = self.files_of(i).into_iter().collect();
            self.anchor = Some(kind.clone());
        }
        self.selected = Some(kind);
    }

    /// The file chosen (its index).
    pub fn selected_file(&self) -> Option<usize> {
        match &self.selected {
            Some(RowKind::File { index, .. }) => Some(*index),
            _ => None,
        }
    }

    /// File `i` chosen for a right click: alone, unless it's among those chosen (the menu
    /// is then theirs).
    fn select_file(&mut self, i: usize) {
        self.selected = Some(RowKind::File { index: i, lit: Vec::new() });
        if !self.marked.contains(&i) {
            self.marked = HashSet::from([i]);
            self.anchor = self.selected.clone();
        }
    }

    fn selected_row(&self) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        self.rows.iter().position(|r| same(&r.kind, selected))
    }

    /// Tree or list (the project's choice).
    pub fn set_flat(&mut self, flat: bool) {
        if self.flat != flat {
            self.flat = flat;
            self.rebuild();
        }
    }

    /// Every folder open (Expand All) or closed (Collapse All).
    pub fn expand_all(&mut self, open: bool) {
        self.collapsed.clear();
        if !open {
            for row in &self.rows {
                if let RowKind::Folder { key, .. } = &row.kind {
                    self.collapsed.insert(key.clone());
                }
            }
            // The folders hidden inside closed ones close too.
            let all = self.all_folders();
            self.collapsed.extend(all);
        }
        self.rebuild();
    }

    fn all_folders(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .files
            .iter()
            .flat_map(|f| f.path.match_indices('/').map(move |(i, _)| f.path[..i].to_owned()))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The rows again (the files, the search, the folders open or the mode changed).
    pub fn rebuild(&mut self) {
        let query: Vec<char> = self.query.text().trim().to_lowercase().chars().collect();
        // The files found (all, without a search) and what matched in their names.
        let mut found: Vec<(usize, Vec<usize>)> = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            if query.is_empty() {
                found.push((i, Vec::new()));
            } else if let Some((_, lit)) = kernel::fuzzy_match(&f.path, &query, &PATHS) {
                // Lit within the name (what a row shows of it).
                let start = f.path.rfind('/').map_or(0, |s| s + 1);
                let lit = if self.flat {
                    lit
                } else {
                    lit.into_iter().filter(|&b| b >= start).map(|b| b - start).collect()
                };
                found.push((i, lit));
            }
        }
        self.found = found.iter().map(|(i, _)| *i).collect();
        self.rows = if self.flat {
            let mut rows: Vec<(usize, Vec<usize>)> = found;
            rows.sort_by(|a, b| self.files[a.0].path.cmp(&self.files[b.0].path));
            rows.into_iter()
                .map(|(index, lit)| Row { depth: 0, kind: RowKind::File { index, lit } })
                .collect()
        } else {
            let mut root = Folder::default();
            for (i, lit) in &found {
                let path = &self.files[*i].path;
                let mut folder = &mut root;
                let mut parts: Vec<&str> = path.split('/').collect();
                let name = parts.pop().unwrap_or_default();
                for part in parts {
                    folder = folder.folders.entry(part.to_owned()).or_default();
                }
                folder.files.push((name.to_owned(), *i));
                let _ = lit;
            }
            let lits: std::collections::HashMap<usize, Vec<usize>> = found.into_iter().collect();
            let mut rows = Vec::new();
            // While searching, every folder with something found is open.
            let collapsed = if query.is_empty() { self.collapsed.clone() } else { HashSet::new() };
            flatten(&root, "", 0, &collapsed, &lits, &mut rows);
            rows
        };
    }

    /// Folder `key` opened or closed.
    fn toggle(&mut self, key: &str) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        self.rebuild();
    }

    fn reveal(&mut self, i: usize) {
        if self.row_h <= 0.0 {
            return;
        }
        let top = i as f32 * self.row_h;
        let height = self.list.size.height;
        if top < self.scroll {
            self.scroll = top;
        } else if top + self.row_h > self.scroll + height {
            self.scroll = top + self.row_h - height;
        }
    }

    /// A key while the tree has the keys: ↑ ↓ among the rows, ← closes (or goes to the
    /// folder above), → opens (or goes into it), ↵ the file's change (a folder: opened /
    /// closed); in the search, editing. `None`: not one of its keys.
    pub fn key(&mut self, key: ViewerKey) -> Option<Option<TreeEvent>> {
        self.key_with(key, crate::KeyReach::default())
    }

    /// [`Self::key`] with how far it reaches in the search (⌥ / Ctrl: a word, ⇧: selecting).
    pub fn key_with(
        &mut self,
        key: ViewerKey,
        reach: crate::KeyReach,
    ) -> Option<Option<TreeEvent>> {
        if self.searching {
            let (word, extend) = (reach.word, reach.extend);
            match key {
                ViewerKey::Escape => {
                    self.searching = false;
                    return Some(None);
                }
                ViewerKey::Backspace => self.query.backspace(word),
                ViewerKey::Delete => self.query.delete(),
                ViewerKey::Left => self.query.move_left(word, extend),
                ViewerKey::Right => self.query.move_right(word, extend),
                ViewerKey::Home => self.query.home(extend),
                ViewerKey::End => self.query.end(extend),
                ViewerKey::Up | ViewerKey::Down | ViewerKey::Enter => {
                    self.searching = false;
                    return self.key_with(key, reach);
                }
                _ => return Some(None),
            }
            if matches!(key, ViewerKey::Backspace | ViewerKey::Delete) {
                self.scroll = 0.0;
                self.rebuild();
            }
            return Some(None);
        }
        let at = self.selected_row();
        let last = self.rows.len().checked_sub(1)?;
        let next = match (key, at) {
            (ViewerKey::Down, Some(i)) => (i + 1).min(last),
            (ViewerKey::Up, Some(i)) => i.saturating_sub(1),
            (ViewerKey::Down | ViewerKey::Up, None) => 0,
            (ViewerKey::Home, _) => 0,
            (ViewerKey::End, _) => last,
            (ViewerKey::Left, Some(i)) => match &self.rows[i].kind {
                RowKind::Folder { key, expanded: true, .. } => {
                    let key = key.clone();
                    self.toggle(&key);
                    return Some(None);
                }
                // The folder above.
                _ => {
                    let depth = self.rows[i].depth;
                    (0..i).rev().find(|&j| self.rows[j].depth < depth).unwrap_or(i)
                }
            },
            (ViewerKey::Right, Some(i)) => match &self.rows[i].kind {
                RowKind::Folder { key, expanded: false, .. } => {
                    let key = key.clone();
                    self.toggle(&key);
                    return Some(None);
                }
                RowKind::Folder { .. } => (i + 1).min(last),
                RowKind::File { .. } => return Some(None),
            },
            (ViewerKey::Enter, Some(i)) => match &self.rows[i].kind {
                RowKind::Folder { key, .. } => {
                    let key = key.clone();
                    self.toggle(&key);
                    return Some(None);
                }
                RowKind::File { index, .. } => return Some(Some(TreeEvent::Open(*index))),
            },
            _ => return None,
        };
        self.choose(next, PressMods::default());
        self.reveal(next);
        Some(None)
    }

    /// What's selected in its search.
    pub fn selected_text(&self) -> Option<String> {
        let selected = self.query.selected_text();
        (self.searching && !selected.is_empty()).then(|| selected.to_owned())
    }

    /// An edit command for its search (⌘A, ⌘X, ⌘Z, ⇧⌘Z): `true` if it has the keys.
    pub fn command(&mut self, id: &str) -> bool {
        if !self.searching {
            return false;
        }
        let edited = match id {
            "select_all" => {
                self.query.select_all();
                false
            }
            "cut" => {
                self.query.insert("");
                true
            }
            "undo" => self.query.undo(),
            "redo" => self.query.redo(),
            _ => return false,
        };
        if edited {
            self.scroll = 0.0;
            self.rebuild();
        }
        true
    }

    /// Text typed into its search (`false`: the search hasn't the keys).
    pub fn text_input(&mut self, typed: &str) -> bool {
        if !self.searching {
            return false;
        }
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.query.insert(&typed);
        self.scroll = 0.0;
        self.rebuild();
        true
    }

    /// A press at `p` (`clicks` in a row): a row chosen (twice: a file's change, a folder
    /// opened / closed), the search, the header's buttons. `None` if it's not on the tree;
    /// `Some(changed_mode)`: what it did.
    pub fn press(&mut self, p: Point, clicks: u32) -> Option<Option<TreeEvent>> {
        self.press_with(p, clicks, PressMods::default())
    }

    /// [`Self::press`] with the modifiers held: ⌘ / Ctrl chooses one more, ⇧ a range.
    pub fn press_with(
        &mut self,
        p: Point,
        clicks: u32,
        mods: PressMods,
    ) -> Option<Option<TreeEvent>> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return Some(None);
        }
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        self.searching = hit == Hit::Search;
        self.selecting = self.searching;
        match hit {
            Hit::Search => self.query.press(p.x, clicks),
            Hit::Mode => {
                self.flat = !self.flat;
                self.rebuild();
            }
            Hit::ExpandAll => self.expand_all(true),
            Hit::CollapseAll => self.expand_all(false),
            Hit::Row(i) => {
                let kind = self.rows.get(i)?.kind.clone();
                self.choose(i, mods);
                if mods.shift || mods.primary {
                    return Some(None);
                }
                match kind {
                    // A folder: its chevron (or a double click) opens / closes it.
                    RowKind::Folder { key, .. } => {
                        let chevron = p.x < self.chevron_end(i);
                        if clicks >= 2 || chevron {
                            self.toggle(&key);
                        }
                    }
                    RowKind::File { index, .. } if clicks >= 2 => {
                        return Some(Some(TreeEvent::Open(index)));
                    }
                    RowKind::File { .. } => {}
                }
            }
        }
        Some(None)
    }

    /// Where row `i`'s chevron ends (a press before it opens / closes the folder).
    fn chevron_end(&self, i: usize) -> f32 {
        let layout = EntryLayout::new(self.scale(), 28.0, 16.0);
        let depth = self.rows.get(i).map_or(0, |r| r.depth);
        layout.icon_rect(layout.x(self.list.min_x(), depth), 0.0).min_x()
    }

    fn scale(&self) -> f32 {
        (self.row_h / 28.0).max(0.1)
    }

    /// The row under `p` for a right click's menu: chosen alone unless it's among those
    /// chosen (the menu is then theirs, [`Self::chosen`]). What it is (a folder: its path).
    pub fn choose_at(&mut self, p: Point) -> Option<RowKind> {
        let i = self.hits.iter().rev().find_map(|(r, h)| match h {
            Hit::Row(i) if r.contains(p) => Some(*i),
            _ => None,
        })?;
        if !self.is_marked(i) {
            self.choose(i, PressMods::default());
        }
        Some(self.rows.get(i)?.kind.clone())
    }

    /// The file under `p` (a right click's menu).
    pub fn file_at(&mut self, p: Point) -> Option<usize> {
        let i = self.hits.iter().rev().find_map(|(r, h)| match h {
            Hit::Row(i) if r.contains(p) => Some(*i),
            _ => None,
        })?;
        let RowKind::File { index, .. } = self.rows.get(i)?.kind.clone() else { return None };
        self.select_file(index);
        Some(index)
    }

    /// When its scrollbar next needs a frame (fading).
    pub fn next_frame(&self) -> Option<std::time::Instant> {
        self.bar.next_frame()
    }

    /// The tree is under `p` (it scrolls under the wheel).
    pub fn contains(&self, p: Point) -> bool {
        self.list.contains(p) || self.hits.iter().any(|(r, _)| r.contains(p))
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        let max = (self.rows.len() as f32 * self.row_h - self.list.size.height).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    pub fn drag(&mut self, p: Point) -> bool {
        // From a press in the search: its text selected.
        if self.selecting {
            self.query.extend_to(p.x);
            return true;
        }
        let Some(offset) = self.bar.drag(p) else { return false };
        let changed = offset != self.scroll;
        self.scroll = offset;
        changed
    }

    pub fn release(&mut self) -> bool {
        self.selecting = false;
        self.bar.release()
    }

    /// The mouse at `p`. `true` if a redraw is needed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        self.bar.hover(p) | changed
    }

    /// The tree in `area`: the count with the buttons (Tree / List, Expand / Collapse All),
    /// the search, the rows in view. `focused`: it has the keys (the row chosen lit);
    /// `queries`: the project's (the files' icons by language).
    /// In `area`; `focused`: it has the keyboard.
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        (area, focused): (Rect, bool),
        queries: Option<&dyn workbench_model::ProjectQueries>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let layer = scene.push_layer(Some(area));
        let row = self.head_row.map_or(HEADER_H, |h| h.row);
        let header = Rect::new(area.min_x(), area.min_y(), area.size.width, px(row));
        let (count_end, buttons_left) = self.paint_header(layer, text, icons, ds, header);
        // In one row between the count and the buttons when there's room; else under them.
        let in_row =
            self.head_row.filter(|_| buttons_left - count_end - px(20.0) >= px(ROW_SEARCH_MIN));
        self.in_row = in_row.is_some();
        let field = match in_row {
            Some(_) => Rect::new(
                count_end + px(12.0),
                (header.min_y() + (header.size.height - px(SEARCH_H)) / 2.0).round(),
                buttons_left - px(8.0) - (count_end + px(12.0)),
                px(SEARCH_H),
            ),
            None => Rect::new(
                area.min_x() + px(10.0),
                header.max_y(),
                (area.size.width - px(20.0)).max(0.0),
                px(SEARCH_H),
            ),
        };
        let placeholder = t("changed_files.search");
        let focus = focused && self.searching;
        ds::search_input(
            &mut crate::LayerPainter::new(layer, text, icons, *ds),
            field,
            &self.query,
            focus,
            &placeholder,
        );
        self.hits.push((field, Hit::Search));
        let top = match (in_row, self.head_row) {
            (Some(head), _) => area.min_y() + px(head.total) + px(6.0),
            _ => field.max_y() + px(6.0),
        };
        let list = Rect::new(area.min_x(), top, area.size.width, (area.max_y() - top).max(0.0));
        self.list = list;
        let layout = EntryLayout::new(ds.scale, 28.0, 16.0);
        self.row_h = layout.row_height;
        let max = (self.rows.len() as f32 * layout.row_height - list.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
        if self.rows.is_empty() {
            let message = if self.files.is_empty() {
                t("changed_files.none")
            } else {
                t("changed_files.no_match")
            };
            let shaped = text.shape_line(&message, &ds.ui_font(-0.5));
            let base = ds::optical_baseline(list.min_y(), layout.row_height, &ds.ui_font(-0.5));
            draw_text(layer, &shaped, list.min_x() + px(18.0), base, p.text_muted);
            return;
        }
        let rows_layer = scene.push_layer(Some(list));
        self.icons.stamp(icons);
        let font = ds.ui_font(0.0);
        let small = ds.ui_font(-1.5);
        let chosen = self.selected_row();
        let range = TreeList::new(layout.row_height, self.rows.len())
            .visible(self.scroll, list.size.height);
        for i in range {
            let top = (list.min_y() + i as f32 * layout.row_height - self.scroll).round();
            let pill = layout.pill(list, top);
            if chosen == Some(i) || (!self.marked.is_empty() && self.is_marked(i)) {
                let color = if focused {
                    p.tree_selection
                } else {
                    p.tree_selection.with_alpha(p.tree_selection.a * 0.6)
                };
                tree::highlight(rows_layer, pill, color, layout.pill_radius());
            } else if self.hover == Some(Hit::Row(i)) {
                tree::highlight(rows_layer, pill, p.tree_hover, layout.pill_radius());
            }
            if let Some(hit) = pill.intersect(&list) {
                self.hits.push((hit, Hit::Row(i)));
            }
            let row = self.rows[i].clone();
            let left = list.min_x();
            layout.guides(rows_layer, left, row.depth, top, p.indent_guide);
            let x = layout.x(left, row.depth);
            let baseline = ds::optical_baseline(top, layout.row_height, &font);
            let right = pill.max_x() - px(8.0);
            match &row.kind {
                RowKind::Folder { key, label, count, expanded } => {
                    let name = key.rsplit('/').next().unwrap_or(key);
                    let entry =
                        EntryIcon::Folder { name, expanded: *expanded, color: p.folder_icon };
                    let icon = tree::entry_icon(
                        rows_layer,
                        icons,
                        &layout,
                        (x, top),
                        entry,
                        (p.text_muted, &|c| c),
                    );
                    let lx = layout.label_x(icon);
                    let files = tr!("changed_files.count", count = *count);
                    let count_w = text.shape_line(&files, &small).width;
                    let room = (right - lx - count_w - px(10.0)).max(0.0);
                    let label = ds::ellipsize_start(text, label, &font, room);
                    let shaped = text.shape_line(&label, &font);
                    draw_text(rows_layer, &shaped, lx, baseline, p.text.with_alpha(0.86));
                    let shaped_count = text.shape_line(&files, &small);
                    draw_text(
                        rows_layer,
                        &shaped_count,
                        lx + shaped.width + px(10.0),
                        ds::optical_baseline(top, layout.row_height, &small),
                        p.text_muted,
                    );
                }
                RowKind::File { index, lit } => {
                    let file = &self.files[*index];
                    let name = if self.flat {
                        file.path.as_str()
                    } else {
                        file.path.rsplit('/').next().unwrap_or(&file.path)
                    };
                    let base_name = file.path.rsplit('/').next().unwrap_or(&file.path);
                    let (icon_id, icon_color) = self.icons.icon(base_name, icons, p, queries);
                    let deleted = file.state == ChangeState::Deleted;
                    let dim = |c: Rgba| if deleted { c.with_alpha(c.a * 0.5) } else { c };
                    let entry = EntryIcon::File { icon: icon_id, color: icon_color };
                    // A file has no chevron: its icon where a folder's would be.
                    let icon = tree::entry_icon(
                        rows_layer,
                        icons,
                        &layout,
                        (x, top),
                        entry,
                        (p.text_muted, &dim),
                    );
                    let lx = layout.label_x(icon);
                    // Its state's badge on the right.
                    let lw = px(ds::CHANGE_BADGE);
                    let badge = Rect::new(
                        (right - lw).round(),
                        (top + (layout.row_height - lw) / 2.0).round(),
                        lw,
                        lw,
                    );
                    ds::change_badge(rows_layer, icons, ds, badge, file.state);
                    let old = file
                        .old_path
                        .as_deref()
                        .map(|o| tr!("changed_files.renamed_from", name = o));
                    let old_w =
                        old.as_ref().map_or(0.0, |o| text.shape_line(o, &small).width + px(10.0));
                    let room = (right - lw - px(10.0) - lx - old_w).max(px(40.0));
                    // Its name kept (its extension too); a path's folders cut first.
                    let label = if self.flat {
                        ds::ellipsize_path(text, name, &font, room)
                    } else {
                        ds::ellipsize_file(text, name, &font, room)
                    };
                    let lit = if label == name { lit.as_slice() } else { &[] };
                    let name_color = name_color(file.state, p);
                    let width = draw_lit(
                        rows_layer,
                        text,
                        &font,
                        &label,
                        (lx, baseline),
                        (name_color, p.accent),
                        lit,
                    );
                    if deleted {
                        // Struck through.
                        let y = (top + layout.row_height / 2.0).round();
                        rows_layer
                            .quad(Quad::filled(Rect::new(lx, y, width, layout.line), name_color));
                    }
                    // After the name: where it was, then its note (cut where the letter is).
                    let mut after = lx + width + px(10.0);
                    let small_base = ds::optical_baseline(top, layout.row_height, &small);
                    if let Some(old) = old {
                        let shaped = text.shape_line(&old, &small);
                        draw_text(rows_layer, &shaped, after, small_base, p.text_muted);
                        after += shaped.width + px(10.0);
                    }
                    if let Some(note) = &file.note {
                        let room = right - lw - px(10.0) - after;
                        if room > px(24.0) {
                            let note = ds::ellipsize(text, note, &small, room);
                            let shaped = text.shape_line(&note, &small);
                            let color = if file.state == ChangeState::Failed {
                                p.warning
                            } else {
                                p.text_muted
                            };
                            draw_text(rows_layer, &shaped, after, small_base, color);
                        }
                    }
                }
            }
        }
        let content = self.rows.len() as f32 * layout.row_height;
        // Hidden at rest: shown while scrolled or hovered.
        self.bar.set_auto_hiding(true);
        self.bar.paint(scene, p, ds.scale, list, content, self.scroll);
    }

    /// "64 files changed", then on the right: Expand All, Collapse All, Tree / List. Where
    /// the count ends and the buttons begin.
    fn paint_header(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        header: Rect,
    ) -> (f32, f32) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let small = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
        let count = tr!("changed_files.changed", count = self.files.len());
        let shaped = text.shape_line(&count, &small);
        draw_text(
            layer,
            &shaped,
            header.min_x() + px(16.0),
            ds::optical_baseline(header.min_y(), header.size.height, &small),
            p.text_muted,
        );
        let size = px(24.0);
        let mid = header.min_y() + header.size.height / 2.0;
        let mut right = header.max_x() - px(10.0);
        let mode_icon = if self.flat { "folder" } else { "rows" };
        for (hit, icon) in [
            (Hit::Mode, mode_icon),
            (Hit::CollapseAll, "chevrons-collapse"),
            (Hit::ExpandAll, "chevrons-expand"),
        ] {
            if self.flat && hit != Hit::Mode {
                continue;
            }
            let r = Rect::new(right - size, (mid - size / 2.0).round(), size, size);
            if self.hover == Some(hit) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(px(radius::MD)));
            }
            if let Some(id) = icons.id(icon) {
                let inner = Rect::new(
                    r.min_x() + px(5.0),
                    r.min_y() + px(5.0),
                    size - px(10.0),
                    size - px(10.0),
                );
                ui_kit::draw_icon(layer, icons, id, inner, p.text_muted);
            }
            self.hits.push((r, hit));
            right = r.min_x() - px(2.0);
        }
        (header.min_x() + px(16.0) + shaped.width, right)
    }

    /// At the last paint its head was in one row ([`Self::head_row`], room for the search).
    pub fn head_in_row(&self) -> bool {
        self.in_row
    }

    /// The tooltip of what's under `p` (the header's buttons).
    pub fn tooltip_at(&self, p: Point) -> Option<crate::Tip> {
        let (r, hit) = self.hits.iter().rev().find(|(r, _)| r.contains(p))?;
        let key = match hit {
            Hit::Mode if self.flat => "changed_files.as_tree",
            Hit::Mode => "changed_files.as_list",
            Hit::ExpandAll => "tooltip.expand",
            Hit::CollapseAll => "tooltip.collapse",
            _ => return None,
        };
        Some(crate::Tip::new(*r, t(key)))
    }
}

/// Two rows are the same one (a file by its index, a folder by its path).
fn same(a: &RowKind, b: &RowKind) -> bool {
    match (a, b) {
        (RowKind::File { index: x, .. }, RowKind::File { index: y, .. }) => x == y,
        (RowKind::Folder { key: x, .. }, RowKind::Folder { key: y, .. }) => x == y,
        _ => false,
    }
}

/// `folder`'s rows under `prefix` at `depth`: its folders first (one holding only one
/// folder folded onto it), then its files.
fn flatten(
    folder: &Folder,
    prefix: &str,
    depth: usize,
    collapsed: &HashSet<String>,
    lits: &std::collections::HashMap<usize, Vec<usize>>,
    rows: &mut Vec<Row>,
) {
    for (name, mut child) in &folder.folders {
        let mut key = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let mut label = name.clone();
        // A folder holding only a folder: one row.
        while child.files.is_empty()
            && child.folders.len() == 1
            && let Some((inner, next)) = child.folders.iter().next()
        {
            key = format!("{key}/{inner}");
            label = format!("{label}/{inner}");
            child = next;
        }
        let expanded = !collapsed.contains(&key);
        rows.push(Row {
            depth,
            kind: RowKind::Folder { key: key.clone(), label, count: child.count(), expanded },
        });
        if expanded {
            flatten(child, &key, depth + 1, collapsed, lits, rows);
        }
    }
    let mut files = folder.files.clone();
    files.sort();
    for (_, index) in files {
        let lit = lits.get(&index).cloned().unwrap_or_default();
        rows.push(Row { depth, kind: RowKind::File { index, lit } });
    }
}

/// A file's letter and its color.
pub fn state_mark(state: ChangeState, p: &Palette) -> (&'static str, Rgba) {
    match state {
        ChangeState::Added => ("A", p.vcs_added),
        ChangeState::Untracked => ("?", p.vcs_added),
        ChangeState::Deleted => ("D", p.text_muted),
        ChangeState::Renamed => ("R", p.vcs_modified),
        ChangeState::Conflicted => ("U", p.danger),
        ChangeState::Modified => ("M", p.vcs_modified),
        ChangeState::Failed => ("!", p.warning),
    }
}

/// A file's name color by its state (added green, modified blue, deleted grey).
fn name_color(state: ChangeState, p: &Palette) -> Rgba {
    match state {
        ChangeState::Added | ChangeState::Untracked => p.vcs_added,
        ChangeState::Modified | ChangeState::Renamed => p.vcs_modified,
        ChangeState::Deleted => p.text_muted,
        ChangeState::Conflicted => p.danger,
        ChangeState::Failed => p.text,
    }
}

/// `label` drawn at `(x, baseline)` in runs of one color (the bytes `lit`: `lit_color`).
/// Returns its width.
pub fn draw_lit(
    layer: &mut Layer,
    text: &mut TextSystem,
    font: &Font,
    label: &str,
    (x, baseline): (f32, f32),
    (color, lit_color): (Rgba, Rgba),
    lit: &[usize],
) -> f32 {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut lit_run = lit.contains(&0);
    for (i, _) in label.char_indices().skip(1) {
        let here = lit.contains(&i);
        if here != lit_run {
            runs.push((start..i, lit_run));
            (start, lit_run) = (i, here);
        }
    }
    runs.push((start..label.len(), lit_run));
    let mut width = 0.0;
    for (range, is_lit) in runs {
        let before =
            if range.start == 0 { 0.0 } else { text.shape_line(&label[..range.start], font).width };
        let shaped = text.shape_line(&label[range], font);
        draw_text(layer, &shaped, x + before, baseline, if is_lit { lit_color } else { color });
        width = before + shaped.width;
    }
    width
}

/// A files tree as a form's control: a press on a row, its search or its buttons (⌘ / ⇧ add
/// to the choice), its scrollbar's thumb held; with the focus its keys and its search; what
/// a press or a key opened is written to `event`.
pub struct TreePart<'a> {
    pub tree: &'a mut FilesTree,
    pub event: &'a mut Option<TreeEvent>,
}

impl std::fmt::Debug for TreePart<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TreePart").finish_non_exhaustive()
    }
}

impl crate::control::Control for TreePart<'_> {
    fn hit(&self, p: Point) -> bool {
        self.tree.contains(p)
    }

    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> crate::control::Press {
        match self.tree.press_with(p, clicks, mods) {
            Some(event) => {
                *self.event = event;
                crate::control::Press::Held
            }
            None => crate::control::Press::Ignored,
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        self.tree.drag(p)
    }

    fn release(&mut self) -> bool {
        self.tree.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.tree.hover(p.unwrap_or(crate::control::NOWHERE))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.tree.scroll_by(dy)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: crate::KeyReach) -> bool {
        match self.tree.key_with(key, reach) {
            Some(event) => {
                *self.event = event;
                true
            }
            None => false,
        }
    }

    fn text(&mut self, typed: &str) -> bool {
        self.tree.text_input(typed)
    }

    fn selected_text(&self) -> Option<String> {
        self.tree.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        self.tree.command(id)
    }

    fn tooltip(&self, p: Point) -> Option<crate::Tip> {
        self.tree.tooltip_at(p)
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.tree.next_frame()
    }
}

#[cfg(test)]
#[path = "../tests/unit/changed_files.rs"]
mod tests;
