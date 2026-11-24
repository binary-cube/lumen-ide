//! The window's panel: the contributors' tree on the left (filtered as typed, its keys
//! those of the "Database" window), and on the right the chosen node's tabs, the active
//! one's [`Viewer`] under them, a splitter between the two (its place kept in the
//! project's session).
//!
//! It asks the contributors for the project's services only once it's drawn (made when the
//! window is first shown) and drops them with the project; a tab's view is made when it's
//! first shown and let go when its node isn't the one shown any more (its contributor keeps
//! what it shows: the log, the rows).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};

use kernel::{EventBus, Point, ProjectId, Rect};
use ui_components::control::Control;
use ui_components::search_field::SearchField;
use ui_components::{
    EdgeScrollbar, Orientation, ProjectServices, ServiceId, ServiceProvider, ServiceTab,
    ServicesContext, Splitter, SplitterStyle, ToolPanel, ToolPanelAction, Viewer, ViewerCursor,
    ViewerKey,
};
use ui_kit::ListFilter;

use crate::tree::{self, Key, Row};

mod tabs;
mod tool_panel;

/// The edit command choosing a whole field (it changes nothing filtered).
const SELECT_ALL: &str = "select_all";

/// The window's id (its button, its menu, its commands' view).
pub const TOOL: &str = workbench_model::SERVICES_TOOL;

/// Logical pixels: the tree's width (by default, at least), a row, the indentation, the
/// tree's inset, the filter field, where the rows start, the tabs' bar, the splitter's strip.
pub const TREE_WIDTH: f32 = 300.0;
/// A view's link to another tab of its node: `service-tab:` and the tab's id in hex
/// (`ServiceId::link`).
pub const TAB_LINK: &str = ui_components::services::TAB_LINK;
pub const TREE_MIN: f32 = 180.0;
pub const ROW_HEIGHT: f32 = 28.0;
pub const INDENT: f32 = 16.0;
pub const PADDING: f32 = 12.0;
pub const FIELD_TOP: f32 = 8.0;
pub const FIELD_HEIGHT: f32 = 30.0;
pub const LIST_TOP: f32 = FIELD_TOP + FIELD_HEIGHT + 6.0;
pub const TABS_HEIGHT: f32 = 36.0;
pub const GAP: f32 = 8.0;

/// Wakes the app from a background thread.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// A contributor's services in the project.
struct Contributor {
    id: &'static str,
    icon: &'static str,
    title: String,
    services: Box<dyn ProjectServices>,
}

/// What a tab's part is under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabHit {
    Tab(usize),
    Close(usize),
}

/// Where the mouse is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Hover {
    Field,
    Clear,
    Row(usize),
    Tab(TabHit),
}

/// Shows a contributor's node (its tab chosen): given to the window
/// ([`ToolPanel::set_data`], `WorkbenchHost::tell_panel`), e.g. by a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reveal {
    pub provider: &'static str,
    pub node: ServiceId,
    pub tab: Option<ServiceId>,
}

/// A command for the view of the tab shown ([`ToolPanel::set_data`]), as from its own menu
/// (a snapshot choosing a profile's finding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabCommand(pub String);

/// What the menu shown is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuTarget {
    Node(Key),
    Tab(ServiceId),
    View,
}

/// The window's panel (see the module's doc).
pub struct ServicesPanel {
    /// The contributors (`None`: the registered ones, read when it opens).
    providers: Option<Vec<Arc<dyn ServiceProvider>>>,
    /// The project it was given (`None`: not yet); its events.
    project: Option<Option<ProjectId>>,
    events: EventBus,
    contributors: Vec<Contributor>,
    /// The contributors were asked for this project.
    opened: bool,
    pub(crate) rows: Vec<Row>,
    /// The contributors' generations the rows were made from (with the filter, the folds).
    rows_of: Option<Vec<u64>>,
    collapsed: HashSet<Key>,
    /// The filter (the app's search field: ×, ⌘A, a drag selecting), typed in while
    /// `typing` (its caret only then, never while a view has the keys).
    pub(crate) filter: SearchField,
    pub(crate) typing: bool,
    /// A press in the filter holds the pointer: the moves select.
    selecting: bool,
    pub(crate) selected: Option<Key>,
    pub(crate) hover: Option<Hover>,
    pub(crate) scroll: f32,
    pub(crate) bar: EdgeScrollbar,
    /// The selected node's tabs, and what they were read for (the node, its generation).
    pub(crate) tabs: Vec<ServiceTab>,
    tabs_of: Option<(Key, u64)>,
    /// Each node's active tab.
    active_tab: HashMap<Key, ServiceId>,
    /// The views of the shown node's tabs, made when first shown.
    views: HashMap<ServiceId, Box<dyn Viewer>>,
    /// How text is shown, as the app told it: given to each view, those made later too.
    text_options: ui_components::TextOptions,
    /// The project's dragged sizes, given to each view made (its splitters' places).
    sizes: Option<workbench_model::LayoutSizes>,
    /// The keys go to the view (clicked into), not the tree.
    pub(crate) in_view: bool,
    dragging_view: bool,
    pub(crate) tree_width: f32,
    pub(crate) splitter: Splitter,
    action: Option<ToolPanelAction>,
    menu_target: Option<MenuTarget>,
    /// Where the mouse last was (the wheel goes to what's under it).
    last_pointer: Option<Point>,
    waker: Arc<Mutex<Option<Waker>>>,
    // Where it was last drawn.
    pub(crate) bounds: Rect,
    pub(crate) tree_area: Rect,
    pub(crate) field: Rect,
    pub(crate) list: Rect,
    pub(crate) tabs_bar: Rect,
    pub(crate) view_area: Rect,
    pub(crate) tab_hits: Vec<(Rect, TabHit)>,
    pub(crate) scale: f32,
}

