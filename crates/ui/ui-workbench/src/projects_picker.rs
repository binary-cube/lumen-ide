//! Project picker: a filter field, the actions (open a project, a file,
//! new file), the current project and the project history, with keyboard navigation
//! (↑ ↓ ↵ esc). The same content is shown in two places: in the modal opened from the top
//! bar and on the editor's empty screen (when nothing is open).
//!
//! The history is organized in groups (nested, collapsed or expanded: ← → ↵ or a click;
//! the recent projects in a group of their own), joined by faint guide lines. A project or
//! a group is dragged into another group, or between two rows (a line shows where); near
//! the list's top or bottom edge, the list scrolls. While filtering, the list is flat,
//! each project with its group's path.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use workspace::recent::{Group, GroupId, Place, RECENT, RecentProjects};

use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::control::{Control, Press};
use ui_components::search_field::SearchField;
use ui_kit::{KeyHint, draw_icon, draw_text};

use crate::Painter;
use crate::ds::{self, Ctx};
use crate::modal::{Modal, ModalHit, ModalSize, ModalSpec};
use crate::top_bar::paint_badge;

mod drag;
mod paint;
mod sessions;

/// A project in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    pub name: String,
    /// The displayed path (e.g. `~/work/proiect`).
    pub display: String,
    pub path: std::path::PathBuf,
    /// What a module says it is (a remote project); `None`: a local project.
    pub kind: Option<workbench_model::ProjectKind>,
}

/// The project history shown: its groups and where each project is, and the projects
/// whose folders exist (described), most recent first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecentList {
    pub model: RecentProjects,
    pub projects: Vec<RecentProject>,
    /// The app's past sessions (newest first), to reopen.
    pub sessions: Vec<HomeSession>,
}

/// A past session of the app as listed: when it last changed (`2 hours ago`), whether it
/// ended badly (a crash, a force quit), its windows.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeSession {
    pub id: u128,
    pub when: String,
    pub crashed: bool,
    pub windows: Vec<HomeSessionWindow>,
}

/// A window of a past session: its project's name and path (or "files without a project"),
/// and its open files.
#[derive(Debug, Clone, PartialEq)]
pub struct HomeSessionWindow {
    pub name: String,
    pub detail: String,
    pub project: bool,
    /// Its project's folder (a click on its row opens it; `None`: files opened alone).
    pub path: Option<PathBuf>,
    pub files: Vec<HomeFile>,
}

/// What a right click is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerTarget {
    Project {
        path: PathBuf,
        current: bool,
    },
    Group(GroupId),
    /// The history's title (a group made there goes to the top).
    History,
}

/// What was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerAction {
    /// Search in project (⌘⇧F).
    SearchProject,
    /// A project file (absolute path).
    OpenPath(PathBuf),
    OpenProject,
    /// A project on a server (SFTP / FTP): the remote module's wizard.
    OpenRemote,
    OpenFile,
    NewFile,
    OpenRecent(PathBuf),
    /// The window's project closed (back to the start screen).
    CloseProject,
    /// What the current project's kind lets the user change (a remote project's
    /// connection): its module opens it.
    EditProject(PathBuf),
    Close,
    /// A group collapsed or expanded.
    ToggleGroup(GroupId),
    /// Dropped into a group, at a place.
    MoveProject {
        path: PathBuf,
        into: GroupId,
        place: Place,
    },
    /// Dropped into `parent` (`None`: the top), before a sibling (`None`: last).
    MoveGroup {
        id: GroupId,
        parent: Option<GroupId>,
        before: Option<GroupId>,
    },
    /// The "New group" button.
    NewGroup,
    /// A past session's row (its windows shown or hidden), a window's row in it (its files).
    ToggleSession(u128),
    ToggleSessionWindow(u128, usize),
    /// A past session's "Reopen": its windows, one after the other.
    RestoreSession(u128),
}

impl PickerAction {
    /// It changes the list only: the modal stays open.
    pub fn keeps_open(&self) -> bool {
        matches!(
            self,
            Self::ToggleGroup(_)
                | Self::MoveProject { .. }
                | Self::MoveGroup { .. }
                | Self::NewGroup
                | Self::ToggleSession(_)
                | Self::ToggleSessionWindow(..)
        )
    }
}

/// A file shown on the empty screen of an open project (recent or found).
#[derive(Debug, Clone, PartialEq)]
pub struct HomeFile {
    pub name: String,
    /// The directory, relative to the project (shown dimmed).
    pub detail: String,
    pub path: PathBuf,
    pub icon: Option<(icons::IconId, Rgba)>,
}

