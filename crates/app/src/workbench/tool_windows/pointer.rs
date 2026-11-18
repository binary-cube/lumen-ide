//! The mouse and keys over a tool window, and what its views ask for.

use super::*;

impl Workbench {
    /// A click in the bottom window: a separator, a header, or a view (its group becomes
    /// current). `None` if it's not there.
    pub(in crate::workbench) fn click_tool_window(
        &mut self,
        text: &mut TextSystem,
        p: Point,
        clicks: u32,
        mods: ui_workbench::PressMods,
    ) -> Option<MouseResult> {
        if !self.tool_window_shown() || !self.tools.bottom_bounds.contains(p) {
            return None;
        }
        self.focus = Focus::Panel;
        // A separator between groups: dragged; a double click shares the space equally.
        if let Some(window) = self.tools.window_mut()
            && let Some(i) = window.split.begin_drag(p)
        {
            if clicks == 2 {
                window.split.end_drag();
                if let Some(divider) = window.split.dividers.get(i).cloned() {
                    window.tree.equalize(&divider);
                }
                self.layout();
                return Some(MouseResult::Redraw);
            }
            return Some(MouseResult::StartDrag);
        }
        if let Some(hit) = self.tool_hit_at(p) {
            match hit {
                ToolHit::Hide => self.set_bottom(None),
                ToolHit::More => {
                    let at = self.tools.hits.iter().find(|(_, h)| *h == ToolHit::More);
                    if let (Some((at, _)), Some(id)) = (at.copied(), self.tools.bottom.clone()) {
                        self.open_window_menu(&id, at);
                    }
                }
                ToolHit::Half => self.toggle_bottom_size(BottomSize::Half),
                ToolHit::Full => self.toggle_bottom_size(BottomSize::Full),
                ToolHit::New(g) => {
                    self.focus_tool_group(g);
                    if let Some(command) = self.tool_new_command() {
                        self.run_tool_command(command_model::CommandId(command));
                    }
                }
                // A double click on a tab renames it.
                ToolHit::View(g, i) if clicks == 2 => {
                    self.focus_tool_group(g);
                    self.ask_tool_view_rename(i);
                }
                ToolHit::View(g, i) => {
                    self.focus_tool_group(g);
                    if let Some(window) = self.tools.window_mut() {
                        let group = window.group_mut();
                        group.active = i.min(group.views.len().saturating_sub(1));
                    }
                    // The tab can be dragged to another place among them.
                    if self.press_tool_tab(g, i, p) {
                        self.layout();
                        return Some(MouseResult::StartDrag);
                    }
                }
                ToolHit::CloseView(g, i) => self.close_tool_views(g, |j, _| j == i),
            }
            self.layout();
            return Some(MouseResult::Redraw);
        }
        let Some(g) = self.tool_group_at(p) else { return Some(MouseResult::None) };
        self.focus_tool_group(g);
        // ⌘ / Ctrl + click on a link opens it; else a press (a selection starts).
        let link_mode = self.link_mode;
        let link =
            if link_mode { self.tools.panel_viewer().and_then(|v| v.link_at(p)) } else { None };
        if let Some(link) = link {
            return Some(self.panel_action(text, link));
        }
        let action = self.tools.panel_viewer().and_then(|v| v.press_with(p, clicks, mods));
        self.tools.panel_dragging = true;
        Some(match action {
            Some(action) => self.panel_action(text, action),
            None => MouseResult::StartDrag,
        })
    }

    /// The mouse dragged after a press in the bottom window: a tab, a separator, or the
    /// view's own drag (its splitters, a selection).
    pub(in crate::workbench) fn drag_tool_window(&mut self, p: Point) -> Option<bool> {
        if self.tools.tab_drag.is_some() {
            return Some(self.drag_tool_tab(p));
        }
        if let Some(window) = self.tools.window_mut()
            && let Some(moved) = window.split.drag(&mut window.tree, p)
        {
            if moved {
                self.layout();
            }
            return Some(true);
        }
        if !self.tools.panel_dragging {
            return None;
        }
        Some(self.tools.panel_viewer().is_some_and(|v| v.drag(p)))
    }

