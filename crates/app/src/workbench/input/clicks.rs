//! A press in the window: its overlays, its chrome, the editor.

use super::*;

impl Workbench {
    /// A click on the completion list, the empty screen, the top / status bar (no overlay
    /// is open). `None`: not handled here.
    pub(super) fn click_overlays(
        &mut self,
        text: &mut TextSystem,
        p: Point,
        clicks: u32,
    ) -> Option<MouseResult> {
        // The hidden top bar, come over the content (full screen): its buttons first.
        if self.chrome.top_bar_hides()
            && let Some(result) = self.click_top_bar(p, clicks)
        {
            return Some(result);
        }
        if self.completion.is_open() {
            // Its scrollbar: the list follows the thumb (a drag goes on).
            if self.completion.scrollbar_press(p) {
                return Some(MouseResult::StartDrag);
            }
            if self.completion.contains(p) {
                self.completion.click(p);
                return Some(if self.accept_completion(text) {
                    MouseResult::Redraw
                } else {
                    MouseResult::None
                });
            }
            self.completion.close();
        }
        // The empty screen: a click there gives it the keyboard (its search field), taking it
        // from the project panel's file search.
        if self.home_visible() && self.editor_area.contains(p) {
            self.focus = Focus::Editor;
            if let Some(action) = self.home.picker.hit(p) {
                return Some(self.picker_action(action, false));
            }
            // A press on a project or a group: a click or a drag, known on release.
            if self.home.picker.pressed() {
                return Some(MouseResult::StartDrag);
            }
            return Some(MouseResult::Redraw);
        }
        if let Some(result) = self.click_top_bar(p, clicks) {
            return Some(result);
        }
        if let Some(action) = self.status.hit(p) {
            if action == crate::commands::LANGUAGE_LEVEL.0 && self.open_level_picker() {
                return Some(MouseResult::Redraw);
            }
            // The `…`: what didn't fit, as rows doing what they do in the bar.
            // The resources: pinned (their pills in the editor's corner) or unpinned, as
            // their window's pin does.
            if action == super::status::RESOURCES {
                self.toggle_resources_pin();
                return Some(MouseResult::Redraw);
            }
            if action == super::status::FILE_DETAILS {
                self.click_status_popover(action);
                return Some(MouseResult::Redraw);
            }
            if action == ui_workbench::STATUS_MORE {
                self.open_status_more();
                return Some(MouseResult::Redraw);
            }
            return Some(MouseResult::Command(action));
        }
        None
    }

