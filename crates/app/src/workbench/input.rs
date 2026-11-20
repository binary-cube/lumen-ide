//! The mouse: click, drag, hover, scroll.

use super::*;

mod clicks;
mod hovering;

impl Workbench {
    /// Left click. Returns `Some(path)` if opening a file from the tree was requested.
    pub fn mouse_down(
        &mut self,
        text: &mut TextSystem,
        p: Point,
        shift: bool,
        alt: bool,
        primary: bool,
        clicks: u32,
    ) -> MouseResult {
        // A click outside the side panel having the keyboard: what it was typing in place
        // (a new entry's name) is given up.
        if let Focus::Side(dock) = self.focus
            && let Some(side) = self.tools.sides.iter_mut().find(|s| s.dock == dock)
            && !side.bounds.contains(p)
        {
            side.panel.focus_lost();
        }
        let result = self.mouse_down_inner(text, p, shift, alt, primary, clicks);
        // On a text field (any of them): the cursor goes where it was clicked, whatever the
        // field selected when it took the keyboard; a double click selects everything.
        if let Some(input) = self.active_input_mut().filter(|i| i.contains(p)) {
            if clicks >= 2 {
                input.select_all();
            } else {
                input.click(p.x);
            }
        }
        result
    }

    fn mouse_down_inner(
        &mut self,
        text: &mut TextSystem,
        p: Point,
        shift: bool,
        alt: bool,
        primary: bool,
        clicks: u32,
    ) -> MouseResult {
        self.drag_unit = None;
        self.tooltips.hide();
        // What goes when the mouse presses elsewhere goes (the editor's cards); a floating
        // window under the mouse (the hover card, a notification, the completion list)
        // takes the click on it, over everything (modals too).
        self.elsewhere(p, true);
        if let Some(o) = self.floating_at(p) {
            return o.slot().click(self, text, p, clicks);
        }
        // The overlay on top takes the click (wherever it falls).
        if let Some(o) = self.active_overlay() {
            return o.slot().click(self, text, p, clicks);
        }
        if let Some(result) = self.click_overlays(text, p, clicks) {
            return result;
        }
        let mods = ui_workbench::PressMods { shift, primary, alt };
        if let Some(result) = self.click_tool_window(text, p, clicks, mods) {
            return result;
        }
        if let Some(result) = self.click_chrome(text, p, clicks, mods) {
            return result;
        }
        self.click_editor(text, p, shift, alt, primary, clicks)
    }