/// What the empty screen shows: the project list (no project) or the project's files.
#[derive(Debug, Clone, Copy)]
pub enum HomeContent<'a> {
    Projects,
    /// The recent files (no filter) or those found by the filter.
    Project {
        name: &'a str,
        files: &'a [HomeFile],
        searching: bool,
    },
}

/// A list row (after filtering).
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Action {
        label: String,
        icon: &'static str,
        shortcut: String,
        action: PickerAction,
    },
    Header(String),
    /// The history's title: where a drop goes to the top, with the "New group" button.
    HistoryHeader(String),
    Group {
        id: GroupId,
        name: String,
        depth: usize,
        collapsed: bool,
        count: usize,
    },
    Project {
        project: RecentProject,
        current: bool,
        depth: usize,
        /// Its group in the tree (`None`: the current project, a filtered list).
        group: Option<GroupId>,
        /// Its group's path, shown while filtering (`Clients / Northwind`).
        place: Option<String>,
    },
    File(HomeFile),
    /// A past session (its windows below it when expanded).
    Session {
        id: u128,
        when: String,
        /// How many windows and files (`3 windows · 12 files`).
        detail: String,
        crashed: bool,
        expanded: bool,
    },
    /// A window of a past session (its files below it when expanded).
    SessionWindow {
        session: u128,
        index: usize,
        name: String,
        detail: String,
        project: bool,
        /// Its project's folder (what a click on the row opens).
        path: Option<PathBuf>,
        count: usize,
        expanded: bool,
    },
    /// A file a past session's window had open.
    SessionFile(HomeFile),
}

impl Item {
    fn action(&self) -> Option<PickerAction> {
        match self {
            Self::Action { action, .. } => Some(action.clone()),
            Self::Project { project, current: false, .. } => {
                Some(PickerAction::OpenRecent(project.path.clone()))
            }
            Self::Project { current: true, .. } => Some(PickerAction::Close),
            Self::Group { id, .. } => Some(PickerAction::ToggleGroup(*id)),
            Self::File(file) | Self::SessionFile(file) => {
                Some(PickerAction::OpenPath(file.path.clone()))
            }
            Self::Session { id, .. } => Some(PickerAction::ToggleSession(*id)),
            // Its project opened (the chevron shows its files); files opened alone: shown.
            Self::SessionWindow { path: Some(path), .. } => {
                Some(PickerAction::OpenRecent(path.clone()))
            }
            Self::SessionWindow { session, index, count, path: None, .. } => {
                (*count > 0).then_some(PickerAction::ToggleSessionWindow(*session, *index))
            }
            Self::Header(_) | Self::HistoryHeader(_) => None,
        }
    }

    /// It can be dragged into a group.
    fn draggable(&self) -> bool {
        matches!(self, Self::Group { .. } | Self::Project { current: false, .. })
    }
}

/// A project or a group being dragged: the row, where the mouse is, and where it would
/// go (`None`: nowhere).
#[derive(Debug, Clone, PartialEq)]
struct Drag {
    item: usize,
    at: Point,
    drop: Option<Drop>,
}

/// Where a drop goes (the move), and how that shows.
#[derive(Debug, Clone, PartialEq)]
struct Drop {
    action: PickerAction,
    mark: Mark,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mark {
    /// Into the group of row `usize` (outlined).
    Row(usize),
    /// The history's title (the top).
    Header,
    /// Between two rows: a line from `x` at `y`.
    Line { x: f32, y: f32 },
}

/// How far each level of the tree is indented (logical pixels): past its group's guide
/// line (under the group's chevron) with room for the branch to each row, which stops
/// short of the row, so the guides never touch a row's hover or selection.
const INDENT: f32 = 28.0;

/// The filter field's height (logical pixels).
const FIELD_H: f32 = 38.0;

/// How far the mouse moves before a press becomes a drag (logical pixels).
const DRAG_START: f32 = 5.0;

/// How close to the list's edge a drag scrolls it (logical pixels), and how fast at most
/// (logical pixels per second).
const EDGE: f32 = 44.0;
const EDGE_SPEED: f32 = 900.0;

#[derive(Debug, Default)]
pub struct ProjectsPicker {
    /// The filter (the app's search field: ×, ⌘A through the app's field with the keys).
    pub query: SearchField,
    /// A press in the filter: a drag selects in it.
    selecting: bool,
    /// The home screen's field has the keyboard (another field, e.g. the project panel's
    /// file search, may have it instead).
    pub focused: bool,
    modal: Modal,
    items: Vec<Item>,
    selected: usize,
    rows: Vec<(Rect, usize)>,
    hover: Option<usize>,
    /// The list's scroll (its content's height measured at the last paint bounds it).
    scroll: ui_components::ScrollState,
    list: Rect,
    scale: f32,
    /// The groups (to refuse a group dropped into its own subgroup).
    groups: Vec<Group>,
    /// The history's title (a drop target) and its "New group" button.
    history: Option<Rect>,
    new_group: Option<Rect>,
    new_group_hover: bool,
    /// A press on a row that may become a drag (its item, where).
    press: Option<(usize, Point)>,
    drag: Option<Drag>,
    /// The last step of the scrolling at an edge during a drag.
    edge_step: Option<Instant>,
    /// The past sessions expanded, their windows expanded.
    sessions_open: HashSet<u128>,
    session_windows_open: HashSet<(u128, usize)>,
    /// Each past session's "Reopen" button, and the one under the mouse.
    restore_buttons: Vec<(Rect, u128)>,
    restore_hover: Option<u128>,
}

fn shortcut(primary: bool, shift: bool, key: &str) -> String {
    let mac = cfg!(target_os = "macos");
    let mut out = String::new();
    if shift {
        out.push_str(if mac { "⇧" } else { "Shift+" });
    }
    if primary {
        out.push_str(if mac { "⌘" } else { "Ctrl+" });
    }
    out.push_str(key);
    out
}

impl ProjectsPicker {
    // ─── The modal ───