impl std::fmt::Debug for ServicesPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServicesPanel")
            .field("project", &self.project)
            .field("contributors", &self.contributors.len())
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}

impl Default for ServicesPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ServicesPanel {
    /// The panel of the registered contributors.
    pub fn new() -> Self {
        Self {
            providers: None,
            project: None,
            events: EventBus::new(),
            contributors: Vec::new(),
            opened: false,
            rows: Vec::new(),
            rows_of: None,
            collapsed: HashSet::new(),
            filter: SearchField::new(),
            typing: false,
            selecting: false,
            selected: None,
            hover: None,
            scroll: 0.0,
            bar: EdgeScrollbar::default(),
            tabs: Vec::new(),
            tabs_of: None,
            active_tab: HashMap::new(),
            views: HashMap::new(),
            text_options: ui_components::TextOptions::default(),
            sizes: None,
            in_view: false,
            dragging_view: false,
            tree_width: TREE_WIDTH,
            splitter: Splitter::new(SplitterStyle::default(), Orientation::Vertical),
            action: None,
            menu_target: None,
            last_pointer: None,
            waker: Arc::new(Mutex::new(None)),
            bounds: Rect::default(),
            tree_area: Rect::default(),
            field: Rect::default(),
            list: Rect::default(),
            tabs_bar: Rect::default(),
            view_area: Rect::default(),
            tab_hits: Vec::new(),
            scale: 1.0,
        }
    }

    /// The panel of `providers` only (tests).
    pub fn with_providers(providers: Vec<Arc<dyn ServiceProvider>>) -> Self {
        let mut panel = Self::new();
        panel.providers = Some(providers);
        panel
    }

    /// The project shown: another one drops what the previous one's contributors gave.
    pub fn set_project(&mut self, project: Option<ProjectId>, events: EventBus) {
        if self.project == Some(project) {
            return;
        }
        if let Some(Some(previous)) = self.project {
            ui_components::services::set_shown(previous, false);
        }
        self.project = Some(project);
        self.events = events;
        self.contributors.clear();
        self.views.clear();
        self.tabs.clear();
        self.tabs_of = None;
        self.active_tab.clear();
        self.collapsed.clear();
        self.selected = None;
        self.rows.clear();
        self.rows_of = None;
        self.scroll = 0.0;
        self.opened = false;
    }

    /// Asks the contributors for the project's services, once (it's shown).
    pub fn open(&mut self) {
        if self.opened || self.project.is_none() {
            return;
        }
        self.opened = true;
        let slot = Arc::clone(&self.waker);
        let wake: Waker = Arc::new(move || {
            let waker = slot.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(waker) = waker {
                waker();
            }
        });
        let ctx =
            ServicesContext { project: self.project.flatten(), events: self.events.clone(), wake };
        let providers = self.providers.clone().unwrap_or_else(ui_components::services::providers);
        for provider in providers {
            if let Some(services) = provider.open(&ctx) {
                self.contributors.push(Contributor {
                    id: provider.id(),
                    icon: provider.icon(),
                    title: provider.title(),
                    services,
                });
            }
        }
        self.rows_of = None;
    }

    fn contributor(&mut self, id: &str) -> Option<&mut Contributor> {
        self.contributors.iter_mut().find(|c| c.id == id)
    }

