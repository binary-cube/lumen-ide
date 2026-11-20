//! The tool windows at the bottom, registered by modules like any other
//! (the terminal, Git, Remote): they share the area under the editor, one shown at a time —
//! a click on its icon in the tool strip shows / hides it, another one switches.
//!
//! A window holds its module's views (`WorkbenchHost::show_in_panel`) as small tabs: pinned
//! ones first (no ×), renamed with a double click or the tab's menu, reordered by dragging;
//! a view may be fixed (`Viewer::closable`: Git's Log and Changes): no close ever takes it.
//! Like the editor, a window splits into groups (its tab's menu: Split Right / Down…), each
//! with its own tabs and its active view shown, laid out by a split tree with the same
//! separators ([`super::splits`]); the current group has the keyboard and gets the new
//! views. Modules know nothing of it: they only give their views. A hidden window isn't
//! painted; after [`RELEASE_AFTER`] hidden its views let their data go (`Viewer::purge`) and
//! read it again when shown; a view that ended on its own (a shell's `exit`) closes, shown
//! or not. The terminal's shells keep running when hidden.

use super::splits::SplitHandles;
use super::*;
use crate::split_tree::SplitTree;
use ui_workbench::ds::{self, radius};

mod keyboard;
mod paint;
mod pointer;
mod saved;
mod tab_drag;

/// What the pieces of a group's header share: the drawing context, the group's area, the
/// header's height and middle, what's under the mouse.
struct ToolHeader<'a> {
    ctx: ds::Ctx<'a>,
    bounds: Rect,
    height: f32,
    mid: f32,
    hover: Option<ToolHit>,
}

/// How long a hidden window keeps its views' data.
const RELEASE_AFTER: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// A view's identity in its window (a UUID v4: it never repeats, whatever the view is
/// called).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ViewId(uuid::Uuid);

impl ViewId {
    fn next() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

/// A view in a tool window, as its module opened it.
pub(super) struct PanelView {
    pub(super) id: ViewId,
    /// The name its module opened it by (it finds it again by this, even renamed).
    pub(super) key: String,
    /// Its tab's title (the key, or the user's name for it).
    pub(super) title: String,
    /// Its viewer's virtual extension (to make it again from a saved session).
    pub(super) extension: String,
    /// Kept first, without ×.
    pub(super) pinned: bool,
    /// Its viewer's id (where its actions' keys work).
    pub(super) provider: &'static str,
    viewer: Box<dyn ui_workbench::Viewer>,
}

impl PanelView {
    /// Its viewer's id and what it holds (the Task Manager).
    pub(super) fn memory(&self) -> (&'static str, ui_workbench::ViewMemory) {
        (self.provider, self.viewer.memory())
    }

    /// Its viewer.
    pub(super) fn viewer_mut(&mut self) -> &mut Box<dyn ui_workbench::Viewer> {
        &mut self.viewer
    }

    /// Its tab can be closed (a fixed one can't: [`ui_workbench::Viewer::closable`]).
    pub(super) fn closable(&self) -> bool {
        self.viewer.closable()
    }

    /// One of its fields takes the keys.
    fn wants_text(&self) -> bool {
        self.viewer.wants_text()
    }
}

/// A group of a tool window (one unless split): its tabs and the one shown.
#[derive(Default)]
pub(super) struct ToolGroup {
    pub(super) views: Vec<PanelView>,
    pub(super) active: usize,
    /// Its area in the window's card: its header on top, its view below.
    pub(super) bounds: Rect,
}

impl ToolGroup {
    pub(super) fn active_view(&self) -> Option<&PanelView> {
        self.views.get(self.active)
    }

    fn active_view_mut(&mut self) -> Option<&mut PanelView> {
        self.views.get_mut(self.active)
    }

    /// Removes view `i`; the active one stays itself (or its neighbour, if it was `i`).
    fn remove(&mut self, i: usize) -> Option<PanelView> {
        if i >= self.views.len() {
            return None;
        }
        let view = self.views.remove(i);
        if self.active > i {
            self.active -= 1;
        }
        self.active = self.active.min(self.views.len().saturating_sub(1));
        Some(view)
    }
}

/// A module's tool window at the bottom: its groups (split like the editor's), the current
/// one (the keyboard's, where new views go) and how they're laid out.
pub(super) struct ToolWindow {
    pub(super) id: String,
    pub(super) groups: Vec<ToolGroup>,
    pub(super) current: usize,
    pub(super) tree: SplitTree,
    pub(super) split: SplitHandles,
    /// Since when it's hidden (its views' data is let go after a while); `None`: shown.
    hidden_since: Option<std::time::Instant>,
    purged: bool,
}

impl std::fmt::Debug for ToolWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolWindow")
            .field("id", &self.id)
            .field("groups", &self.groups.iter().map(|g| g.views.len()).collect::<Vec<_>>())
            .finish()
    }
}

