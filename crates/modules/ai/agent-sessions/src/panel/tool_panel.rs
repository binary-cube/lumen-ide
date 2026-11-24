//! The panel's answers to the app: reading when shown, painting, the pointer, the keys,
//! the header's buttons, the menus and their commands.

use std::time::Instant;

use i18n::t;
use kernel::{Point, Rect};
use ui_components::control::Control;
use ui_components::ds::Ctx;
use ui_components::{
    KeyReach, PanelButton, PanelContext, ToolPanel, ToolPanelAction, ViewMenuEntry, ViewerCursor,
    ViewerKey, ViewerPaint,
};

use crate::favorites::Label;
use crate::folders::Folder;

use super::{
    AGENTS_LIST, CHOOSE, COPY, DONE, DropOn, FAVORITE, FOLDER_DELETE, FOLDER_RENAME, LABEL, MORE,
    NEW, NEW_FOLDER, NEW_WITH, ONLY_FAVORITES, OPEN_CHAT, OPEN_TERMINAL, PIN, PRESSED_ROW, REFRESH,
    RENAME, RENAME_FIELD, Row, SEARCH_FIELD, SessionsPanel, UNREAD,
};

impl ToolPanel for SessionsPanel {
    fn update(&mut self, _ctx: &PanelContext<'_>) {}

    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(view) = data.downcast_ref::<workbench_model::ProjectView>() else { return };
        self.project = view.project;
        let root = view.queries.as_ref().map(|q| q.root());
        if root != self.root {
            self.root = root;
            self.sessions.clear();
            self.more = false;
            self.read_at = None;
            self.reading = None;
        }
    }

    /// A read come in; a session the follower saw change (found, renamed): read again now.
    fn poll(&mut self) -> bool {
        let changes = crate::follow::changes();
        if changes != self.followed {
            self.followed = changes;
            self.read();
        }
        self.take_read()
    }

    fn is_loading(&self) -> bool {
        self.reading.is_some() && self.read_at.is_none()
    }