    pub fn is_open(&self) -> bool {
        self.modal.is_open()
    }

    pub fn open(&mut self) {
        self.query.set_text("");
        self.selected = 0;
        self.scroll.reset();
        self.modal.open();
    }

    pub fn close(&mut self) {
        self.modal.close();
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.modal.set_viewport(viewport, scale);
    }

    /// The modal appearing, or a drag held at the list's edge (it scrolls each frame).
    pub fn is_animating(&self) -> bool {
        self.modal.is_animating() || self.edge_speed() != 0.0
    }

    // ─── The content ───

    /// Rebuilds the list from the projects and the filter.
    /// The list for an open project: the actions and the files (recent or found).
    fn rebuild_project(&mut self, files: &[HomeFile], searching: bool) {
        let filter = ui_kit::ListFilter::new(self.query.text());
        let matches = |s: &str| filter.matches(s);
        let mut items = Vec::new();
        let actions = [
            (
                i18n::t("home.search_project"),
                "search",
                shortcut(true, true, "F"),
                PickerAction::SearchProject,
            ),
            (i18n::t("home.open_file"), "file", shortcut(true, false, "O"), PickerAction::OpenFile),
            (i18n::t("home.new_file"), "plus", shortcut(true, false, "N"), PickerAction::NewFile),
        ];
        for (label, icon, shortcut, action) in actions {
            if matches(&label) {
                items.push(Item::Action { label, icon, shortcut, action });
            }
        }
        if !files.is_empty() {
            let title = if searching { "home.files" } else { "home.recent_files" };
            items.push(Item::Header(i18n::t(title)));
            items.extend(files.iter().cloned().map(Item::File));
        }
        self.items = items;
        // With a filter, the first found file is selected (Enter opens it directly).
        let first_file = self.items.iter().position(|i| matches!(i, Item::File(_)));
        if searching
            && self.selected == 0
            && let Some(f) = first_file
        {
            self.selected = f;
        }
        let selectable = self.selectable();
        if !selectable.contains(&self.selected) {
            self.selected = selectable.first().copied().unwrap_or(0);
        }
    }

