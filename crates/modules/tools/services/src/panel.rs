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
use ui_components::control::{Control, Press};
use ui_components::search_field::SearchField;
use ui_components::{
    EdgeScrollbar, KeyReach, Orientation, PanelButton, PanelContext, PanelMemento, ProjectServices,
    ServiceId, ServiceProvider, ServiceState, ServiceTab, ServicesContext, Splitter, SplitterStyle,
    ToolPanel, ToolPanelAction, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerPaint,
};
use ui_kit::ListFilter;
use workbench_model::ProjectView;

use crate::tree::{self, Key, Row};

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

    fn refresh_tabs(&mut self) {
        let key = self.selected.filter(|k| k.node().is_some());
        let generation = key.and_then(|k| {
            self.contributors.iter().find(|c| c.id == k.provider()).map(|c| c.services.generation())
        });
        let of = key.zip(generation);
        if self.tabs_of == of && of.is_some() {
            return;
        }
        if of.is_none() && self.tabs_of.is_none() && self.tabs.is_empty() {
            return;
        }
        self.tabs_of = of;
        self.tabs = match key {
            Some(Key::Node(p, node)) => {
                self.contributor(p).map(|c| c.services.tabs(node)).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        // The views of tabs not shown any more let go.
        let shown: HashSet<ServiceId> = self.tabs.iter().map(|t| t.id).collect();
        self.views.retain(|id, _| shown.contains(id));
        if let Some(key) = key {
            let active = self.active_tab.get(&key).copied().filter(|id| shown.contains(id));
            // A node that is one of its tabs (a result under its session): that tab shown.
            let itself = key.node().filter(|id| shown.contains(id));
            match active.or(itself).or_else(|| self.tabs.first().map(|t| t.id)) {
                Some(id) => {
                    self.active_tab.insert(key, id);
                }
                None => {
                    self.active_tab.remove(&key);
                }
            }
        }
        if self.tabs.is_empty() {
            self.in_view = false;
        }
    }

    /// The shown node's active tab.
    pub fn active_tab(&self) -> Option<ServiceId> {
        self.selected.and_then(|k| self.active_tab.get(&k).copied())
    }

    /// The active tab's view, made if it isn't.
    pub(crate) fn active_view(&mut self) -> Option<&mut Box<dyn Viewer>> {
        let id = self.active_tab()?;
        if !self.views.contains_key(&id) {
            let provider = self.selected?.provider();
            let mut view = self.contributor(provider)?.services.open_tab(id)?;
            if let Some(sizes) = &self.sizes {
                view.set_data(sizes);
            }
            view.set_text_options(&self.text_options);
            self.views.insert(id, view);
        }
        self.views.get_mut(&id)
    }

    /// How many tabs' views are made (tests: let go when not shown).
    pub fn views_made(&self) -> usize {
        self.views.len()
    }

    pub fn rows(&mut self) -> &[Row] {
        self.refresh();
        &self.rows
    }

    pub fn tabs(&mut self) -> &[ServiceTab] {
        self.refresh();
        &self.tabs
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

    /// Shows tab `i` of the node shown.
    pub fn show_tab(&mut self, i: usize) {
        let (Some(key), Some(tab)) = (self.selected, self.tabs.get(i)) else { return };
        self.active_tab.insert(key, tab.id);
    }

    /// Closes tab `i` (if it can be): its contributor told, its view let go.
    pub fn close_tab(&mut self, i: usize) {
        let Some(tab) = self.tabs.get(i).cloned().filter(|t| t.closable) else { return };
        let Some(provider) = self.selected.map(|k| k.provider()) else { return };
        if let Some(c) = self.contributor(provider) {
            c.services.close_tab(tab.id);
        }
        self.views.remove(&tab.id);
        self.tabs_of = None;
        self.refresh_tabs();
    }

    /// Pins / unpins tab `i`.
    pub fn toggle_pin(&mut self, i: usize) {
        let Some(tab) = self.tabs.get(i).cloned() else { return };
        let Some(provider) = self.selected.map(|k| k.provider()) else { return };
        if let Some(c) = self.contributor(provider) {
            c.services.pin_tab(tab.id, !tab.pinned);
        }
        self.tabs_of = None;
        self.refresh_tabs();
    }

    /// Closes the tabs but `i` (the pinned ones and those that can't close kept).
    pub fn close_other_tabs(&mut self, i: usize) {
        let Some(keep) = self.tabs.get(i).map(|t| t.id) else { return };
        let others: Vec<ServiceId> = self
            .tabs
            .iter()
            .filter(|t| t.id != keep && t.closable && !t.pinned)
            .map(|t| t.id)
            .collect();
        for id in others {
            if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                self.close_tab(i);
            }
        }
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

    /// What the view asked for: a close closes its tab; the rest goes to the app.
    fn view_action(&mut self, action: Option<ViewerAction>) -> Option<ToolPanelAction> {
        match action? {
            ViewerAction::Close => {
                let active = self.active_tab()?;
                let i = self.tabs.iter().position(|t| t.id == active)?;
                self.close_tab(i);
                None
            }
            // A link to another tab of the node shown (the Output's line naming a profile).
            ViewerAction::OpenLink(link) if link.starts_with(TAB_LINK) => {
                let id = u128::from_str_radix(&link[TAB_LINK.len()..], 16).ok()?;
                let i = self.tabs.iter().position(|t| t.id.0 == id)?;
                self.show_tab(i);
                None
            }
            other => Some(ToolPanelAction::View(other)),
        }
    }

    /// What the shown view answered a press, a key or a command with: a menu it asked for
    /// under a button of its own goes to the app, shown under it as this panel's menu (the
    /// row chosen comes back to the view, [`ToolPanel::menu_command`]), nothing else done;
    /// else its action ([`Self::view_action`]).
    fn view_answer(&mut self, action: Option<ViewerAction>) -> Option<ToolPanelAction> {
        if let Some((anchor, entries)) = self.active_view().and_then(|v| v.take_menu()) {
            self.menu_target = Some(MenuTarget::View);
            return Some(ToolPanelAction::MenuUnder { anchor, entries });
        }
        self.view_action(action)
    }

    /// What a view's action does here (tests).
    #[cfg(test)]
    pub(crate) fn view_action_for_tests(
        &mut self,
        action: Option<ViewerAction>,
    ) -> Option<ToolPanelAction> {
        self.view_action(action)
    }

    /// The tree's width in physical pixels within `width`.
    pub(crate) fn tree_px(&self, width: f32) -> f32 {
        let s = self.scale;
        let max = (width * 0.6).max(TREE_MIN * s);
        (self.tree_width * s).round().clamp((TREE_MIN * s).min(max), max)
    }

    /// The tab's menu rows.
    fn tab_menu(&self, i: usize) -> Vec<ViewMenuEntry> {
        let Some(tab) = self.tabs.get(i) else { return Vec::new() };
        let t = i18n::t;
        let mut rows = vec![
            ViewMenuEntry::item(
                "tab.pin",
                if tab.pinned { t("services.unpin_tab") } else { t("services.pin_tab") },
            )
            .with_icon("pin"),
        ];
        if tab.closable {
            rows.push(ViewMenuEntry::Separator);
            rows.push(ViewMenuEntry::item("tab.close", t("services.close_tab")).with_icon("x"));
        }
        if self.tabs.len() > 1 {
            rows.push(ViewMenuEntry::item("tab.close_others", t("services.close_other_tabs")));
        }
        rows
    }
}

impl Drop for ServicesPanel {
    fn drop(&mut self) {
        if let Some(Some(project)) = self.project {
            ui_components::services::set_shown(project, false);
        }
    }
}

impl ToolPanel for ServicesPanel {
    /// How text is shown: handed to its views (the ones made later get it when made).
    fn set_text_options(&mut self, options: &ui_components::TextOptions) {
        self.text_options = *options;
        for view in self.views.values_mut() {
            view.set_text_options(options);
        }
    }

    fn update(&mut self, _ctx: &PanelContext<'_>) {}

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(view) = data.downcast_ref::<ProjectView>() {
            self.set_project(view.project, view.events.clone());
        } else if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            for view in self.views.values_mut() {
                view.set_data(sizes);
            }
            self.sizes = Some(sizes.clone());
        } else if let Some(reveal) = data.downcast_ref::<Reveal>() {
            self.select(Key::Node(reveal.provider, reveal.node), reveal.tab);
        } else if let Some(crate::PanelCommand(id)) = data.downcast_ref() {
            self.tree_command(id);
        } else if let Some(TabCommand(id)) = data.downcast_ref() {
            self.in_view = true;
            if self.active_view().is_some_and(|v| v.command(id)) {
                let action = self.active_view().and_then(|v| v.take_action());
                self.action = self.view_answer(action);
            }
        } else if let Some(PanelMemento(text)) = data.downcast_ref()
            && let Some(width) = text.strip_prefix("tree=").and_then(|w| w.parse::<f32>().ok())
            && width.is_finite()
        {
            self.tree_width = width.max(TREE_MIN);
        }
    }

    fn memento(&self) -> Option<String> {
        Some(format!("tree={}", self.tree_width.round()))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let Some(waker) = &ctx.waker {
            let mut slot = self.waker.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.is_none() {
                *slot = Some(Arc::clone(waker));
            }
        }
        // Drawn: shown (its contributors know their work is seen).
        if let Some(Some(project)) = self.project {
            ui_components::services::set_shown(project, true);
        }
        self.refresh();
        self.paint_panel(ctx);
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        let mut reveal = None;
        for c in &mut self.contributors {
            changed |= c.services.poll();
            if let Some((node, tab)) = c.services.take_reveal() {
                reveal = Some((Key::Node(c.id, node), tab));
            }
        }
        if let Some((key, tab)) = reveal {
            self.select(key, tab);
            changed = true;
        }
        if let Some(view) = self.active_tab().and_then(|id| self.views.get_mut(&id)) {
            changed |= view.poll();
        }
        if changed {
            self.refresh();
        }
        changed
    }

    fn parked(&mut self) {
        if let Some(Some(project)) = self.project {
            ui_components::services::set_shown(project, false);
        }
        self.dragging_view = false;
        self.splitter.end_drag();
    }

    fn purge(&mut self) {
        // The views made again when shown; the contributors let go of what they rebuild.
        self.views.clear();
        for c in &mut self.contributors {
            c.services.purge();
        }
    }

    fn is_animating(&self) -> bool {
        let view =
            self.active_tab().and_then(|id| self.views.get(&id)).is_some_and(|v| v.is_animating());
        view || self.splitter.is_animating()
    }

    /// A service running (a statement, a request) or its view loading: the loader's pace.
    fn is_loading(&self) -> bool {
        let running = self.rows.iter().any(|r| r.state == ServiceState::Running);
        let view =
            self.active_tab().and_then(|id| self.views.get(&id)).is_some_and(|v| v.is_loading());
        running || view
    }

    fn scroll(&mut self, dy: f32) -> bool {
        self.scroll_by(0.0, dy)
    }

    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        // Over the view: its own; over the tree: the rows.
        if self.view_area.size.width > 0.0 && self.hover_in_view() {
            return self.active_view().is_some_and(|v| v.scroll(dx, dy));
        }
        let before = self.scroll;
        let h = (ROW_HEIGHT * self.scale).round();
        let content = self.rows.len() as f32 * h + (12.0 * self.scale).round();
        let max = (content - self.list.size.height).max(0.0);
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ToolPanelAction> {
        self.last_pointer = Some(p);
        if self.splitter.begin_drag(p) {
            if clicks == 2 {
                self.splitter.end_drag();
                self.tree_width = TREE_WIDTH;
            }
            return None;
        }
        if self.view_area.contains(p) {
            self.in_view = true;
            self.typing = false;
            self.dragging_view = true;
            let action = self.active_view().and_then(|v| v.press(p, clicks));
            let action = action.or_else(|| self.active_view().and_then(|v| v.take_action()));
            return self.view_answer(action);
        }
        self.in_view = false;
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        match self.hit(p) {
            // The field's press: × empties it (the keys stay in it), else the caret put and
            // a selection begun.
            Some(Hover::Clear | Hover::Field) => {
                self.typing = true;
                self.selecting = Control::press(&mut self.filter, p, clicks) == Press::Held;
                if self.filter.take_cleared() {
                    self.filtered();
                }
            }
            Some(Hover::Tab(TabHit::Close(i))) => self.close_tab(i),
            Some(Hover::Tab(TabHit::Tab(i))) => self.show_tab(i),
            Some(Hover::Row(i)) => {
                self.typing = false;
                let key = self.rows[i].key;
                self.select_index(i);
                if self.on_chevron(i, p) {
                    self.toggle(key);
                } else if clicks == 2 {
                    self.activate(key);
                }
            }
            None => self.typing = false,
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        self.last_pointer = Some(p);
        if let Some(center) = self.splitter.drag_to(p) {
            let width = (center - self.bounds.min_x() - GAP * self.scale / 2.0) / self.scale;
            let max = (self.bounds.size.width / self.scale * 0.6).max(TREE_MIN);
            let width = width.clamp(TREE_MIN, max).round();
            let changed = width != self.tree_width;
            self.tree_width = width;
            return changed;
        }
        if self.dragging_view {
            return self.active_view().is_some_and(|v| v.drag(p));
        }
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            return changed;
        }
        if self.selecting {
            return Control::drag(&mut self.filter, p);
        }
        false
    }

    fn release(&mut self) -> bool {
        self.selecting = false;
        let split = self.splitter.end_drag();
        let held = std::mem::take(&mut self.dragging_view);
        let view = held && self.active_view().is_some_and(|v| v.release());
        // A menu its release asked for (a button decided on letting go): the app's to show.
        if held && let Some(asked) = self.view_answer(None) {
            self.action = Some(asked);
        }
        split | view | self.bar.release()
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_pointer = Some(p);
        let split = self.splitter.hover(p);
        let bar = self.bar.hover(p);
        let view = self.view_area.contains(p) && self.active_view().is_some_and(|v| v.hover(p));
        let hover = if self.view_area.contains(p) { None } else { self.hit(p) };
        // The filter's × lit under the mouse.
        let clear = Control::hover(&mut self.filter, Some(p));
        let changed = clear || hover != self.hover;
        self.hover = hover;
        changed || bar || split || view
    }

    fn clickable(&self, p: Point) -> bool {
        matches!(self.hit(p), Some(Hover::Row(_) | Hover::Clear | Hover::Tab(_)))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.splitter.contains(p) {
            return Some(ViewerCursor::ResizeHorizontal);
        }
        if self.view_area.contains(p) {
            let id = self.active_tab()?;
            return self.views.get(&id)?.cursor_at(p);
        }
        match self.hit(p)? {
            Hover::Field | Hover::Clear => Control::cursor(&self.filter, p),
            _ => None,
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        if self.view_area.contains(p) {
            let id = self.active_tab()?;
            return self.views.get(&id)?.tooltip_at(p);
        }
        self.state_tip(p)
    }

    fn wants_text(&self) -> bool {
        if self.in_view {
            return self
                .active_tab()
                .and_then(|id| self.views.get(&id))
                .is_some_and(|v| v.wants_text());
        }
        self.typing
    }

    fn takes_keys(&self) -> bool {
        true
    }

    fn keyed(&self) -> bool {
        true
    }

    fn keyed_view(&self) -> Option<&'static str> {
        if !self.in_view {
            return None;
        }
        let view = self.active_tab().and_then(|id| self.views.get(&id))?;
        if view.wants_text() { None } else { view.actions_view() }
    }

    fn text_input(&mut self, text: &str) -> bool {
        if self.in_view {
            return self.active_view().is_some_and(|v| v.text_input(text));
        }
        // On one line, the field's own way (its breaks as spaces).
        Control::text(&mut self.filter, text);
        self.filtered();
        true
    }

    /// Typing on the rows filters them (the field takes the keys).
    fn text_on_rows(&mut self, text: &str) -> bool {
        // In a view that takes no text: a single key of its own (a tree's `*`, the first
        // letter of its filter).
        if self.in_view {
            let mut chars = text.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) if !c.is_control() => {
                    let used = self.active_view().is_some_and(|v| v.key(c));
                    let action = self.active_view().and_then(|v| v.take_action());
                    self.action = self.view_action(action);
                    used
                }
                _ => false,
            };
        }
        if text.chars().all(char::is_control) {
            return false;
        }
        self.typing = true;
        self.filter.input.select_all();
        self.text_input(text)
    }

    fn selected_text(&self) -> Option<String> {
        if self.in_view {
            return self.active_tab().and_then(|id| self.views.get(&id))?.selected_text();
        }
        Control::selected_text(&self.filter).filter(|_| self.typing)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if self.in_view {
            let used = self.active_view().is_some_and(|v| v.edit_key_with(key, reach));
            let action = self.active_view().and_then(|v| v.take_action());
            self.action = self.view_answer(action);
            // Esc in a view that didn't take it: the keys back to the tree.
            if !used && key == ViewerKey::Escape {
                self.in_view = false;
                return true;
            }
            return used;
        }
        if self.typing {
            let used = match key {
                // The field's editing keys (what erases filters again).
                ViewerKey::Backspace | ViewerKey::Delete => {
                    Control::key(&mut self.filter, key, reach)
                }
                ViewerKey::Left | ViewerKey::Right | ViewerKey::Home | ViewerKey::End => {
                    return Control::key(&mut self.filter, key, reach);
                }
                ViewerKey::Escape => {
                    self.filter.set_text("");
                    self.typing = false;
                    true
                }
                // The rows take the keys back (↑ ↓ ↵ through what's filtered).
                _ => {
                    self.typing = false;
                    return self.row_key(key);
                }
            };
            if used {
                self.filtered();
            }
            return used;
        }
        self.row_key(key)
    }

    fn command(&mut self, id: &str) -> bool {
        if self.in_view {
            let used = self.active_view().is_some_and(|v| v.command(id));
            let action = self.active_view().and_then(|v| v.take_action());
            self.action = self.view_answer(action);
            return used;
        }
        // The filter with the keys: the app's editing commands are its (⌘A, ⌘X, ⌘Z, ⌘⇧Z).
        if self.typing && Control::command(&mut self.filter, id) {
            if id != SELECT_ALL {
                self.filtered();
            }
            return true;
        }
        self.tree_command(id)
    }

    fn header(&self) -> Option<Vec<PanelButton>> {
        let button =
            |id, icon, key: &str, group| PanelButton { id, icon, tooltip: i18n::t(key), group };
        // Nothing to open or fold yet: only its title (and the app's hide).
        if self.rows.is_empty() && self.filter.text().trim().is_empty() {
            return Some(Vec::new());
        }
        Some(vec![
            button("expand", "chevrons-expand", "services.expand_all", 0),
            button("collapse", "chevrons-collapse", "services.collapse_all", 0),
        ])
    }

    fn header_action(&mut self, id: &str) -> Option<ToolPanelAction> {
        self.tree_command(id);
        None
    }

    fn menu_at(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if self.view_area.contains(p) {
            self.in_view = true;
            self.menu_target = Some(MenuTarget::View);
            let entries = self.active_view()?.context_menu(p)?;
            return (!entries.is_empty()).then_some(entries);
        }
        if let Some(TabHit::Tab(i) | TabHit::Close(i)) = self.tab_hit(p) {
            let id = self.tabs.get(i)?.id;
            self.menu_target = Some(MenuTarget::Tab(id));
            return Some(self.tab_menu(i));
        }
        let i = self.row_at(p)?;
        let key = self.rows[i].key;
        self.select_index(i);
        let entries = match key {
            Key::Node(provider, node) => self
                .contributors
                .iter()
                .find(|c| c.id == provider)
                .map(|c| c.services.menu(node))
                .unwrap_or_default(),
            Key::Group(_) => Vec::new(),
        };
        self.menu_target = Some(MenuTarget::Node(key));
        (!entries.is_empty()).then_some(entries)
    }

    fn menu_command(&mut self, id: &str) -> bool {
        match self.menu_target.take() {
            Some(MenuTarget::View) => {
                let used = self.active_view().is_some_and(|v| v.command(id));
                let action = self.active_view().and_then(|v| v.take_action());
                self.action = self.view_answer(action);
                used
            }
            Some(MenuTarget::Tab(tab)) => {
                let Some(i) = self.tabs.iter().position(|t| t.id == tab) else { return false };
                match id {
                    "tab.pin" => self.toggle_pin(i),
                    "tab.close" => self.close_tab(i),
                    "tab.close_others" => self.close_other_tabs(i),
                    _ => return false,
                }
                true
            }
            Some(MenuTarget::Node(Key::Node(provider, node))) => {
                let Some(c) = self.contributor(provider) else { return false };
                let used = c.services.command(node, id);
                // What the row asks of the app (a command needing the host).
                let action = c.services.take_action();
                self.action = self.view_action(action);
                used
            }
            _ => false,
        }
    }

    fn focus_lost(&mut self) {
        self.typing = false;
    }

    fn web_page(&self) -> Option<ui_components::WebPage> {
        // The shown tab's page (a response's preview).
        self.views.get(&self.active_tab()?)?.web_page()
    }

    fn web_event(&mut self, page: u128, event: ui_components::WebEvent) {
        // The view whose page it is, shown or not.
        if let Some(view) =
            self.views.values_mut().find(|v| v.web_page().is_some_and(|p| p.id == page))
        {
            view.web_event(page, event);
        }
    }

    fn take_web_sends(&mut self) -> Vec<(u128, String)> {
        self.views.values_mut().flat_map(|v| v.take_web_sends()).collect()
    }

    fn take_action(&mut self) -> Option<ToolPanelAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        self.views.values_mut().flat_map(|v| v.take_notifications()).collect()
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
#[path = "../tests/unit/panel.rs"]
mod tests;