    /// Hidden: the read going on given up (asked again when shown).
    fn parked(&mut self) {
        self.reading = None;
    }

    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        self.paint_panel(&mut ctx);
        // What the scrollbar wants of the frames (its fade), for the questions before the next.
        self.with_controls(|c, parts| c.settle(parts));
        // The agents' list, over everything, under its field.
        if let Some(anchor) = self.filter_field
            && self.filter_list.is_open()
        {
            let ds = Ctx::new(ctx.palette, ctx.scale);
            let bounds = ctx.bounds;
            self.filter_list.paint(ctx.scene, ctx.text, ctx.icons, &ds, (anchor, bounds), "");
        } else {
            self.filter_list.close();
        }
    }

    fn scroll(&mut self, dy: f32) -> bool {
        // The agents' list open takes the wheel (whether or not the mouse reached it yet).
        if self.filter_list.is_open() {
            return self.with_controls(|c, parts| c.wheel(parts, 0.0, dy)).is_some_and(|(_, m)| m);
        }
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ToolPanelAction> {
        // The parts first: the agents' list open (a row, or closed by a click outside), the
        // scrollbar, the renamed row's field (the caret moved), the search (the keys to it).
        // A press beside them takes the keys from the search.
        match self.with_controls(|c, parts| c.press(parts, p, 1)) {
            Some((AGENTS_LIST, _)) => {
                let event = self.list_event.take();
                self.filter_list_event(event);
                return None;
            }
            // Pressed elsewhere than in the renamed row: the name's given (its × pressed:
            // searched again).
            Some((SEARCH_FIELD, _)) => {
                self.finish_rename();
                self.follow(SEARCH_FIELD);
                return None;
            }
            Some(_) => return None,
            None => {}
        }
        // Renaming: a click elsewhere gives the name.
        self.finish_rename();
        // The agents' filter: its list, under it.
        if let Some(anchor) = self.filter_field.filter(|r| r.contains(p)) {
            if anchor.size.width > 0.0 {
                self.open_agent_filter();
            }
            return None;
        }
        if self.more_rect.is_some_and(|r| r.contains(p)) {
            self.limit = self.limit.saturating_add(MORE);
            self.read();
            return None;
        }
        // A row's star: added to the favorites or taken off (the session isn't opened).
        if let Some(star) = self.star_at(p) {
            self.toggle_star(star);
            return None;
        }
        // A row's "⋮": its menu, under it.
        if let Some(i) = self.more_at(p) {
            let anchor = self.more_button.map(|(r, _)| r).unwrap_or_default();
            let entries = self.session_menu(i)?;
            return Some(ToolPanelAction::MenuUnder { anchor, entries });
        }
        // A folder: opened or closed.
        if let Some(f) = self.folder_at(p) {
            if let (Some(project), Some(folder)) = (self.project, self.folders.get(f)) {
                crate::folders::toggle_open(project, &folder.id);
            }
            return None;
        }
        // A session: opened on letting go (unless it's dragged into a folder meanwhile).
        let i = self.row_at(p)?;
        self.press = Some((i, p));
        self.controls.hold(PRESSED_ROW);
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        match self.with_controls(|c, parts| c.drag(parts, p)) {
            // A session pressed: dragged once it's moved far enough.
            Some((PRESSED_ROW, false)) => return false,
            Some((PRESSED_ROW, true)) => {}
            Some((_, moved)) => return moved,
            None => return false,
        }
        // Onto a folder or one of its sessions: into it; elsewhere in the list: out of any.
        let into = |f: &Folder| DropOn::Folder(f.id.clone());
        let drop = match self.row_index(p).and_then(|n| self.rows.get(n)) {
            Some(Row::Folder(f)) => self.folders.get(*f).map(into),
            Some(Row::Session { i, nested: true }) => {
                let folder = self.marked.get(*i).cloned().flatten().and_then(|m| m.folder);
                self.folders.iter().find(|f| Some(&f.id) == folder.as_ref()).map(into)
            }
            _ => self.list.contains(p).then_some(DropOn::Loose),
        };
        self.drop_on = drop;
        true
    }

    /// Let go: a session dragged goes where it was dropped; one pressed and not moved is
    /// opened (its terminal asked for, as a click's would be).
    fn release(&mut self) -> bool {
        match self.with_controls(|c, parts| c.release(parts)) {
            Some((PRESSED_ROW, _)) => {}
            Some((_, changed)) => return changed,
            None => return false,
        }
        let Some((i, _)) = self.press.take() else { return false };
        let dropped = self.drop_on.take();
        if self.dragging.take().is_some() {
            if let (Some(project), Some(session), Some(drop)) =
                (self.project, self.sessions.get(i), dropped)
            {
                let folder = match &drop {
                    DropOn::Folder(id) => Some(id.as_str()),
                    DropOn::Loose => None,
                };
                crate::folders::move_to(project, session.agent, &session.id, folder);
            }
            return true;
        }
        self.resume(i, true);
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let mut changed = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        // The agents' list open: the rows under it aren't hovered.
        if self.filter_list.is_open() {
            return changed;
        }
        let folder = self.folder_at(p);
        changed |= folder != self.hovered_folder;
        self.hovered_folder = folder;
        let row = self.row_at(p);
        let star = self.star_at(p);
        let more = self.more_at(p).is_some();
        let filter = self.filter_field.is_some_and(|r| r.contains(p));
        changed |= filter != self.hover_filter;
        self.hover_filter = filter;
        changed |= row != self.hovered || star != self.hover_star || more != self.hover_more;
        (self.hovered, self.hover_star, self.hover_more) = (row, star, more);
        changed
    }

    /// Clicked into (or a session renamed), it takes the keys (its rows stay the mouse's).
    fn takes_keys(&self) -> bool {
        self.typing() || self.renaming.is_some() || self.filter_list.is_open()
    }

    fn wants_text(&self) -> bool {
        self.typing() || self.renaming.is_some() || self.filter_list.searches()
    }

    /// To the field with the keys (the renamed row's, the search), the agents' list open or
    /// not (what's typed isn't the list's).
    fn text_input(&mut self, typed: &str) -> bool {
        let Some(part) = self.with_controls(|c, parts| c.text(parts, typed)) else {
            return false;
        };
        self.follow(part);
        true
    }

    /// What the field with the keys has selected (lent read-only as the bare field, whose
    /// selection reads as its part's).
    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| match i {
            RENAME_FIELD => self.renaming.as_ref().map(|r| &r.input as &dyn Control),
            SEARCH_FIELD => Some(&self.query as &dyn Control),
            _ => None,
        })
    }

    /// To the part with the keys: the agents' list open, else the renamed row's field or
    /// the search.
    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(part) = self.with_controls(|c, parts| c.key(parts, key, reach)) else {
            return false;
        };
        self.follow(part);
        true
    }

    fn focus_lost(&mut self) {
        self.with_controls(|c, parts| c.focus(parts, None));
        self.finish_rename();
    }

    fn clickable(&self, p: Point) -> bool {
        self.row_at(p).is_some()
            || self.more_at(p).is_some()
            || self.star_at(p).is_some()
            || self.more_rect.is_some_and(|r| r.contains(p))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // The agents' list open: its own cursor, never what's under it.
        if self.filter_list.is_open() {
            return Some(self.filter_list.cursor_at(p).unwrap_or(ViewerCursor::Arrow));
        }
        if self.field.contains(p) {
            return Control::cursor(&self.query, p);
        }
        if self.renaming.as_ref().is_some_and(|n| n.field.contains(p)) {
            return Some(ViewerCursor::Text);
        }
        self.clickable(p).then_some(ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        if let Some((rect, _)) = self.more_button.filter(|(r, _)| r.contains(p)) {
            return Some(ui_components::tooltip::Tip::new(rect, t("agents.more_actions")));
        }
        // A star says what a click does.
        if let Some((rect, star)) = self.stars.iter().find(|(r, _)| r.contains(p)) {
            let key = if self.starred(*star) { "agents.unfavorite" } else { "agents.favorite" };
            return Some(ui_components::tooltip::Tip::new(*rect, t(key)));
        }
        let n = self.row_index(p)?;
        let label = match self.rows.get(n)? {
            Row::Session { i, .. } => self.sessions.get(*i)?.title.clone(),
            Row::Folder(f) => self.folders.get(*f)?.name.clone(),
            Row::Missing(_) => return None,
        };
        let rect = Rect::new(self.list.min_x(), self.row_top(n), self.list.size.width, self.row_h);
        Some(ui_components::tooltip::Tip::new(rect, label))
    }

    fn header(&self) -> Option<Vec<PanelButton>> {
        let button = |id, icon, key: &str| PanelButton { id, icon, tooltip: t(key), group: 0 };
        let mut buttons = vec![button(REFRESH, "refresh", "agents.refresh")];
        if self.project.is_some() {
            buttons.insert(0, button(NEW_FOLDER, "folder-plus", "agents.new_folder"));
        }
        // The filter: a full star while only the favorites are listed.
        let (icon, key) = if self.only_favorites {
            (crate::FAVORITE_ON_ICON, "agents.all_sessions")
        } else {
            (crate::FAVORITE_ICON, "agents.only_favorites")
        };
        buttons.insert(0, button(ONLY_FAVORITES, icon, key));
        let offered = match &self.agents {
            Some(agents) => !agents.is_empty(),
            None => agents::any(|_| true),
        };
        if self.root.is_some() && offered {
            buttons.insert(0, button(CHOOSE, "chevron-down", "agents.choose"));
            buttons.insert(0, button(NEW, "plus", "agents.new"));
        }
        Some(buttons)
    }

    fn header_action(&mut self, id: &str) -> Option<ToolPanelAction> {
        match id {
            ONLY_FAVORITES => {
                self.only_favorites = !self.only_favorites;
                self.shown_for = None;
                self.scroll = 0.0;
                None
            }
            REFRESH => {
                self.read_at = None;
                self.read();
                None
            }
            // The agent chosen starts; none installed: which one, from the list.
            NEW => match self.chosen() {
                Some(agent) => {
                    self.start(&agent);
                    self.action.take()
                }
                None => Some(ToolPanelAction::Menu(self.agents_menu())),
            },
            CHOOSE => Some(ToolPanelAction::Menu(self.agents_menu())),
            NEW_FOLDER => {
                self.new_folder(|_, _| {});
                None
            }
            _ => None,
        }
    }

    /// A session's own menu: open it as a conversation (when they're shown) or in a terminal.
    fn menu_at(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        // A missing favorite's: taken off the list.
        if let Some(n) = self.missing_at(p) {
            self.menu_favorite = Some(self.missing[n].clone());
            self.menu_on = None;
            let off = ViewMenuEntry::item(FAVORITE, t("agents.unfavorite")).with_icon("x");
            return Some(vec![off]);
        }
        // A folder's: renamed, deleted; a new one beside it.
        if let Some(f) = self.folder_at(p) {
            self.menu_folder = self.folders.get(f).map(|f| f.id.clone());
            self.menu_on = None;
            return Some(vec![
                ViewMenuEntry::item(FOLDER_RENAME, t("agents.rename_folder"))
                    .with_icon("text-cursor"),
                ViewMenuEntry::item(FOLDER_DELETE, t("agents.delete_folder")).with_icon("trash"),
                ViewMenuEntry::Separator,
                ViewMenuEntry::item(NEW_FOLDER, t("agents.new_folder")).with_icon("folder-plus"),
            ]);
        }
        // Below the rows: a new folder.
        let Some(i) = self.row_at(p) else {
            return (self.project.is_some() && self.list.contains(p)).then(|| {
                vec![
                    ViewMenuEntry::item(NEW_FOLDER, t("agents.new_folder"))
                        .with_icon("folder-plus"),
                ]
            });
        };
        self.session_menu(i)
    }

    /// An edit command for the field with the keys (⌘A, ⌘X, ⌘Z, ⇧⌘Z): the renamed row's,
    /// the search while it's typed in; the agents' list open takes none (they go on to it).
    fn command(&mut self, id: &str) -> bool {
        let listed = self.filter_list.is_open();
        let taken = self.with_controls(|c, parts| {
            c.command(parts, id).or_else(|| {
                let under = c.focused().filter(|_| listed)?;
                parts.get_mut(under)?.command(id).then_some(under)
            })
        });
        let Some(part) = taken else { return false };
        self.follow(part);
        true
    }

    fn menu_command(&mut self, id: &str) -> bool {
        if self.folder_command(id) {
            return true;
        }
        // A session's marks, its rename, what's copied of it.
        if let Some(what) = id.strip_prefix(COPY) {
            self.copy(what);
            return true;
        }
        if id == RENAME {
            if let Some(i) = self.menu_session() {
                self.start_rename(i);
            }
            return true;
        }
        if id == PIN || id == DONE || id == UNREAD || id.starts_with(LABEL) {
            let (Some(project), Some((agent, session))) = (self.project, self.menu_on.clone())
            else {
                return false;
            };
            let label = id.strip_prefix(LABEL).map(Label::from_key);
            crate::favorites::mark(project, agent, &session, |m| match label {
                Some(label) => m.label = label,
                None if id == PIN => m.pinned = !m.pinned,
                None if id == DONE => m.done = !m.done,
                None => m.unread = !m.unread,
            });
            return true;
        }
        // Added to the favorites, or taken off them.
        if id == FAVORITE {
            let (Some(project), Some(f)) = (self.project, self.menu_favorite.take()) else {
                return false;
            };
            crate::favorites::toggle(project, &f.agent, &f.id, &f.title);
            return true;
        }
        if let Some((agent, session)) =
            self.menu_on.take().filter(|_| id == OPEN_CHAT || id == OPEN_TERMINAL)
        {
            let at = self.sessions.iter().position(|s| s.agent == agent && s.id == session);
            if let Some(i) = at {
                self.resume(i, id == OPEN_TERMINAL);
            }
            return true;
        }
        let Some(agent) = id.strip_prefix(NEW_WITH) else { return false };
        let Some(agent) = self.agents().into_iter().find(|a| a.id() == agent && a.installed())
        else {
            return false;
        };
        self.start(&agent);
        true
    }

    fn take_action(&mut self) -> Option<ToolPanelAction> {
        self.action.take()
    }

    /// Its scrollbar fading: a frame when it next changes.
    fn frame_deadline(&self) -> Option<Instant> {
        self.controls.next_frame()
    }

    fn purge(&mut self) {
        self.read_at = None;
    }
}