    /// The rows made again if a contributor changed (or the filter, the folds); the tabs
    /// of the node shown read again if they changed.
    pub fn refresh(&mut self) {
        self.open();
        let generations: Vec<u64> =
            self.contributors.iter().map(|c| c.services.generation()).collect();
        if self.rows_of.as_ref() != Some(&generations) {
            let filter = ListFilter::new(self.filter.text());
            let groups: Vec<tree::Group<'_>> = self
                .contributors
                .iter()
                .map(|c| tree::Group {
                    provider: c.id,
                    icon: c.icon,
                    title: &c.title,
                    nodes: c.services.nodes(),
                })
                .collect();
            self.rows = tree::rows(&groups, &self.collapsed, &filter);
            self.rows_of = Some(generations);
            let gone = self.selected.is_some_and(|k| !self.exists(&k));
            if gone {
                self.selected = None;
            }
            // Nothing chosen yet: the first node (its tabs shown at once).
            if self.selected.is_none() {
                self.selected = self.rows.iter().find(|r| r.key.node().is_some()).map(|r| r.key);
            }
        }
        self.refresh_tabs();
    }

    /// The node still exists (shown or folded away).
    fn exists(&self, key: &Key) -> bool {
        match key {
            Key::Group(p) => self.contributors.iter().any(|c| c.id == *p),
            Key::Node(p, id) => self
                .contributors
                .iter()
                .find(|c| c.id == *p)
                .is_some_and(|c| c.services.nodes().iter().any(|n| n.id == *id)),
        }
    }

    pub fn rows(&mut self) -> &[Row] {
        self.refresh();
        &self.rows
    }

    pub fn selected(&self) -> Option<Key> {
        self.selected
    }

    /// Selects `key` (its parents opened) and, if given, its tab `tab`.
    pub fn select(&mut self, key: Key, tab: Option<ServiceId>) {
        self.refresh();
        // Its parents opened.
        if let Key::Node(p, id) = key {
            self.collapsed.remove(&Key::Group(p));
            let parents: Vec<ServiceId> = self
                .contributors
                .iter()
                .find(|c| c.id == p)
                .map(|c| {
                    let nodes = c.services.nodes();
                    let mut out = Vec::new();
                    let mut at = nodes.iter().find(|n| n.id == id).and_then(|n| n.parent);
                    while let Some(parent) = at.filter(|p| !out.contains(p)) {
                        out.push(parent);
                        at = nodes.iter().find(|n| n.id == parent).and_then(|n| n.parent);
                    }
                    out
                })
                .unwrap_or_default();
            for parent in parents {
                self.collapsed.remove(&Key::Node(p, parent));
            }
            self.rows_of = None;
        }
        self.selected = Some(key);
        if let Some(tab) = tab {
            self.active_tab.insert(key, tab);
        }
        self.tabs_of = None;
        self.refresh();
        if let Some(i) = self.rows.iter().position(|r| r.key == key) {
            self.select_index(i);
        }
    }

    /// Opens / folds `key`.
    pub fn toggle(&mut self, key: Key) {
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        self.rows_of = None;
    }

    pub fn collapse_all(&mut self) {
        self.refresh();
        for row in &self.rows {
            if row.expandable {
                self.collapsed.insert(row.key);
            }
        }
        self.rows_of = None;
        self.scroll = 0.0;
    }

    pub fn expand_all(&mut self) {
        self.collapsed.clear();
        self.rows_of = None;
    }

    /// Activates the selected node (its contributor's double click), or opens / folds it.
    fn activate(&mut self, key: Key) {
        let done = match key {
            Key::Node(p, node) => self.contributor(p).is_some_and(|c| c.services.activate(node)),
            Key::Group(_) => false,
        };
        if !done && self.rows.iter().any(|r| r.key == key && r.expandable) {
            self.toggle(key);
        }
    }

    /// The selection moved to row `i` (kept in view).
    pub(crate) fn select_index(&mut self, i: usize) {
        let Some(row) = self.rows.get(i) else { return };
        if self.selected != Some(row.key) {
            self.selected = Some(row.key);
            self.tabs_of = None;
            self.refresh_tabs();
        }
        let h = (ROW_HEIGHT * self.scale).round();
        let top = i as f32 * h;
        let view = self.list.size.height - (6.0 * self.scale).round();
        if top < self.scroll {
            self.scroll = top;
        } else if view > 0.0 && top + h > self.scroll + view {
            self.scroll = top + h - view;
        }
    }

    fn selected_index(&self) -> Option<usize> {
        let key = self.selected?;
        self.rows.iter().position(|r| r.key == key)
    }