    /// A click on the parts around the editor: split dividers, color swatches, breadcrumbs,
    /// side panel, terminal, tool strips, project panel, tabs. `None`: not handled here.
    pub(super) fn click_chrome(
        &mut self,
        text: &mut TextSystem,
        p: Point,
        clicks: u32,
        mods: ui_workbench::PressMods,
    ) -> Option<MouseResult> {
        // The separator between groups: resizing by dragging.
        if self.editors.split.begin_drag(p).is_some() {
            return Some(MouseResult::StartDrag);
        }
        // Click in another editor group: it becomes the current group, then the click continues.
        self.focus_group_at(p);
        if self.open_color_picker_at(p) {
            return Some(MouseResult::Redraw);
        }
        if let Some(i) = self.breadcrumbs.hit(p) {
            match self.crumbs.value().and_then(|(_, targets)| targets.get(i).copied().flatten()) {
                Some(offset) => {
                    self.reveal_offset(text, offset);
                }
                None => {
                    self.reveal_in_project(self.active);
                }
            }
            return Some(MouseResult::Redraw);
        }
        if let Some(side) = self.tools.side_at(p) {
            // Its header's buttons (when the app draws its header).
            if side.header.contains(p) {
                let hit = side.header.hit_rect(p);
                let tool = side.id;
                // A bottom panel's size and hide buttons (the app's own).
                use super::tool_windows::BottomSize;
                match hit.map(|(_, id)| id) {
                    Some(super::window_menu::WINDOW_MORE) => {
                        if let Some((at, _)) = hit {
                            self.open_window_menu(tool, at);
                        }
                        return Some(MouseResult::Redraw);
                    }
                    Some(side_panel::BOTTOM_HIDE) => {
                        self.close_side_panel(tool);
                        self.layout();
                        return Some(MouseResult::Redraw);
                    }
                    Some(side_panel::BOTTOM_HALF) => {
                        self.toggle_bottom_size(BottomSize::Half);
                        return Some(MouseResult::Redraw);
                    }
                    Some(side_panel::BOTTOM_FULL) => {
                        self.toggle_bottom_size(BottomSize::Full);
                        return Some(MouseResult::Redraw);
                    }
                    _ => {}
                }
                let action = hit.and_then(|(_, id)| side.panel.header_action(id));
                match action {
                    // Its own menu, under the button.
                    Some(ui_workbench::ToolPanelAction::Menu(entries)) => {
                        if let Some((rect, _)) = hit {
                            self.open_panel_menu(
                                tool,
                                entries,
                                Point::new(rect.min_x(), rect.max_y() + 4.0 * self.scale),
                            );
                        }
                    }
                    Some(action) => self.side_action(text, action),
                    None => {}
                }
                return Some(MouseResult::Redraw);
            }
            side.dragging = true;
            // A left one (in the project panel's place) or one that takes text gets the keyboard.
            let dock = side.dock;
            if dock == ui_workbench::Dock::Left
                || side.panel.wants_text()
                || side.panel.takes_keys()
            {
                self.focus = Focus::Side(dock);
            }
            if let Some(action) =
                self.tools.side_at(p).and_then(|s| s.panel.press_with(p, clicks, mods))
            {
                let focus = self.focus;
                self.side_action(text, action);
                // A file opened from its rows: the keyboard stays with it (as it was).
                let keeps = self.tools.side_at(p).is_some_and(|s| s.panel.keeps_keyboard());
                if keeps && focus == Focus::Side(dock) {
                    self.focus = focus;
                }
            }
            return Some(MouseResult::Redraw);
        }
        if self.tools.terminal_splitter.begin_drag(p) {
            if clicks == 2 {
                self.tools.terminal_splitter.end_drag();
                self.tools.bottom_size = tool_windows::BottomSize::Kept;
                self.data.sizes.remove(workbench_model::sizes::DOCK_BOTTOM);
                self.layout();
                return Some(MouseResult::Redraw);
            }
            return Some(MouseResult::StartDrag);
        }
        if let Some((_, id)) = self.tools.strip.hit_rect(p) {
            // A registered button on the left: its window at the bottom shown / hidden, or —
            // none yet, or the app's own — its command (Git's log, the project panel).
            let id = id.to_owned();
            if self.toggle_tool_window(&id) {
                return Some(MouseResult::Redraw);
            }
            if let Some(command) = self.tool_command(&id) {
                self.run_tool_command(command);
                return Some(match self.request.take() {
                    Some(request) => request,
                    None => MouseResult::Redraw,
                });
            }
        }
        if let Some(id) = self.tools.strip.hit(p).or_else(|| self.tools.right_strip.hit(p)) {
            return Some(self.tool_button(id));
        }
        if self.tools.side_on(ui_workbench::Dock::Right).is_some()
            && self.tools.side_splitter.begin_drag(p)
        {
            // Double click on the separator: the default width.
            if clicks == 2 {
                self.tools.side_splitter.end_drag();
                self.data.sizes.remove(workbench_model::sizes::DOCK_RIGHT);
                self.layout();
                return Some(MouseResult::Redraw);
            }
            return Some(MouseResult::StartDrag);
        }
        if self.tools.splitter.begin_drag(p) {
            // Double click on the separator: the default width.
            if clicks == 2 {
                self.tools.splitter.end_drag();
                self.data.sizes.remove(workbench_model::sizes::DOCK_LEFT);
                self.layout();
                return Some(MouseResult::Redraw);
            }
            return Some(MouseResult::StartDrag);
        }
        if let Some(hit) = self.tab_bar.hit(p) {
            return Some(match hit {
                TabHit::Activate(i) => {
                    self.activate(i);
                    // The tab can be dragged (into another group or to another position).
                    self.editors.tab_press = Some((i, p));
                    MouseResult::StartDrag
                }
                TabHit::Close(i) => MouseResult::CloseTab(i),
                TabHit::Maximize => {
                    self.toggle_editor_full(true, true);
                    MouseResult::Redraw
                }
                TabHit::MoreLeft | TabHit::MoreRight => {
                    self.tab_bar.page(hit == TabHit::MoreRight);
                    MouseResult::Redraw
                }
                TabHit::Back | TabHit::Forward => {
                    let target = if hit == TabHit::Forward {
                        self.active + 1
                    } else {
                        self.active.wrapping_sub(1)
                    };
                    if target < self.tabs.len() {
                        self.activate(target);
                        MouseResult::Redraw
                    } else {
                        MouseResult::None
                    }
                }
            });
        }
        None
    }