    /// The mouse dragged with the button pressed: extends the selection and scrolls at the edges.
    pub fn mouse_drag(&mut self, text: &mut TextSystem, p: Point) -> bool {
        if let Some(changed) = self.drag_picker(p) {
            return changed;
        }
        // A window over the editor holding a press of its own (a thumb, a slider, a view's
        // drag): each says, the app names none.
        if let Some(changed) = Overlay::ALL.into_iter().find_map(|o| o.slot().drag(self, text, p)) {
            return changed;
        }
        if let Some(side) = self.tools.sides.iter_mut().find(|s| s.dragging) {
            return side.panel.drag(p);
        }
        if let Some(changed) = self.drag_tool_window(p) {
            return changed;
        }
        if let Some(moved) = self.editors.split.drag(&mut self.editors.tree, p) {
            if moved {
                self.layout();
            }
            return true;
        }
        if let Some(moved) = self.drag_preview_split(p) {
            return moved;
        }
        if let Some((i, start)) = self.editors.tab_press {
            let moved = (p.x - start.x).abs().max((p.y - start.y).abs());
            if self.editors.tab_drag.is_none() && moved < super::splits::DRAG_THRESHOLD * self.scale
            {
                return false;
            }
            let Some(tab) = self.tabs.get(i) else { return false };
            let title = tab.title();
            let (support, icons, palette) = (&self.support, &self.icons, &self.palette);
            let name = tab.icon_name();
            let icon =
                Some(self.file_icons.get(support, icons, palette, self.theme_generation, &name));
            let target = self.tab_drop_target(p);
            self.editors.tab_drag = Some(TabDrag { tab: i, title, icon, at: p, target });
            return true;
        }
        if self.viewer_drag {
            let changed = self
                .tabs
                .get_mut(self.active)
                .and_then(|t| t.viewer.as_mut())
                .is_some_and(|v| v.viewer.drag(p));
            self.take_viewer_action(text);
            return changed;
        }
        // A dock's separator held: its dock takes the size it's dragged to.
        if let Some(changed) = self.drag_divider(p) {
            return changed;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        if tab.view.scrollbar_drag(p, &tab.doc, &tab.folds) {
            return true;
        }
        let offset = tab.view.offset_at(p, &tab.doc, &tab.folds, text, &self.palette);
        // A redraw only when the selection or the scroll moved (not at each move within a
        // character).
        let before = (tab.doc.selections().clone(), tab.view.scroll_position());
        match &self.drag_unit {
            Some(DragUnit::Word { origin, word_chars }) => {
                let target = commands::word_range(&tab.doc, offset, word_chars);
                commands::drag_extend(&mut tab.doc, origin.clone(), target);
            }
            Some(DragUnit::Column { anchor }) => {
                let anchor = *anchor;
                let head = tab.view.column_at(p, &tab.doc, &tab.folds, text, &self.palette);
                // Only after the mouse left the initial position (otherwise it's an ⌥ + click).
                if head != anchor {
                    commands::column_select(&mut tab.doc, anchor, head);
                }
            }
            Some(DragUnit::Line { origin }) => {
                let target = commands::line_range(&tab.doc, offset);
                commands::drag_extend(&mut tab.doc, origin.clone(), target);
            }
            None => commands::drag_to(&mut tab.doc, offset),
        }
        tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
        before != (tab.doc.selections().clone(), tab.view.scroll_position())
    }

    /// A click on a strip's button: its tool window shown / hidden, or its command run (the
    /// app's own reach the app).
    pub(super) fn tool_button(&mut self, id: &str) -> MouseResult {
        if !self.toggle_tool(id) {
            return MouseResult::None;
        }
        self.request.take().unwrap_or(MouseResult::Redraw)
    }

    /// Shows / hides the project panel; without a project, asks for one to open.
    pub fn toggle_project_panel(&mut self) -> bool {
        if self.project.is_none() {
            self.request = Some(MouseResult::Command("project.open"));
            return true;
        }
        // The project tree's module panel (another left panel in its place: it comes back).
        self.toggle_side_panel(PROJECT_TOOL)
    }

    /// When something waiting for the mouse to rest appears (the code hover card, a
    /// tooltip): one redraw is scheduled for then, instead of frames meanwhile.
    pub fn hover_deadline(&self) -> Option<std::time::Instant> {
        // A tooltip, and what each window over the editor waits for (its slot says).
        let windows = Overlay::ALL.into_iter().filter_map(|o| o.slot().rest_deadline(self));
        self.tooltips.deadline().into_iter().chain(windows).min()
    }

    /// An animation (e.g. hover on a separator) is in progress: another frame is needed.
    pub fn animating(&self) -> bool {
        self.dividers_animating()
            || self.tool_window_animating()
            || self.tools.sides.iter().any(|s| s.panel.is_animating())
            // Every group's editors: another group's scroll strip shrinks back too.
            || all_tabs(&self.tabs, &self.editors.groups).any(|t| t.view.is_animating())
            || self
                .tabs
                .get(self.active)
                .and_then(|t| t.viewer.as_ref())
                .is_some_and(|v| v.viewer.is_animating() || v.split.splitter.is_animating())
            || self.tooltips.is_animating()
            || self.chrome.top_bar_reveal.animating()
            || self.editors.split.animating()
            || self.toolbars_animating()
            || Overlay::ALL.into_iter().any(|o| o.slot().animating(self))
    }

    /// The active view's loader turns (work going on): frames at a loader's pace.
    pub fn loading(&self) -> bool {
        self.tool_window_loading()
            || self
                .tabs
                .get(self.active)
                .and_then(|t| t.viewer.as_ref())
                .is_some_and(|v| v.viewer.is_loading())
            || Overlay::ALL.into_iter().any(|o| o.slot().is_open(self) && o.slot().loading(self))
            || self.toolbars_loading()
            || self.tools.sides.iter().any(|s| s.panel.is_loading())
    }

    /// The mouse button was released. `true` if a redraw is needed. What a view decides on
    /// a release (a diagram's member clicked: its code) is acted on now.
    pub fn mouse_up(&mut self, text: &mut TextSystem) -> bool {
        if let Some(changed) = self.release_picker() {
            return changed;
        }
        if let Some(changed) = Overlay::ALL.into_iter().find_map(|o| o.slot().release(self, text)) {
            return changed;
        }
        if let Some(side) = self.tools.sides.iter_mut().find(|s| s.dragging) {
            side.dragging = false;
            let changed = side.panel.release();
            // A drop: an entry moved; a click decided on its release (a row opened where
            // a press could also have started a drag): done as a click's would be.
            match side.panel.take_action() {
                Some(ui_workbench::ToolPanelAction::Move { from, into }) => {
                    self.move_entry(&from, &into);
                    return true;
                }
                Some(other) => {
                    self.side_action(text, other);
                    return true;
                }
                None => {}
            }
            return changed;
        }
        if self.release_tool_window() {
            return true;
        }
        if self.editors.split.end_drag() {
            return true;
        }
        if self.release_preview_split() {
            return true;
        }
        self.editors.tab_press = None;
        if let Some(drag) = self.editors.tab_drag.take() {
            self.drop_tab(&drag);
            return true;
        }
        if std::mem::take(&mut self.viewer_drag) {
            let changed = self
                .tabs
                .get_mut(self.active)
                .and_then(|t| t.viewer.as_mut())
                .is_some_and(|v| v.viewer.release());
            self.take_viewer_action(text);
            return changed;
        }
        self.drag_unit = None;
        let scrollbar = self.tabs.get_mut(self.active).is_some_and(|t| t.view.scrollbar_release());
        // The lane's thumb pressed on a mark and let go without moving: the mark's line.
        if let Some(tab) = self.tabs.get_mut(self.active)
            && let Some(offset) = tab.view.take_mark_press()
        {
            let offset = offset.min(tab.doc.buffer().len());
            commands::click(&mut tab.doc, offset, false, false);
            tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
            self.focus = Focus::Editor;
        }
        self.release_divider() | scrollbar
    }

    /// `p` is on the active tab's text editor (a file, not a view).
    fn editor_focusable_at(&self, p: Point) -> bool {
        self.tabs.get(self.active).is_some_and(|t| {
            t.view.bounds().contains(p)
                && t.doc.path().is_none_or(|path| !is_virtual(path))
                && t.viewer.as_ref().is_none_or(|v| v.editor_visible())
        })
    }

    /// Scroll on the panel under the cursor (physical pixels, positive = down / right).
    pub fn scroll(&mut self, at: Point, dx: f32, dy: f32) -> bool {
        // What goes when the wheel turns elsewhere goes (the hover card); a floating window
        // under the mouse scrolls itself, nothing under it does.
        self.elsewhere(at, false);
        if let Some(scrolled) = self.floating_at(at).and_then(|o| o.slot().scroll(self, dx, dy)) {
            return scrolled;
        }
        // The overlay on top scrolls (nothing behind a modal does; a popover lets it through).
        if let Some(scrolled) = self.active_overlay().and_then(|o| o.slot().scroll(self, dx, dy)) {
            return scrolled;
        }
        if let Some(scrolled) = self.scroll_tool_window(at, dx, dy) {
            return scrolled;
        }
        if self.home_visible() && self.editor_area.contains(at) {
            return self.home.picker.scroll_by(dy);
        }
        if let Some(side) = self.tools.side_at(at) {
            return side.panel.scroll_by(dx, dy);
        }
        if let Some(changed) = self.with_group_at(at, |this| this.scroll_editor(at, dx, dy)) {
            return changed;
        }
        self.scroll_editor(at, dx, dy)
    }

    /// Scroll in the current editor group (tabs, viewer, editor).
    pub(super) fn scroll_editor(&mut self, at: Point, dx: f32, dy: f32) -> bool {
        if self.tab_bar.contains(at) {
            // The vertical wheel also scrolls the tabs horizontally.
            let delta = if dx.abs() > dy.abs() { dx } else { dy };
            return self.tab_bar.scroll_by(delta);
        }
        // ⌘⌥ (Ctrl+Alt) + wheel zooms; ⌘ alone is for links (a stray wheel turn while
        // following a link must not zoom).
        let zoom = self.wheel_zoom;
        match self.tabs.get_mut(self.active) {
            Some(tab) => {
                if let Some(v) = &mut tab.viewer
                    && v.viewer_visible()
                    && v.bounds.contains(at)
                {
                    // ⌘⌥ (Ctrl+Alt) + scroll: zoom (images); otherwise scrolling.
                    if zoom {
                        return v.viewer.zoom((-dy * 0.004).exp(), at);
                    }
                    return v.viewer.scroll(dx, dy);
                }
                // ⌘⌥ (Ctrl+Alt) + scroll in the editor: zoom on the text.
                if zoom {
                    return self.zoom_by((-dy * 0.004).exp(), at);
                }
                tab.view.scroll_by(dx, dy, &tab.doc, &tab.folds)
            }
            None => false,
        }
    }

    pub fn line_height(&self) -> f32 {
        self.tabs.get(self.active).map_or(20.0 * self.scale, |t| t.view.line_height())
    }

    /// Where the IME window should be shown (next to the cursor), if the editor has focus.
    pub fn caret_rect(&mut self, text: &mut TextSystem) -> Option<Rect> {
        // A field has the keyboard (a modal's, a search's): its cursor, not the editor's.
        if let Some(input) = self.active_input() {
            return input.caret_area();
        }
        if self.focus != Focus::Editor || self.keyboard_in_overlay() {
            return None;
        }
        let tab = self.tabs.get_mut(self.active)?;
        // A view in the text's place (the Task Manager, an image): no text caret to tell the
        // system of — else its input source's popup (macOS: "US") shows over the view.
        if tab.viewer.as_ref().is_some_and(|v| v.kind == ui_workbench::ViewerKind::Replace) {
            return None;
        }
        Some(tab.view.caret_rect(&tab.doc, &tab.folds, text, &self.palette))
    }

    // ─── Painting ───
}

impl Workbench {
    /// The least width the side panel on `dock` may be dragged to: what its header needs
    /// (its title and buttons side by side; 0 before it's drawn).
    pub(super) fn header_min(&self, dock: ui_workbench::Dock) -> f32 {
        self.tools.side_on(dock).map_or(0.0, |s| s.header.min_width())
    }
}