impl Purge for ToolWindow {
    /// Its views let go of what they can rebuild.
    fn purge(&mut self) {
        for view in self.groups.iter_mut().flat_map(|g| &mut g.views) {
            view.viewer.purge();
        }
    }
}

impl ToolWindow {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            groups: vec![ToolGroup::default()],
            current: 0,
            tree: SplitTree::default(),
            split: SplitHandles::default(),
            hidden_since: None,
            purged: false,
        }
    }

    /// The current group.
    pub(super) fn group(&self) -> &ToolGroup {
        &self.groups[self.current]
    }

    pub(super) fn group_mut(&mut self) -> &mut ToolGroup {
        &mut self.groups[self.current]
    }

    /// Every view, group after group.
    pub(super) fn views(&self) -> impl Iterator<Item = &PanelView> {
        self.groups.iter().flat_map(|g| &g.views)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.groups.iter().all(|g| g.views.is_empty())
    }

    /// The group and place of the view `pick` chooses.
    fn find(&self, pick: impl Fn(&PanelView) -> bool) -> Option<(usize, usize)> {
        self.groups
            .iter()
            .enumerate()
            .find_map(|(g, group)| Some((g, group.views.iter().position(&pick)?)))
    }

    /// The groups left empty go (one is always kept): their space to a neighbour, the
    /// current one staying itself or becoming the one next to it. `true` if one went.
    pub(super) fn prune(&mut self) -> bool {
        let mut pruned = false;
        for g in (0..self.groups.len()).rev() {
            if self.groups.len() > 1 && self.groups[g].views.is_empty() {
                self.groups.remove(g);
                self.tree.remove(g);
                if self.current > g || self.current >= self.groups.len() {
                    self.current = self.current.saturating_sub(1);
                }
                pruned = true;
            }
        }
        pruned
    }

    /// The current group split (`dir`): a new, empty group after it becomes current.
    pub(super) fn split_current(&mut self, dir: Split) -> usize {
        let at = self.current + 1;
        self.tree.split(self.current, dir);
        self.groups.insert(at, ToolGroup::default());
        self.current = at;
        at
    }
}

/// What the tool window's header has under the mouse: a group's tab (group, index), its
/// ×, its "+", or the window's hide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolHit {
    View(usize, usize),
    CloseView(usize, usize),
    /// "+" after a group's tabs: another view there (the window's `new` command).
    New(usize),
    /// Half the height, all of it (pressed again: back to its own size), hidden.
    Half,
    Full,
    /// The window's options ("⋮": its module's rows, then the app's).
    More,
    Hide,
}

/// How tall the bottom area is: its own size (the project's, dragged), half the room, or all
/// of it (the editor and the side panels out of the way). The size it keeps isn't touched, so
/// leaving half or all of it goes back to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum BottomSize {
    #[default]
    Kept,
    Half,
    Full,
}

/// Where a press on a tab of the shown window is: pressed (a click so far), dragged (its
/// tab follows the pointer, the others make room), or given up with Esc (until release).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TabDragState {
    Pressed,
    Moving,
    Cancelled,
}

/// A press on one of the shown window's tabs, which can become a drag reordering them.
/// The tabs' widths are taken from the strip as painted when pressed (titles don't change
/// meanwhile); the views move as the tab crosses its neighbours, so the active one stays
/// itself and Esc puts it back where it was.
#[derive(Debug, Clone)]
pub(super) struct ToolTabDrag {
    view: ViewId,
    /// Its group, and where it was in it when pressed (Esc puts it back there).
    group: usize,
    origin: usize,
    press: Point,
    /// The pointer's distance from the tab's left edge.
    grab: f32,
    /// The tab's left edge now, as the pointer puts it (not kept within the strip).
    left: f32,
    /// The first tab's left edge and the space between tabs.
    start: f32,
    gap: f32,
    /// The tabs' widths, in the views' order (moved along with them).
    widths: Vec<f32>,
    pub(super) state: TabDragState,
    /// The pointer, and whether it's over the tab's own strip (the tab follows it there);
    /// elsewhere, where a drop would put it (none: back where it was).
    at: Point,
    in_strip: bool,
    target: Option<ToolDrop>,
}

/// A side of a group's view a tab can be dropped on: a new group there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DropSide {
    Left,
    Right,
    Top,
    Bottom,
}

impl DropSide {
    /// The split it makes, and whether the new group comes before the one dropped on.
    fn split(self) -> (Split, bool) {
        match self {
            Self::Left => (Split::Right, true),
            Self::Right => (Split::Right, false),
            Self::Top => (Split::Down, true),
            Self::Bottom => (Split::Down, false),
        }
    }
}

/// Where a tab dragged out of its strip would go: another group's tabs (at `index`, the
/// insertion line `marker`), into another group (the middle of its view), or a new group on
/// a side of a group's view (`zone`: the part it would take).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ToolDrop {
    Strip { group: usize, index: usize, marker: Rect },
    Into { group: usize, zone: Rect },
    Edge { group: usize, side: DropSide, zone: Rect },
}