    pub(in crate::workbench) fn release_tool_window(&mut self) -> bool {
        // A tab dropped: in its strip, it's already in its place (moved while dragged);
        // elsewhere, where it was dropped.
        if let Some(drag) = self.tools.tab_drag.take() {
            if drag.state == TabDragState::Moving
                && !drag.in_strip
                && let Some(target) = drag.target
            {
                self.drop_tool_tab(drag.view, target);
            }
            return drag.state != TabDragState::Pressed;
        }
        if self.tools.window_mut().is_some_and(|w| w.split.end_drag()) {
            return true;
        }
        if !std::mem::take(&mut self.tools.panel_dragging) {
            return false;
        }
        self.tools.panel_viewer().is_some_and(|v| v.release())
    }

    /// The mouse over the bottom window: `None` if it's not there.
    pub(in crate::workbench) fn hover_tool_window(
        &mut self,
        p: Point,
    ) -> Option<(bool, CursorIcon)> {
        let inside = self.tool_window_shown() && self.tools.bottom_bounds.contains(p);
        let (moved, over) =
            self.tools.window_mut().map_or((false, None), |w| w.split.hover(inside.then_some(p)));
        if !inside {
            // Left it: nothing lit there any more (the rest of the window takes the hover).
            self.tools.hover = None;
            return None;
        }
        if let Some(icon) = over {
            return Some((moved, icon));
        }
        let hit = self.tool_hit_at(p);
        let changed = moved || hit != self.tools.hover;
        self.tools.hover = hit;
        if hit.is_some() {
            return Some((changed, CursorIcon::Pointer));
        }
        let link_mode = self.link_mode;
        let Some(viewer) = self.tool_viewer_at(p) else {
            return Some((changed, CursorIcon::Default));
        };
        // ⌘ / Ctrl held: the link under the mouse shown as one.
        let redraw = viewer.hover(p) | viewer.link_hover(link_mode.then_some(p));
        let icon = match viewer.cursor_at(p) {
            Some(cursor) => super::cursor_icon(cursor),
            None => CursorIcon::Default,
        };
        Some((changed || redraw, icon))
    }

    /// The wheel over the bottom window: the view under it scrolls.
    pub(in crate::workbench) fn scroll_tool_window(
        &mut self,
        at: Point,
        dx: f32,
        dy: f32,
    ) -> Option<bool> {
        if !self.tool_window_shown() || !self.tools.bottom_bounds.contains(at) {
            return None;
        }
        Some(self.tool_viewer_at(at).is_some_and(|v| v.scroll(dx, dy)))
    }

    /// A key for the bottom window's view (it has the keyboard). `None` if it doesn't.
    pub(in crate::workbench) fn tool_window_key(
        &mut self,
        text: &mut TextSystem,
        key: ui_workbench::ViewerKey,
        reach: ui_workbench::KeyReach,
    ) -> Option<bool> {
        if self.focus != Focus::Panel || !self.tool_window_shown() || self.keyboard_in_overlay() {
            return None;
        }
        let used = self.tools.panel_viewer().is_some_and(|v| v.edit_key_with(key, reach));
        self.take_panel_action(text);
        Some(used)
    }

    /// What a view of the bottom window asked for: as a tab's view, but a comparison opens
    /// in the editor above (the last one replaced), and "close" closes the view.
    pub(in crate::workbench) fn panel_action(
        &mut self,
        text: &mut TextSystem,
        action: ui_workbench::ViewerAction,
    ) -> MouseResult {
        use ui_workbench::ViewerAction;
        match action {
            ViewerAction::CompareBeside(input, title) => {
                self.compare_above(*input, &title);
                MouseResult::Redraw
            }
            // The view asked to close: its tab.
            ViewerAction::Close => {
                let active = self.tools.window().map_or(0, |w| w.group().active);
                self.close_tool_view(active);
                MouseResult::Redraw
            }
            other => self.viewer_action(text, other),
        }
    }

    /// What the views' background work prepared (the bottom ones', the active tab's), done.
    pub fn run_panel_actions(&mut self, text: &mut TextSystem) {
        self.take_viewer_action(text);
        for action in std::mem::take(&mut self.panel_actions) {
            if let MouseResult::ViewerRequest(request) = self.panel_action(text, action) {
                self.request = Some(MouseResult::ViewerRequest(request));
            }
        }
    }

    /// Takes what the bottom view's keys produced.
    pub(in crate::workbench) fn take_panel_action(&mut self, text: &mut TextSystem) {
        if let Some(action) = self.tools.panel_viewer().and_then(|v| v.take_action())
            && let MouseResult::ViewerRequest(request) = self.panel_action(text, action)
        {
            self.request = Some(MouseResult::ViewerRequest(request));
        }
    }
}