    fn rebuild(&mut self, current: Option<&RecentProject>, recent: &RecentList) {
        let filter = ui_kit::ListFilter::new(self.query.text());
        let matches = |s: &str| filter.matches(s);
        let mut items = Vec::new();
        let mut actions = vec![
            (
                i18n::t("home.open_project"),
                "folder-open",
                shortcut(true, true, "O"),
                PickerAction::OpenProject,
            ),
            (i18n::t("home.open_remote"), "server", String::new(), PickerAction::OpenRemote),
            (i18n::t("home.open_file"), "file", shortcut(true, false, "O"), PickerAction::OpenFile),
            (i18n::t("home.new_file"), "plus", shortcut(true, false, "N"), PickerAction::NewFile),
        ];
        if !workbench_model::presence::command(workbench_model::OPEN_REMOTE_PROJECT) {
            actions.retain(|(_, _, _, action)| !matches!(action, PickerAction::OpenRemote));
        }
        // A project open here (the top bar's list): its kind's settings (a remote one's
        // connection), and it can be closed from it.
        if let Some(c) = current
            && let Some(edit) = c.kind.as_ref().and_then(|k| k.edit.as_ref())
        {
            let action = PickerAction::EditProject(c.path.clone());
            actions.push((edit.label.clone(), edit.icon, String::new(), action));
        }
        if current.is_some() {
            let label = i18n::t("home.close_project");
            actions.push((label, "folder-x", String::new(), PickerAction::CloseProject));
        }
        for (label, icon, shortcut, action) in actions {
            if matches(&label) {
                items.push(Item::Action { label, icon, shortcut, action });
            }
        }
        if filter.is_empty() {
            self.push_sessions(&mut items, &recent.sessions);
        }
        let model = &recent.model;
        let place = |p: &RecentProject| {
            let names = model.path_of(model.group_of(&p.path));
            let names: Vec<String> = names.iter().map(|n| group_name(n)).collect();
            Some(names.join(" / "))
        };
        if let Some(c) = current.filter(|c| matches(&c.name) || matches(&c.display)) {
            items.push(Item::Header(i18n::t("home.current")));
            let place = if filter.is_empty() { None } else { place(c) };
            items.push(Item::Project {
                project: c.clone(),
                current: true,
                depth: 0,
                group: None,
                place,
            });
        }
        let others: HashMap<&Path, &RecentProject> = recent
            .projects
            .iter()
            .filter(|r| Some(&r.path) != current.map(|c| &c.path))
            .map(|r| (r.path.as_path(), r))
            .collect();
        if filter.is_empty() {
            // The groups' tree (the recent projects in their own).
            if !others.is_empty() || model.groups.len() > 1 {
                items.push(Item::HistoryHeader(i18n::t("home.all_projects")));
                push_tree(&mut items, model, &others, None, 0);
            }
        } else {
            let found: Vec<Item> = recent
                .projects
                .iter()
                .filter(|p| others.contains_key(p.path.as_path()))
                .filter_map(|p| {
                    let place = place(p);
                    let hit = matches(&p.name)
                        || matches(&p.display)
                        || place.as_deref().is_some_and(matches);
                    hit.then(|| Item::Project {
                        project: p.clone(),
                        current: false,
                        depth: 0,
                        group: None,
                        place,
                    })
                })
                .collect();
            if !found.is_empty() {
                items.push(Item::Header(i18n::t("home.recent")));
                items.extend(found);
            }
        }
        self.groups.clone_from(&model.groups);
        self.items = items;
        let selectable = self.selectable();
        if !selectable.contains(&self.selected) {
            self.selected = selectable.first().copied().unwrap_or(0);
        }
    }

    fn selectable(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, i)| !matches!(i, Item::Header(_) | Item::HistoryHeader(_)))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn type_text(&mut self, typed: &str) {
        Control::text(&mut self.query, typed);
        self.selected = 0;
        self.scroll.reset();
    }

    pub fn backspace(&mut self) {
        Control::key(&mut self.query, ui_components::ViewerKey::Backspace, Default::default());
        self.selected = 0;
    }

    pub fn select_all(&mut self) {
        self.query.input.select_all();
    }

    pub fn has_query(&self) -> bool {
        !self.query.text().is_empty()
    }

    pub fn clear_query(&mut self) {
        self.query.set_text("");
        self.selected = 0;
    }

    /// ↑ / ↓ through the selectable items.
    pub fn step(&mut self, delta: isize) {
        let selectable = self.selectable();
        let Some(pos) = selectable.iter().position(|&i| i == self.selected) else {
            self.selected = selectable.first().copied().unwrap_or(0);
            return;
        };
        let next = ui_kit::step_index(pos, delta, selectable.len());
        self.selected = selectable[next];
        // The selected row stays visible.
        if let Some((r, _)) = self.rows.iter().find(|(_, i)| *i == self.selected) {
            self.scroll.keep_visible(*r, self.list);
        }
    }

    /// Enter: the selected item.
    pub fn confirm(&self) -> Option<PickerAction> {
        self.items.get(self.selected).and_then(Item::action)
    }

    /// The selected group and whether it's collapsed (← / → collapse and expand it).
    pub fn selected_group(&self) -> Option<(GroupId, bool)> {
        match self.items.get(self.selected)? {
            Item::Group { id, collapsed, .. } => Some((*id, *collapsed)),
            _ => None,
        }
    }