/// How close to a side of a view (a share of its size) a drop makes a group there.
const EDGE_SHARE: f32 = 0.25;

/// The side of `view` the point `p` is near (within [`EDGE_SHARE`]), and the half it would
/// take; `None`: its middle.
fn drop_side(view: Rect, p: Point) -> Option<(DropSide, Rect)> {
    let (w, h) = (view.size.width.max(1.0), view.size.height.max(1.0));
    let fx = (p.x - view.min_x()) / w;
    let fy = (p.y - view.min_y()) / h;
    let sides = [
        (fx, DropSide::Left),
        (1.0 - fx, DropSide::Right),
        (fy, DropSide::Top),
        (1.0 - fy, DropSide::Bottom),
    ];
    let (near, side) = sides.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))?;
    (near < EDGE_SHARE).then(|| {
        let zone = match side {
            DropSide::Left => Rect::new(view.min_x(), view.min_y(), w / 2.0, h),
            DropSide::Right => Rect::new(view.min_x() + w / 2.0, view.min_y(), w / 2.0, h),
            DropSide::Top => Rect::new(view.min_x(), view.min_y(), w, h / 2.0),
            DropSide::Bottom => Rect::new(view.min_x(), view.min_y() + h / 2.0, w, h / 2.0),
        };
        (side, zone)
    })
}

impl ToolTabDrag {
    /// Where the dragged tab is drawn: under the pointer, kept within the tabs' span (never
    /// over the window's name before them or the "+" after them).
    fn floating_left(&self, at: usize) -> f32 {
        let width = self.widths.get(at).copied().unwrap_or(0.0);
        let span =
            self.widths.iter().sum::<f32>() + self.gap * self.widths.len().saturating_sub(1) as f32;
        self.left.clamp(self.start, (self.start + span - width).max(self.start))
    }
}

/// The place among the tabs for the one at `at` (of `widths`, laid out from `start` with
/// `gap` between), its left edge at `left`: before the first other one whose middle it
/// hasn't passed, kept within `range` (the pinned ones stay first).
fn tab_drop_index(
    widths: &[f32],
    at: usize,
    left: f32,
    start: f32,
    gap: f32,
    range: std::ops::RangeInclusive<usize>,
) -> usize {
    let dragged = widths[at];
    let center = left + dragged / 2.0;
    let mut x = start;
    let mut index = 0;
    for (_, &other) in widths.iter().enumerate().filter(|(j, _)| *j != at) {
        // Before this one or after it: whichever puts the dragged tab's middle closer.
        if center < x + (dragged + other + gap) / 2.0 {
            break;
        }
        x += other + gap;
        index += 1;
    }
    index.clamp(*range.start(), *range.end())
}

impl Workbench {
    /// What the bottom windows' views hold (their viewer's id, their memory).
    pub(super) fn panel_views_memory(
        &self,
    ) -> impl Iterator<Item = (&'static str, ui_workbench::ViewMemory)> + '_ {
        self.tools.windows.iter().flat_map(|w| w.views().map(PanelView::memory))
    }

    /// What the bottom area shows becomes window `id` (`None`: nothing): a module's window
    /// of views, or a panel docked there. The window leaving lets go of its views' data after
    /// a while, a panel leaving is set aside as it is; the keyboard leaves it.
    pub(super) fn set_bottom(&mut self, id: Option<&str>) {
        if self.tools.bottom.as_deref() == id {
            return;
        }
        if let Some(window) = self.tools.window_mut() {
            window.hidden_since = Some(std::time::Instant::now());
        }
        if self.focus == Focus::Panel {
            self.focus = Focus::Editor;
        }
        // A panel shown at the bottom (Services) is set aside for what comes in its place.
        if let Some(panel) =
            self.tools.side_on(ui_workbench::Dock::Bottom).map(|s| s.id).filter(|p| Some(*p) != id)
        {
            self.close_side_panel(panel);
        }
        // Hidden: shown again at its own size.
        if id.is_none() {
            self.tools.bottom_size = BottomSize::Kept;
        }
        self.tools.bottom = id.map(ToOwned::to_owned);
        if let Some(window) = self.tools.window_mut() {
            window.hidden_since = None;
            window.purged = false;
        }
    }

    /// Half or all the height (`size`); pressed again, back to its own size.
    /// The tooltip's key of the button giving the bottom area `size`: back to its size when
    /// it has it, else what it does.
    pub(super) fn bottom_size_tooltip(&self, size: BottomSize) -> &'static str {
        match size {
            _ if self.tools.bottom_size == size => "tooltip.bottom_restore",
            BottomSize::Full => "tooltip.bottom_full",
            _ => "tooltip.bottom_half",
        }
    }