    /// A click in the active tab: its viewer, its find bar, the editor (selection, folds,
    /// markers).
    pub(super) fn click_editor(
        &mut self,
        text: &mut TextSystem,
        p: Point,
        shift: bool,
        alt: bool,
        primary: bool,
        clicks: u32,
    ) -> MouseResult {
        // A module's bar above the text (a console's Run, Stop…).
        if let Some(result) = self.click_toolbar(text, p) {
            return result;
        }
        // The separator between a preview's halves: over both of them.
        if let Some(result) = self.press_preview_split(p, clicks) {
            return result;
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return MouseResult::None };
        // The viewer: the Editor / Split / Preview switcher, then its clicks (links).
        if let Some(v) = &mut tab.viewer {
            if let Some(&(_, mode)) = v.toggles.iter().find(|(r, _)| r.contains(p)) {
                v.mode = mode;
                tab.sync_bounds(self.editor_area, self.scale);
                return MouseResult::Redraw;
            }
            // Split: the half clicked has the keys.
            v.keys = v.viewer_visible() && v.bounds.contains(p);
            if v.viewer_visible() && v.bounds.contains(p) {
                self.focus = Focus::Editor;
                let mods = ui_workbench::PressMods { shift, primary, alt };
                return match v.viewer.pointer_down(p, clicks, mods) {
                    Some(action) => self.viewer_action(text, action),
                    // No action: a drag may start (e.g. panning the canvas).
                    None => {
                        self.viewer_drag = true;
                        MouseResult::StartDrag
                    }
                };
            }
        }
        if tab.find_open && tab.find_bar.contains(p) {
            if let Some(hit) = tab.find_bar.hit(p) {
                self.find_hit(text, hit);
            }
            return MouseResult::Redraw;
        }
        // On the right lane's thumb: held, a mark under it taken on letting go (unmoved).
        if tab.view.on_lane_thumb(p, &tab.doc, &tab.folds)
            && tab.view.scrollbar_press(p, &tab.doc, &tab.folds)
        {
            return MouseResult::StartDrag;
        }
        // Click on an error marker (the strip on the right): the cursor goes to the error.
        if let Some(offset) = tab.view.error_mark_at(p) {
            let offset = offset.min(tab.doc.buffer().len());
            commands::click(&mut tab.doc, offset, false, false);
            tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
            self.focus = Focus::Editor;
            return MouseResult::Redraw;
        }
        if tab.view.scrollbar_press(p, &tab.doc, &tab.folds) {
            return MouseResult::StartDrag;
        }
        // A lens: its usages, its implementations.
        if let Some((lens, part)) = tab.view.lens_at(p) {
            self.lens_clicked(text, lens, part);
            return MouseResult::Redraw;
        }
        // The bulb: the actions at the cursor.
        if tab.view.bulb_at(p) {
            self.focus = Focus::Editor;
            self.show_context_actions(text);
            return MouseResult::Redraw;
        }
        if let Some(line) =
            tab.view.fold_marker_at(p, &tab.doc, &tab.folds).or_else(|| tab.view.fold_pill_at(p))
        {
            tab.toggle_fold(line);
            return MouseResult::Redraw;
        }
        // A module's statement mark (a `.http` file's ▶): its bar decides (run the request).
        if let Some(line) = tab.view.statement_mark_at(p) {
            let at = tab.doc.buffer().line_start(line as usize);
            let used = tab.toolbar.as_mut().is_some_and(|t| t.bar.mark_clicked(at));
            if used {
                self.take_toolbar_action(text);
                return MouseResult::Redraw;
            }
        }
        // An annotation (blame): its line, then what a module runs on it (git: the commit in
        // the log).
        if let Some(line) = tab.view.annotation_at(p) {
            self.goto_position(text, line as usize, 0);
            let support = Arc::clone(&self.support);
            let _ = support
                .gutter_actions
                .iter()
                .filter(|a| a.mark == workbench_model::GutterMark::Annotation)
                .any(|a| self.run_command(a.command) == Some(true));
            return MouseResult::Redraw;
        }
        // A change bar: what a module runs on it (git: its actions, revert, compare);
        // without one, what its decorator says (git: the file compared with its last commit).
        if let Some(change) = tab.view.vcs_change_at(p) {
            let line = self.change_line(change);
            if let Some(line) = line {
                self.goto_position(text, line, 0);
            }
            let support = Arc::clone(&self.support);
            let ran = support
                .gutter_actions
                .iter()
                .filter(|a| a.mark == workbench_model::GutterMark::Change)
                .any(|a| self.run_command(a.command) == Some(true));
            if !ran && let Some(line) = line {
                self.mark_clicked(line);
            }
            return MouseResult::Redraw;
        }
        if tab.view.bounds().contains(p) {
            self.focus = Focus::Editor;
            let offset = tab.view.offset_at(p, &tab.doc, &tab.folds, text, &self.palette);
            // ⌘+click (Ctrl+click): to the declaration under the mouse.
            if primary {
                commands::click(&mut tab.doc, offset, false, false);
                // A document link under the mouse (a URL, a file in quotes): followed.
                let link = self
                    .link
                    .as_ref()
                    .filter(|l| {
                        l.tab == self.active && l.range.start <= offset && offset <= l.range.end
                    })
                    .and_then(|l| l.target.clone());
                if let Some(target) = link {
                    self.follow_link(text, target);
                    return self.take_request().unwrap_or(MouseResult::Redraw);
                }
                if !self.goto_definition(text)
                    && let Some(link) = self.document_link_at(offset)
                {
                    self.follow_link(text, link.target);
                    return self.take_request().unwrap_or(MouseResult::Redraw);
                }
                return MouseResult::Redraw;
            }
            match clicks {
                2 => {
                    let language = tab.syntax.snapshot().map_or_else(
                        || tab.syntax.language().clone(),
                        |s| s.language_at(offset).clone(),
                    );
                    let word_chars =
                        self.support.languages.get(&language).map_or("", |l| l.word_chars);
                    let origin = commands::word_range(&tab.doc, offset, word_chars);
                    commands::select_word_at(&mut tab.doc, offset, word_chars);
                    self.drag_unit = Some(DragUnit::Word { origin, word_chars });
                }
                n if n >= 3 => {
                    let origin = commands::line_range(&tab.doc, offset);
                    commands::select_line_at(&mut tab.doc, offset);
                    self.drag_unit = Some(DragUnit::Line { origin });
                }
                // ⌥ + click: adds / removes a cursor; dragged further: column selection.
                _ if alt && !shift => {
                    let anchor = tab.view.column_at(p, &tab.doc, &tab.folds, text, &self.palette);
                    commands::toggle_cursor(&mut tab.doc, offset);
                    self.drag_unit = Some(DragUnit::Column { anchor });
                }
                _ => commands::click(&mut tab.doc, offset, shift, alt),
            }
            return MouseResult::StartDrag;
        }
        MouseResult::None
    }

