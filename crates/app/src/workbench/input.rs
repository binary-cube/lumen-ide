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
        self.param_hint.hide();
        self.drag_unit = None;
        self.tooltips.hide();
        // A click on the code hover card stays there (the editor below doesn't move); on
        // its scrollbar, the content follows the thumb (a drag goes on).
        if self.code_hover.scrollbar_press(p) {
            return MouseResult::StartDrag;
        }
        if self.code_hover.contains(p) {
            return MouseResult::None;
        }
        self.code_hover.hide();
        // A notification's card takes the click on it, over everything (modals too).
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
        if let Some(changed) = self.drag_menu_drawer(p) {
            return changed;
        }
        if self.completion.scrollbar_held() {
            return self.completion.scrollbar_drag(p);
        }
        if self.code_hover.scrollbar_held() {
            return self.code_hover.scrollbar_drag(p);
        }
        if self.dialogs.dialog.scrollbar_held() {
            return self.dialogs.dialog.scrollbar_drag(p);
        }
        // A scrollbar of the settings, held: the navigation or the content follows.
        if self.settings.scrollbar_held() {
            return self.settings.drag_scrollbar(p);
        }
        // A slider in the settings, held: each stop it reaches is chosen (saved, shown).
        if self.settings.slider_held() {
            if let Some(SettingsAction::Choose { block, key, value }) = self.settings.drag(p) {
                self.settings_choose(block, key, value);
                self.settings_model.blocks = self.settings_blocks();
                return true;
            }
            return false;
        }
        if let Some(side) = self.tools.sides.iter_mut().find(|s| s.dragging) {
            return side.panel.drag(p);
        }
        if let Some(changed) = self.drag_modal_view(p) {
            return changed;
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
        if let Some(center) = self.tools.terminal_splitter.drag_to(p) {
            // Dragged: its own size again, where it's dragged to.
            self.tools.bottom_size = tool_windows::BottomSize::Kept;
            // The terminal spans from the separator to the bottom (above the status bar).
            let s = self.scale;
            let bottom = self.size.height as f32 - StatusBar::height(s);
            let height = (bottom - center - SPLITTER_WIDTH * s / 2.0) / s;
            let max = (bottom / s * 0.75).max(TERMINAL_MIN_HEIGHT);
            let height = height.clamp(TERMINAL_MIN_HEIGHT, max).round();
            let sizes = &self.data.sizes;
            if sizes.get(workbench_model::sizes::DOCK_BOTTOM) != Some(height) {
                sizes.set(workbench_model::sizes::DOCK_BOTTOM, height);
                self.layout();
            }
            return true;
        }
        if let Some(center) = self.tools.side_splitter.drag_to(p) {
            // The right panel: from the separator to the right toolbar.
            let right =
                self.tools.side_on(ui_workbench::Dock::Right).map_or(0.0, |s| s.bounds.max_x());
            let width = (right - center - SPLITTER_WIDTH * self.scale / 2.0) / self.scale;
            // Never narrower than its header needs (its title and buttons side by side).
            let least = SIDE_PANEL_MIN.max(self.header_min(ui_workbench::Dock::Right));
            let max = (self.size.width as f32 / self.scale * 0.5).max(least);
            let width = width.clamp(least, max).round();
            let sizes = &self.data.sizes;
            if sizes.get(workbench_model::sizes::DOCK_RIGHT) != Some(width) {
                sizes.set(workbench_model::sizes::DOCK_RIGHT, width);
                self.layout();
            }
            return true;
        }
        if let Some(center) = self.tools.splitter.drag_to(p) {
            // The panel starts after the tool strip, not at the window's edge.
            let left = self.tools.sidebar.min_x();
            let width = (center - left - SPLITTER_WIDTH * self.scale / 2.0) / self.scale;
            let least = TREE_MIN_WIDTH.max(self.header_min(ui_workbench::Dock::Left));
            let max = (self.size.width as f32 / self.scale * 0.6).max(least);
            let width = width.clamp(least, max).round();
            let sizes = &self.data.sizes;
            if sizes.get(workbench_model::sizes::DOCK_LEFT) != Some(width) {
                sizes.set(workbench_model::sizes::DOCK_LEFT, width);
                self.layout();
            }
            return true;
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
        // A card due but waited for from a language server: its grace's end instead (no
        // frames meanwhile; `served_deadline` has it).
        let card = self
            .code_hover
            .deadline()
            .filter(|at| *at > std::time::Instant::now() || !self.served_hover_waiting());
        // A category of the menu drawer the mouse rests on: shown then.
        let drawer = self.menu_drawer.as_ref().and_then(|d| d.deadline());
        [card, self.tooltips.deadline(), drawer].into_iter().flatten().min()
    }

    /// An animation (e.g. hover on a separator) is in progress: another frame is needed.
    pub fn animating(&self) -> bool {
        self.tools.splitter.is_animating()
            || self.tools.side_splitter.is_animating()
            || self.tools.terminal_splitter.is_animating()
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
            || self.code_hover.is_animating()
            || self.param_hint.is_animating()
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
            || self.modal_view_loading()
            || self.toolbars_loading()
            || self.tools.sides.iter().any(|s| s.panel.is_loading())
    }

    /// The mouse button was released. `true` if a redraw is needed. What a view decides on
    /// a release (a diagram's member clicked: its code) is acted on now.
    pub fn mouse_up(&mut self, text: &mut TextSystem) -> bool {
        if let Some(changed) = self.release_picker() {
            return changed;
        }
        if self.settings.release()
            || self.release_menu_drawer()
            || self.completion.scrollbar_release()
            || self.code_hover.scrollbar_release()
            || self.dialogs.dialog.scrollbar_release()
        {
            return true;
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
        if let Some(changed) = self.release_modal_view() {
            self.take_modal_action(text);
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
        self.tools.splitter.end_drag()
            | self.tools.side_splitter.end_drag()
            | self.tools.terminal_splitter.end_drag()
            | scrollbar
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
        // The card under the mouse scrolls its explanation (the editor behind it doesn't).
        if self.code_hover.contains(at) {
            return self.code_hover.scroll_by(dy);
        }
        self.code_hover.hide();
        // Nothing under a notification scrolls.
        if let Some(scrolled) = self.floating_at(at).and_then(|o| o.slot().scroll(self, dx, dy)) {
            return scrolled;
        }
        // The overlay on top scrolls (nothing behind a modal does; a popover lets it through).
        if let Some(scrolled) = self.active_overlay().and_then(|o| o.slot().scroll(self, dx, dy)) {
            return scrolled;
        }
        // The completion list under the mouse scrolls (the editor behind it doesn't).
        if self.completion.contains(at) {
            return self.completion.scroll_by(dy);
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