    pub(super) fn toggle_bottom_size(&mut self, size: BottomSize) {
        let now = self.tools.bottom_size;
        self.tools.bottom_size = if now == size { BottomSize::Kept } else { size };
        self.layout();
    }

    #[cfg(feature = "snapshot")]
    /// The bottom window half the height or all of it (snapshots).
    pub fn bottom_size_for_snapshot(&mut self, full: bool) {
        self.toggle_bottom_size(if full { BottomSize::Full } else { BottomSize::Half });
    }

    /// The bottom area shows window `id`.
    pub(super) fn bottom_shows(&self, id: &str) -> bool {
        self.tools.bottom.as_deref() == Some(id)
    }

    /// The kind of the bottom window's view, unless one of its fields takes the keys.
    pub(super) fn keyed_panel_view(&self) -> Option<&'static str> {
        let view = self.tools.window()?.group().active_view()?;
        // A view taking the whole keyboard (the terminal) still has its own commands: its
        // keys reach the app only as ⌘ / Ctrl+Shift shortcuts, never as typing.
        (!view.wants_text() || view.viewer.takes_keyboard()).then_some(view.provider)
    }

    /// The bottom area shows a module's window (not the terminal).
    pub(super) fn tool_window_shown(&self) -> bool {
        self.tools.window().is_some()
    }

    /// Shows `view` (made by the viewer of `extension`, given `data`) in tool window `tool`
    /// at the bottom — the view found by `key` again if it's open (in whichever group); a
    /// new one, titled `title`, goes to the current group. `false` if no module shows
    /// `extension`.
    pub(super) fn show_in_panel(
        &mut self,
        tool: &str,
        key: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        let Some((w, g, v)) = self.panel_view(tool, key, title, extension) else { return false };
        let window = &mut self.tools.windows[w];
        window.current = g;
        window.groups[g].active = v;
        window.groups[g].views[v].viewer.set_data(data);
        self.show_tool_window(tool);
        true
    }

    /// The view `key` made in tool window `tool` (unless it has it) behind the ones shown,
    /// given `data` if made now: nothing shown changes (a fixed tab a session lacked).
    pub(super) fn add_to_panel(
        &mut self,
        tool: &str,
        key: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        if self.tools.windows.iter().any(|w| w.id == tool && w.find(|v| v.key == key).is_some()) {
            return true;
        }
        let Some((w, g, v)) = self.panel_view(tool, key, title, extension) else { return false };
        self.tools.windows[w].groups[g].views[v].viewer.set_data(data);
        true
    }

    /// Where view `key` is in tool window `tool` (window, group, view), made at the end of
    /// its current group if it has none (the window too). `None`: no view for `extension`.
    fn panel_view(
        &mut self,
        tool: &str,
        key: &str,
        title: &str,
        extension: &str,
    ) -> Option<(usize, usize, usize)> {
        let found = self.tools.windows.iter().position(|w| w.id == tool).and_then(|w| {
            let (g, v) = self.tools.windows[w].find(|v| v.key == key)?;
            Some((w, g, v))
        });
        if found.is_some() {
            return found;
        }
        let path = PathBuf::from(format!("{title}.{extension}"));
        let provider = self
            .support
            .viewers
            .iter()
            .find(|p| p.kind(&path) == Some(ui_workbench::ViewerKind::Replace));
        let (Ok(mut viewer), provider) = provider.map(|p| (p.open(&path), p.id()))? else {
            return None;
        };
        self.give_sizes(viewer.as_mut());
        let w = match self.tools.windows.iter().position(|w| w.id == tool) {
            Some(w) => w,
            None => {
                self.tools.windows.push(ToolWindow::new(tool));
                self.tools.windows.len() - 1
            }
        };
        let window = &mut self.tools.windows[w];
        let g = window.current;
        window.groups[g].views.push(PanelView {
            id: ViewId::next(),
            key: key.to_owned(),
            title: title.to_owned(),
            extension: extension.to_owned(),
            pinned: false,
            provider,
            viewer,
        });
        Some((w, g, window.groups[g].views.len() - 1))
    }

    /// Shows tool window `id` at the bottom (instead of whatever was there).
    fn show_tool_window(&mut self, id: &str) {
        self.set_bottom(Some(id));
        self.focus = Focus::Panel;
        self.layout();
    }

    /// The tool strip's button for window `id`: shows it, or hides it if it's shown.
    /// `false` if it has no views yet (its module opens one).
    pub(super) fn toggle_tool_window(&mut self, id: &str) -> bool {
        if self.bottom_shows(id) {
            self.set_bottom(None);
            self.layout();
            return true;
        }
        if !self.tools.windows.iter().any(|w| w.id == id && !w.is_empty()) {
            return false;
        }
        self.show_tool_window(id);
        true
    }

    /// The windows' views' background work (a page read, details), and their data let go
    /// after a while hidden. `true` if a redraw is needed.
    pub(super) fn poll_tool_windows(&mut self) -> bool {
        let mut busy = false;
        let shown = self.tools.bottom.clone();
        // Views that ended on their own (a shell's `exit`) close, shown or not; a group left
        // empty goes.
        let mut emptied = false;
        let mut relayout = false;
        for window in &mut self.tools.windows {
            let here = shown.as_deref() == Some(window.id.as_str());
            let mut closed = false;
            for group in &mut window.groups {
                let active = group.active_view().map(|v| v.id);
                let before = group.views.len();
                group.views.retain(|v| !v.viewer.closed());
                if group.views.len() != before {
                    closed = true;
                    group.active = active
                        .and_then(|id| group.views.iter().position(|v| v.id == id))
                        .unwrap_or(group.active)
                        .min(group.views.len().saturating_sub(1));
                }
            }
            if closed {
                relayout |= window.prune() && here;
                busy |= here;
                emptied |= here && window.is_empty();
            }
        }
        if emptied {
            self.set_bottom(None);
        }
        if emptied || relayout {
            self.layout();
        }
        for window in &mut self.tools.windows {
            if shown.as_deref() == Some(window.id.as_str()) {
                for group in &mut window.groups {
                    if let Some(view) = group.active_view_mut() {
                        busy |= view.viewer.poll();
                        // What its background work prepared: done at the wake's next step.
                        self.panel_actions.extend(view.viewer.take_action());
                    }
                }
            } else if !window.purged
                && window.hidden_since.is_some_and(|t| t.elapsed() > RELEASE_AFTER)
            {
                window.purge();
                window.purged = true;
            }
        }
        // The side panels hidden as long: their data goes too (made again when shown).
        for side in &mut self.tools.parked {
            if side.parked_since.is_some_and(|t| t.elapsed() > RELEASE_AFTER) {
                side.panel.purge();
                side.parked_since = None;
            }
        }
        busy
    }

    /// What's under `p` in the window's headers: the topmost (a tab's ×, drawn over its tab,
    /// was registered after it).
    fn tool_hit_at(&self, p: Point) -> Option<ToolHit> {
        self.tools.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// Group `g` of the shown window becomes current (its view gets the keyboard).
    pub(super) fn focus_tool_group(&mut self, g: usize) {
        if let Some(window) = self.tools.window_mut()
            && g < window.groups.len()
        {
            window.current = g;
        }
    }

    /// Closes view `i` of the shown window's current group.
    pub(super) fn close_tool_view(&mut self, i: usize) {
        let g = self.tools.window().map_or(0, |w| w.current);
        self.close_tool_views(g, |j, _| j == i);
    }

    /// Closes the views of group `g` of the shown window that `close` picks (by their place
    /// and the view) — never a fixed one; the group goes if it's left empty, the window hides
    /// once it has none.
    pub(super) fn close_tool_views(&mut self, g: usize, close: impl Fn(usize, &PanelView) -> bool) {
        let empty = self.tools.window_mut().is_some_and(|w| {
            let Some(group) = w.groups.get_mut(g) else { return false };
            let active = group.active_view().map(|v| v.id);
            let mut j = 0;
            group.views.retain(|v| {
                let keep = !v.closable() || !close(j, v);
                j += 1;
                keep
            });
            group.active = active
                .and_then(|id| group.views.iter().position(|v| v.id == id))
                .unwrap_or(group.active)
                .min(group.views.len().saturating_sub(1));
            w.prune();
            w.is_empty()
        });
        if empty {
            self.set_bottom(None);
        }
        self.layout();
    }

    /// The shown window's view `i` (of its current group): its title, and its tab as its
    /// menu sees it (pinned, closable, how many views the group has).
    pub(super) fn tool_view_at(&self, i: usize) -> Option<(String, crate::commands::ToolTab)> {
        let group = self.tools.window()?.group();
        let v = group.views.get(i)?;
        let tab = crate::commands::ToolTab {
            index: i,
            count: group.views.len(),
            pinned: v.pinned,
            closable: v.closable(),
        };
        Some((v.title.clone(), tab))
    }

    /// The shown window's tab under `p` (a right click): its group becomes current; the tab
    /// as its menu sees it.
    pub fn tool_tab_menu_at(&mut self, p: Point) -> Option<crate::commands::ToolTab> {
        let (g, i) = self.tool_tab_at(p)?;
        self.focus_tool_group(g);
        Some(self.tool_view_at(i)?.1)
    }

    /// The tab of the shown window under `p`: its group and place.
    pub(super) fn tool_tab_at(&self, p: Point) -> Option<(usize, usize)> {
        self.tools.hits.iter().find_map(|(r, hit)| match hit {
            ToolHit::View(g, i) if r.contains(p) => Some((*g, *i)),
            _ => None,
        })
    }

    /// The shown window makes views (the terminal: a shell): its `new` command.
    pub(super) fn tool_new_command(&self) -> Option<&'static str> {
        self.tools.bottom.as_deref().and_then(|id| self.tool(id)).and_then(|t| match t.content() {
            ui_workbench::ToolContent::Views { new, .. } => new,
            _ => None,
        })
    }

    /// The shown window makes views (its tabs' menus offer another one in a new group).
    pub fn tool_makes_views(&self) -> bool {
        self.tool_new_command().is_some()
    }

    /// The shown window's current group split (`dir`) for its view `i`: moved into the new
    /// group (`move_view`, when it isn't the group's only one), or another view made there
    /// by the window's `new` command (a shell). `false` if neither can be.
    pub(super) fn split_tool_view(&mut self, i: usize, dir: Split, move_view: bool) -> bool {
        let new = self.tool_new_command();
        let Some(window) = self.tools.window_mut() else { return false };
        let count = window.group().views.len();
        if i >= count || (move_view && count < 2) || (!move_view && new.is_none()) {
            return false;
        }
        if move_view {
            let Some(view) = window.group_mut().remove(i) else { return false };
            window.split_current(dir);
            window.group_mut().views.push(view);
        } else if let Some(command) = new {
            window.split_current(dir);
            self.run_tool_command(command_model::CommandId(command));
            // Not made (yet): no empty group is left behind.
            if let Some(window) = self.tools.window_mut() {
                window.prune();
            }
        }
        self.focus = Focus::Panel;
        self.layout();
        true
    }

    /// Next Group / Previous Group: the keyboard to the bottom window's next (previous)
    /// group when it has it, else to the editor's. `false` if there's no other one.
    pub fn cycle_group(&mut self, forward: bool) -> bool {
        if self.focus == Focus::Panel
            && let Some(window) = self.tools.window_mut()
        {
            let n = window.groups.len();
            if n < 2 {
                return false;
            }
            window.current =
                if forward { (window.current + 1) % n } else { (window.current + n - 1) % n };
            return true;
        }
        let n = self.editors.groups.len();
        n > 1 && {
            let current = self.editors.current;
            self.focus_group(if forward { (current + 1) % n } else { (current + n - 1) % n })
        }
    }

    /// Close Group: the bottom window's current group with its views (the bottom window has
    /// the keyboard), its space to a neighbour; the window hides with its last one. Its fixed
    /// views move to the neighbour (alone, they stay). `false` if the bottom window doesn't
    /// have the keyboard.
    pub fn close_group(&mut self) -> bool {
        if self.focus != Focus::Panel || !self.tool_window_shown() {
            return false;
        }
        let g = self.tools.window().map_or(0, |w| w.current);
        if let Some(window) = self.tools.window_mut()
            && window.groups.len() > 1
            && let Some(group) = window.groups.get_mut(g)
        {
            let (fixed, rest): (Vec<PanelView>, Vec<PanelView>) =
                std::mem::take(&mut group.views).into_iter().partition(|v| !v.closable());
            group.views = rest;
            let to = if g > 0 { g - 1 } else { 1 };
            window.groups[to].views.extend(fixed);
        }
        self.close_tool_views(g, |_, _| true);
        true
    }

    /// The tool window toggled by command `id` (its registered toggle).
    pub fn tool_toggled_by(&self, id: command_model::CommandId) -> Option<&'static str> {
        self.support.tools.iter().find(|t| t.toggle_command() == Some(id.0)).map(|t| t.id())
    }

    /// Shows / hides tool window `id`: a panel on its side, a window at the bottom (its
    /// first view made by its command when it has none), or its command run.
    pub fn toggle_tool(&mut self, id: &str) -> bool {
        let Some(tool) = self.tool(id).cloned() else { return false };
        match tool.content() {
            // Toggled by an app's own command (the project panel: without a project, one is
            // opened): that command decides.
            ui_workbench::ToolContent::Panel => match tool
                .toggle_command()
                .filter(|&c| !self.support.commands.iter().any(|m| m.spec.id.0 == c))
            {
                Some(command) => {
                    self.run_tool_command(command_model::CommandId(command));
                    true
                }
                None => self.toggle_side_panel(id),
            },
            ui_workbench::ToolContent::Views { command, .. } => {
                if !self.toggle_tool_window(id) {
                    self.run_tool_command(command_model::CommandId(command));
                }
                true
            }
            ui_workbench::ToolContent::Command { command, .. } => {
                self.run_tool_command(command_model::CommandId(command));
                true
            }
        }
    }

    /// The bottom view has the keyboard and its window makes views (the terminal): another
    /// one (⌘⇧T), in the current group. `false` if not.
    pub fn new_in_panel(&mut self) -> bool {
        if self.focus != Focus::Panel {
            return false;
        }
        self.tool_new_command().is_some_and(|command| {
            self.run_tool_command(command_model::CommandId(command));
            true
        })
    }

    /// When a view next wants a frame at its own pace (a scrollbar fading): the shown
    /// bottom window's, the side panels', the tabs'.
    pub fn views_deadline(&self) -> Option<std::time::Instant> {
        let bottom =
            self.tools.window().into_iter().flat_map(|w| {
                w.groups.iter().filter_map(|g| g.active_view()?.viewer.frame_deadline())
            });
        let sides = self.tools.sides.iter().filter_map(|s| s.panel.frame_deadline());
        let tabs = super::tab::all_tabs(&self.tabs, &self.editors.groups)
            .filter_map(|t| t.viewer.as_ref()?.viewer.frame_deadline());
        // The code editors' scrollbars fading.
        let editors = super::tab::all_tabs(&self.tabs, &self.editors.groups)
            .filter_map(|t| t.view.scrollbar_deadline());
        bottom.chain(sides).chain(tabs).chain(editors).min()
    }

    /// For a profile recording only (`kernel::probe`): each part that asks for the next frame
    /// told by its name — moving (an animation: counted now, it draws this frame), or a time
    /// of its own (`asked`: counted once by the frame that comes for it). Nothing is looked at
    /// unless one records.
    pub fn probe_frame_askers(&self, asked: &mut Vec<(&'static str, std::time::Instant)>) {
        use kernel::probe::{Cause, trigger};
        if !kernel::probe::recording() {
            return;
        }
        let mut ask = |owner: &'static str, moving: bool, due: Option<std::time::Instant>| {
            if moving {
                trigger(owner, Cause::Animation);
            }
            if let Some(at) = due {
                asked.push((owner, at));
            }
        };
        if let Some(w) = self.tools.window() {
            for v in w.groups.iter().filter_map(|g| g.active_view()) {
                ask(v.provider, v.viewer.is_animating(), v.viewer.frame_deadline());
            }
        }
        for s in &self.tools.sides {
            ask(s.id, s.panel.is_animating(), s.panel.frame_deadline());
        }
        for t in super::tab::all_tabs(&self.tabs, &self.editors.groups) {
            if let Some(v) = &t.viewer {
                ask(v.provider, v.viewer.is_animating(), v.viewer.frame_deadline());
            }
            ask("editor", t.view.is_animating(), t.view.scrollbar_deadline());
        }
        ask("tooltips", self.tooltips.is_animating(), None);
        ask("code-hover", self.code_hover.is_animating(), None);
        ask("toolbars", self.toolbars_animating(), None);
    }

    pub(super) fn tool_window_animating(&self) -> bool {
        self.tools.window().is_some_and(|w| {
            w.split.animating()
                || w.groups.iter().any(|g| g.active_view().is_some_and(|v| v.viewer.is_animating()))
        })
    }

    /// The bottom area's view shows a loader (work going on).
    pub(super) fn tool_window_loading(&self) -> bool {
        self.tools.window().is_some_and(|w| {
            w.groups.iter().any(|g| g.active_view().is_some_and(|v| v.viewer.is_loading()))
        })
    }

    // ─── Layout ───

    fn tool_header_height(&self) -> f32 {
        (34.0 * self.scale).round()
    }

    /// The shown window's groups in the bottom area, following its split tree (one: the
    /// whole card), with the separators between them.
    pub(super) fn layout_tool_groups(&mut self) {
        let (b, scale) = (self.tools.bottom_bounds, self.scale);
        let gap = (SPLIT_GAP * scale).round();
        let palette = self.palette.clone();
        let Some(window) = self.tools.window_mut() else { return };
        let rects = window.split.layout(&window.tree, b, gap, scale, &palette);
        for (g, rect) in rects {
            if let Some(group) = window.groups.get_mut(g) {
                group.bounds = rect;
            }
        }
    }

    /// Where group `bounds`' view is drawn: under its header, inside the card's border.
    fn tool_group_view_bounds(&self, bounds: Rect) -> Rect {
        let header = self.tool_header_height();
        let inset = self.scale.round().max(1.0);
        Rect::new(
            bounds.min_x() + inset,
            bounds.min_y() + header + inset,
            (bounds.size.width - 2.0 * inset).max(0.0),
            (bounds.size.height - header - 2.0 * inset).max(0.0),
        )
    }

    /// Where the current group's view is drawn.
    #[cfg(feature = "snapshot")]
    fn tool_view_bounds(&self) -> Rect {
        let bounds = self.tools.window().map_or(self.tools.bottom_bounds, |w| w.group().bounds);
        self.tool_group_view_bounds(bounds)
    }

    /// The group of the shown window whose view is under `p`.
    fn tool_group_at(&self, p: Point) -> Option<usize> {
        let window = self.tools.window()?;
        window.groups.iter().position(|g| self.tool_group_view_bounds(g.bounds).contains(p))
    }

    /// What the bottom window's view under `p` says of it (its buttons' tooltips, a name
    /// cut short).
    pub(super) fn tool_view_tooltip(&self, p: Point) -> Option<ui_workbench::Tip> {
        if !self.tools.bottom_bounds.contains(p) {
            return None;
        }
        let g = self.tool_group_at(p)?;
        self.tools.window()?.groups[g].active_view()?.viewer.tooltip_at(p)
    }

    /// The active viewer of the group under `p`.
    fn tool_viewer_at(&mut self, p: Point) -> Option<&mut Box<dyn ui_workbench::Viewer>> {
        let g = self.tool_group_at(p)?;
        self.tools.window_mut()?.groups[g].active_view_mut().map(PanelView::viewer_mut)
    }

    // ─── Painting ───

    // ─── Input ───

    /// The comparison for the bottom window, in the editor (the last one it opened replaced).
    fn compare_above(&mut self, input: diff::CompareInput, title: &str) {
        let dir = input.right.0.parent().map(Path::to_path_buf).unwrap_or_default();
        let path = dir.join(format!("{title}.{}", diff::COMPARE_VIEW));
        let opened = workbench_model::ViewHost::compare(self, input, title);
        if let Some(old) = self.beside_compare.take().filter(|old| *old != path)
            && let Some(i) = self.tabs.iter().position(|t| t.doc.path() == Some(old.as_path()))
        {
            self.close_tab(i);
        }
        if opened {
            self.beside_compare = Some(path);
        }
        // The window keeps the keyboard (↑ ↓ through its list).
        self.focus = Focus::Panel;
    }

    /// The right-click menu of the bottom window's view at `p` (`None`: not over one, or it
    /// has none): its rows' ids made the app's ([`TOOL_VIEW_MENU`] + the view's id). Its
    /// group becomes current and the window gets the keyboard.
    pub fn tool_view_menu_at(&mut self, p: Point) -> Option<Vec<ui_workbench::MenuEntry>> {
        if !self.tool_window_shown() {
            return None;
        }
        let g = self.tool_group_at(p)?;
        self.focus_tool_group(g);
        let entries = self.tools.panel_viewer()?.context_menu(p)?;
        self.focus = Focus::Panel;
        Some(super::viewers::prefixed_menu(entries, TOOL_VIEW_MENU))
    }

    /// Where the bottom window's current view is drawn, when it's shown (snapshots point
    /// into it).
    #[cfg(feature = "snapshot")]
    pub fn tool_view_area(&self) -> Option<Rect> {
        self.tool_window_shown().then(|| self.tool_view_bounds())
    }

    /// A row of the bottom view's menu chosen: sent to the view, what it asks done.
    pub fn run_tool_view_menu(&mut self, text: &mut TextSystem, id: &str) -> bool {
        let used = self.tools.panel_viewer().is_some_and(|v| v.command(id));
        if let Some(action) = self.tools.panel_viewer().and_then(|v| v.take_action()) {
            match self.panel_action(text, action) {
                MouseResult::None | MouseResult::Redraw => {}
                request => self.request = Some(request),
            }
        }
        used
    }
}