    /// Click: a row, the filter (its × empties it, else the caret put, a drag selecting),
    /// or (in the modal) outside it.
    pub fn hit(&mut self, p: Point) -> Option<PickerAction> {
        self.press = None;
        self.drag = None;
        self.selecting = false;
        if self.query.hit(p) {
            match Control::press(&mut self.query, p, 1) {
                Press::Held => self.selecting = true,
                _ if self.query.take_cleared() => self.selected = 0,
                _ => {}
            }
            return None;
        }
        if self.new_group.is_some_and(|r| r.contains(p) && self.list.contains(p)) {
            return Some(PickerAction::NewGroup);
        }
        if let Some(id) = self.restore_at(p) {
            return Some(PickerAction::RestoreSession(id));
        }
        if let Some(&(row, i)) =
            self.rows.iter().find(|(r, _)| r.contains(p) && self.list.contains(p))
        {
            // A project or a group: a click or a drag, known on release.
            if self.items.get(i).is_some_and(Item::draggable) {
                self.selected = i;
                self.press = Some((i, p));
                return None;
            }
            // A past session's window: its chevron shows or hides its files, the rest of the
            // row opens its project.
            if let Some(Item::SessionWindow { session, index, count, .. }) = self.items.get(i)
                && *count > 0
                && p.x < row.min_x() + (sessions::CHEVRON_END * self.scale).round()
            {
                return Some(PickerAction::ToggleSessionWindow(*session, *index));
            }
            return self.items.get(i).and_then(Item::action);
        }
        if self.modal.is_open() && self.modal.hit(p) == Some(ModalHit::Close) {
            return Some(PickerAction::Close);
        }
        None
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover =
            self.rows.iter().find(|(r, _)| r.contains(p) && self.list.contains(p)).map(|(_, i)| *i);
        let clear = Control::hover(&mut self.query, Some(p));
        let new_group = self.new_group.is_some_and(|r| r.contains(p) && self.list.contains(p));
        let frame = self.modal.is_open() && self.modal.hover(p);
        let restore = self.restore_at(p);
        let changed = hover != self.hover
            || clear
            || new_group != self.new_group_hover
            || restore != self.restore_hover;
        self.restore_hover = restore;
        self.hover = hover;
        self.new_group_hover = new_group;
        changed || frame
    }

    pub fn over_clickable(&self, p: Point) -> bool {
        self.query.cursor(p) == Some(ui_components::ViewerCursor::Pointer)
            || self.new_group.is_some_and(|r| r.contains(p) && self.list.contains(p))
            || self.restore_at(p).is_some()
            || self.rows.iter().any(|(r, _)| r.contains(p) && self.list.contains(p))
    }

    /// The wheel over the list: never past its end (momentum scrolling doesn't draw a frame
    /// past it and then snap back: jitter).
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        self.scroll.scroll_by(dy)
    }

    /// The filter field, at the top of `area` (with × when it has text); returns it.
    fn paint_filter(
        &mut self,
        area: Rect,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) -> Rect {
        let field = Rect::new(area.min_x(), area.min_y(), area.size.width, ctx.px(FIELD_H));
        let layer = scene.push_layer(None);
        let files_mode = !self.items.iter().any(|i| {
            matches!(i, Item::Project { .. } | Item::Group { .. } | Item::HistoryHeader(_))
        });
        let placeholder = i18n::t(if files_mode { "home.filter_files" } else { "home.filter" });
        // Its caret only while it has the keys (the project panel's search may have them).
        self.query.paint(layer, text, icons, ctx, (field, self.focused), &placeholder);
        field
    }
}

/// A group's name as shown: the recent group's is the UI's.
fn group_name(name: &str) -> String {
    if name.is_empty() { i18n::t("home.recent_group") } else { name.to_owned() }
}

/// The groups in `parent`, each followed (unless collapsed) by its subgroups and its
/// projects (`shown`: those listed), at `depth`.
fn push_tree(
    items: &mut Vec<Item>,
    model: &RecentProjects,
    shown: &HashMap<&Path, &RecentProject>,
    parent: Option<GroupId>,
    depth: usize,
) {
    // Bounded: a repaired list has no cycles, but a deep one stops somewhere.
    if depth > 32 {
        return;
    }
    for group in model.children(parent) {
        let count = shown.keys().filter(|p| model.is_within(model.group_of(p), group.id)).count();
        items.push(Item::Group {
            id: group.id,
            name: group_name(&group.name),
            depth,
            collapsed: group.collapsed,
            count,
        });
        if group.collapsed {
            continue;
        }
        push_tree(items, model, shown, Some(group.id), depth + 1);
        for path in model.projects_in(group.id) {
            if let Some(project) = shown.get(path.as_path()) {
                items.push(Item::Project {
                    project: (*project).clone(),
                    current: false,
                    depth: depth + 1,
                    group: Some(group.id),
                    place: None,
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/projects_picker.rs"]
mod tests;
