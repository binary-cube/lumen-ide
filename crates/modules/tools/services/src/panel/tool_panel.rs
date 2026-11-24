//! The window's panel as a side panel ([`ToolPanel`]): its project and text options, drawn
//! and polled, let go while parked, the mouse and the wheel, the keys and the text, its
//! commands, header, menus and the pages' events.

use std::sync::{Arc, PoisonError};

use kernel::Point;
use ui_components::control::{Control, Press};
use ui_components::{
    KeyReach, PanelButton, PanelContext, PanelMemento, ServiceState, ToolPanel, ToolPanelAction,
    ViewMenuEntry, ViewerCursor, ViewerKey, ViewerPaint,
};
use workbench_model::ProjectView;

use crate::tree::Key;

use super::{
    GAP, Hover, MenuTarget, ROW_HEIGHT, Reveal, SELECT_ALL, ServicesPanel, TREE_MIN, TREE_WIDTH,
    TabCommand, TabHit,
};

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