/// What of window `w` is saved: its views that ask (the terminal's shells), in their
/// groups; the groups left without any go (from its tree too). `None` if none is saved.
fn saved_tool_window(w: &ToolWindow) -> Option<workbench_model::SavedToolWindow> {
    let mut tree = w.tree.clone();
    let mut views = Vec::new();
    let mut actives = Vec::new();
    let mut current = 0;
    let mut kept_groups = 0;
    for (g, group) in w.groups.iter().enumerate() {
        let kept: Vec<(usize, &PanelView)> =
            group.views.iter().enumerate().filter(|(_, v)| v.viewer.restorable()).collect();
        if kept.is_empty() {
            // Its leaf goes (the ones after it renumbered, as `kept_groups` counts).
            tree.remove(kept_groups);
            continue;
        }
        // The current group, or the last kept one before it.
        if g <= w.current {
            current = kept_groups;
        }
        actives.push(kept.iter().position(|(i, _)| *i == group.active).unwrap_or(0));
        views.extend(kept.into_iter().map(|(_, v)| workbench_model::SavedView {
            title: v.title.clone(),
            extension: v.extension.clone(),
            pinned: v.pinned,
            group: kept_groups,
            key: (v.key != v.title).then(|| v.key.clone()),
            memento: v.viewer.memento(),
        }));
        kept_groups += 1;
    }
    if views.is_empty() {
        return None;
    }
    let split = if kept_groups > 1 { tree.to_spec() } else { String::new() };
    let active = actives.get(current).copied().unwrap_or(0);
    let (group, actives) = if kept_groups > 1 { (current, actives) } else { (0, Vec::new()) };
    Some(workbench_model::SavedToolWindow {
        id: w.id.clone(),
        views,
        active,
        split,
        group,
        actives,
    })
}

/// The prefix of the bottom window's view's menu rows among the app's menu commands.
pub const TOOL_VIEW_MENU: &str = "tool-view-menu:";

#[cfg(test)]
#[path = "../../tests/unit/workbench/tool_windows.rs"]
mod tests;