    /// A key on the rows.
    fn row_key(&mut self, key: ViewerKey) -> bool {
        self.refresh();
        let len = self.rows.len();
        if len == 0 {
            return false;
        }
        let at = self.selected_index();
        let page = ((self.list.size.height / (ROW_HEIGHT * self.scale)).floor() as usize).max(1);
        match key {
            ViewerKey::Down => self.select_index(at.map_or(0, |i| (i + 1).min(len - 1))),
            ViewerKey::Up => self.select_index(at.map_or(0, |i| i.saturating_sub(1))),
            ViewerKey::Home => self.select_index(0),
            ViewerKey::End => self.select_index(len - 1),
            ViewerKey::PageDown => self.select_index(at.map_or(0, |i| (i + page).min(len - 1))),
            ViewerKey::PageUp => self.select_index(at.map_or(0, |i| i.saturating_sub(page))),
            ViewerKey::Right => {
                let Some(i) = at else { return false };
                let row = self.rows[i].clone();
                if row.expandable && !row.expanded {
                    self.toggle(row.key);
                } else if row.expanded && i + 1 < len {
                    self.select_index(i + 1);
                }
            }
            ViewerKey::Left => {
                let Some(i) = at else { return false };
                let row = self.rows[i].clone();
                if row.expanded && self.filter.text().trim().is_empty() {
                    self.toggle(row.key);
                } else if let Some(parent) = (0..i).rev().find(|&j| self.rows[j].depth < row.depth)
                {
                    self.select_index(parent);
                }
            }
            ViewerKey::Enter => {
                let Some(i) = at else { return false };
                let key = self.rows[i].key;
                self.activate(key);
            }
            ViewerKey::Escape if !self.filter.text().is_empty() => {
                self.filter.set_text("");
                self.filtered();
            }
            _ => return false,
        }
        true
    }

    /// The filter changed: the rows made again from the top.
    fn filtered(&mut self) {
        self.rows_of = None;
        self.scroll = 0.0;
    }

    /// Types `text` into the filter (tests, the rows' typing).
    pub fn type_filter(&mut self, text: &str) {
        self.typing = true;
        self.text_input(text);
    }

    pub(crate) fn row_at(&self, p: Point) -> Option<usize> {
        if !self.list.contains(p) {
            return None;
        }
        let h = (ROW_HEIGHT * self.scale).round();
        let list = ui_components::TreeList::new(h, self.rows.len());
        list.row_at(p.y - self.list.min_y() + self.scroll - (6.0 * self.scale).round())
    }

    fn tab_hit(&self, p: Point) -> Option<TabHit> {
        // × is drawn over its tab (registered after it).
        self.tab_hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    pub(crate) fn hit(&self, p: Point) -> Option<Hover> {
        if self.field.contains(p) {
            // Its × (with text) is the field's own.
            let clear = Control::cursor(&self.filter, p) == Some(ViewerCursor::Pointer);
            return Some(if clear { Hover::Clear } else { Hover::Field });
        }
        if let Some(hit) = self.tab_hit(p) {
            return Some(Hover::Tab(hit));
        }
        self.row_at(p).map(Hover::Row)
    }

    fn on_chevron(&self, i: usize, p: Point) -> bool {
        let Some(row) = self.rows.get(i) else { return false };
        let x =
            self.list.min_x() + ((PADDING + f32::from(row.depth) * INDENT) * self.scale).round();
        row.expandable && p.x >= x - 4.0 * self.scale && p.x <= x + 16.0 * self.scale
    }

    /// The tree's width in physical pixels within `width`.
    pub(crate) fn tree_px(&self, width: f32) -> f32 {
        let s = self.scale;
        let max = (width * 0.6).max(TREE_MIN * s);
        (self.tree_width * s).round().clamp((TREE_MIN * s).min(max), max)
    }
}

impl Drop for ServicesPanel {
    fn drop(&mut self) {
        if let Some(Some(project)) = self.project {
            ui_components::services::set_shown(project, false);
        }
    }
}

impl ServicesPanel {
    /// One of the tree's own commands (its header's buttons, its button's menu).
    pub fn tree_command(&mut self, id: &str) -> bool {
        match id {
            "collapse" => self.collapse_all(),
            "expand" => self.expand_all(),
            _ => return false,
        }
        true
    }

    /// The mouse was last over the view (where the wheel goes).
    fn hover_in_view(&self) -> bool {
        self.last_pointer.is_some_and(|p| self.view_area.contains(p))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/panel.rs"]
mod tests;