    /// A click on the top bar. Full screen, its middle window button leaves full screen
    /// and its empty area does nothing (nothing to move or maximize).
    pub(in crate::workbench) fn click_top_bar(
        &mut self,
        p: Point,
        clicks: u32,
    ) -> Option<MouseResult> {
        let full_screen = self.chrome.full_screen;
        Some(match self.chrome.top_bar.hit(p)? {
            TopBarHit::Menu => {
                self.toggle_menu_drawer();
                MouseResult::Redraw
            }
            // A module's badge: what it runs (the project's number chosen).
            TopBarHit::Badge => match self.chrome.badge.as_ref().and_then(|b| b.command) {
                Some(command) => {
                    self.run_command(command);
                    MouseResult::Redraw
                }
                None => MouseResult::None,
            },
            TopBarHit::Projects => {
                self.home.recent = self.recent_projects();
                self.sync_home_sessions();
                if self.home.projects_menu.is_open() {
                    self.home.projects_menu.close();
                } else {
                    self.home.projects_menu.open();
                }
                MouseResult::Redraw
            }
            // A registered button (search, settings): its command.
            TopBarHit::Action(id) => self.tool_button(id),
            TopBarHit::Minimize => MouseResult::MinimizeWindow,
            TopBarHit::Maximize if full_screen => {
                MouseResult::Command(crate::commands::FULL_SCREEN.0)
            }
            TopBarHit::Maximize => MouseResult::ToggleMaximize,
            TopBarHit::Close => MouseResult::CloseWindow,
            TopBarHit::Empty if full_screen => MouseResult::None,
            TopBarHit::Empty if clicks == 2 => MouseResult::ToggleMaximize,
            TopBarHit::Empty => MouseResult::DragWindow,
        })
    }

    /// A right click on the active editor's text: the cursor moves to it, unless it falls
    /// inside a selection (the menu then acts on the selection). `false` if `p` isn't on
    /// a file's editor (or something is open over it).
    pub fn editor_context_click(&mut self, text: &mut TextSystem, p: Point) -> bool {
        if self.modal_open() || !self.editor_focusable_at(p) {
            return false;
        }
        let palette = self.palette.clone();
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        if tab.view.over_scrollbar(p) || (tab.find_open && tab.find_bar.contains(p)) {
            return false;
        }
        if let Some(v) = &mut tab.viewer {
            v.keys = false;
        }
        let offset = tab.view.offset_at(p, &tab.doc, &tab.folds, text, &palette);
        let inside =
            tab.doc.selections().iter().any(|s| !s.is_empty() && s.range().contains(&offset));
        if !inside {
            commands::click(&mut tab.doc, offset, false, false);
        }
        self.focus = Focus::Editor;
        true
    }
}
