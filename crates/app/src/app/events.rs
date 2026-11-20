//! The platform's events, one handler per kind.

use super::*;

impl EditorApp {
    pub(super) fn event_inner(&mut self, event: PlatformEvent) -> Control {
        if self.frames.renderer.is_none() {
            return Control::Close;
        }
        // A profile recording counts what the user did and what the window went through
        // (a wake isn't counted: the poll it leads to names who had something new).
        if kernel::probe::recording() {
            use kernel::probe::{Cause, trigger};
            match &event {
                PlatformEvent::RedrawRequested | PlatformEvent::Wake => {}
                PlatformEvent::Resized(_)
                | PlatformEvent::ScaleFactorChanged(_)
                | PlatformEvent::Occluded(_)
                | PlatformEvent::Focused(_)
                | PlatformEvent::CloseRequested
                | PlatformEvent::SystemAppearanceChanged(..)
                | PlatformEvent::MenuOpening
                | PlatformEvent::ScreenColor(..) => trigger("window", Cause::Window),
                _ => trigger("input", Cause::Input),
            }
        }
        match event {
            PlatformEvent::CloseRequested => return self.close_window(),
            PlatformEvent::RedrawRequested => return self.on_redraw_requested(),
            PlatformEvent::Resized(size) => self.on_resized(size),
            PlatformEvent::ScaleFactorChanged(_) => self.relayout(),
            // A frame skipped while the window was hidden is redone when it becomes visible again.
            PlatformEvent::Occluded(false) | PlatformEvent::Focused(true) => {
                // Back from another program: the repository may have changed meanwhile.
                if matches!(event, PlatformEvent::Focused(true)) {
                    self.workbench.refresh_vcs();
                    self.workbench.project_brought_forward();
                    // What changed meanwhile in the open files (the folders shown: the
                    // project panel's own check), compared off this thread.
                    self.workbench.check_open_files();
                }
                self.window_state.occluded = false;
                self.window_state.hidden = None;
                self.workbench.set_occluded(false);
                // Back in front: the theme's effects move again (stopped while idle, they
                // waited for input — the focus coming back counts as such).
                self.workbench
                    .window_returned(Instant::now(), matches!(event, PlatformEvent::Focused(true)));
                if let Some(renderer) = &mut self.frames.renderer {
                    renderer.set_visible(true);
                }
                if let Some(resources) = &mut self.resources {
                    resources.set_visible(true);
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            PlatformEvent::CursorMoved(p) => self.on_cursor_moved(p),
            PlatformEvent::CursorLeft => {
                if self.workbench.pointer_left() {
                    self.redraw();
                }
            }
            PlatformEvent::ModifiersChanged(m) => self.on_modifiers_changed(m),
            PlatformEvent::Pinch(delta) => {
                if self.workbench.zoom_by(1.0 + delta, self.pointer.cursor) {
                    self.redraw();
                }
            }
            PlatformEvent::Scroll(delta) => self.on_scroll(delta),
            PlatformEvent::MouseDown { button: MouseButton::Left, position, modifiers } => {
                // On the window, not on a page over it: the keyboard back from the pages.
                self.web_views.release_focus();
                self.double_shift.cancel();
                self.alt_tap.cancel();
                self.mouse_down(position, modifiers.shift, modifiers.alt, modifiers.primary());
            }
            PlatformEvent::MouseUp { button: MouseButton::Left, .. } => {
                self.pointer.dragging = false;
                if self.with_text(Workbench::mouse_up).unwrap_or(false) {
                    self.redraw();
                }
            }
            PlatformEvent::MouseDown { button: MouseButton::Right, position, .. } => {
                self.on_right_click(position)
            }
            PlatformEvent::Ime(ime) => self.on_ime(ime),
            PlatformEvent::SystemAppearanceChanged(appearance) => {
                self.on_appearance_changed(appearance)
            }
            PlatformEvent::Wake => self.on_wake(),
            // The menu bar about to show: its rows read again (a module's own state —
            // Revert Change after typing — may have changed without the window's), pushed
            // by `sync_menu_states` after the event.
            PlatformEvent::MenuOpening => self.window_state.menu_context = None,
            PlatformEvent::ScreenColor(color) => self.on_screen_color(color),
            PlatformEvent::FilesHovered { paths, position } => {
                self.pointer.cursor = position;
                self.workbench.drag_files_over(&paths, position);
                self.redraw();
            }
            PlatformEvent::FilesHoverCancelled => {
                if self.workbench.end_file_drag() {
                    self.redraw();
                }
            }
            PlatformEvent::FilesDropped { paths, position } => {
                self.pointer.cursor = position;
                self.drop_files(&paths, position);
            }
            PlatformEvent::Command(id) => return self.on_menu_command(id),
            PlatformEvent::Key(key) => return self.on_key(key),
            PlatformEvent::Occluded(true) => {
                self.window_state.occluded = true;
                self.wait_while_hidden();
                self.workbench.set_occluded(true);
                // The frame kept for partial redraws (a surface-sized texture) goes meanwhile.
                if let Some(renderer) = &mut self.frames.renderer {
                    renderer.set_visible(false);
                }
                // The status bar's figures aren't seen: not measured meanwhile.
                if let Some(resources) = &mut self.resources {
                    resources.set_visible(false);
                }
            }
            // Left for another window or program: its project told (what it holds only for
            // use may go after a while), its picture as it was (the open projects' list), then
            // the frame kept for quick redraws given back (a window-sized texture, ~24 MB on
            // a large Retina one): it rarely draws now; the next frame is drawn whole.
            PlatformEvent::Focused(false) => {
                self.workbench.project_left();
                self.capture_window(false);
                if let Some(renderer) = &mut self.frames.renderer {
                    renderer.release_idle();
                }
                self.workbench.icons().release_drawings();
            }
            PlatformEvent::MouseDown { .. } | PlatformEvent::MouseUp { .. } => {}
        }
        Control::Continue
    }

    fn on_redraw_requested(&mut self) -> Control {
        self.render();
        if self.startup.first_frame_logged && self.step_resize_test() {
            return Control::Exit;
        }
        Control::Continue
    }

    fn on_resized(&mut self, size: PhysicalSize) {
        if let Some(renderer) = &mut self.frames.renderer {
            renderer.resize(size);
        }
        // No `relayout()`: that also requests a RedrawRequested, so each resize step would
        // be painted twice (once here, synchronously, once after).
        if let Some(window) = self.window.clone() {
            self.workbench.resize(window.inner_size(), window.scale_factor() as f32);
        }
        self.frames.pending_input = None;
        // We paint synchronously, during the resize event: if we waited for the next
        // RedrawRequested, the system would meanwhile show the old frame stretched to the
        // new size (the text appears to jitter).
        self.render();
    }

    fn on_cursor_moved(&mut self, p: Point) {
        self.pointer.cursor = p;
        // Files dragged over the window: only where they'd go follows the pointer.
        if self.workbench.file_drag_active() {
            if self.workbench.move_file_drag(p) {
                self.redraw();
            }
            return;
        }
        if self.pointer.dragging {
            if self.with_text(|wb, text| wb.mouse_drag(text, p)).unwrap_or(false) {
                self.redraw();
            }
            if let Some(request) = self.workbench.take_request() {
                self.mouse_result(request);
            }
            return;
        }
        let (mut redraw, mut icon) = self.workbench.hover(p);
        // The diagnostics overlay's × (over everything else).
        redraw |= self.frames.overlay.hover(p);
        if self.frames.overlay.close_hovered() {
            icon = CursorIcon::Pointer;
        }
        redraw |= self.with_text(|wb, text| wb.update_link(text, p)).unwrap_or(false);
        redraw |= self.with_text(|wb, text| wb.update_error_tip(text, p)).unwrap_or(false);
        redraw |= self.with_text(|wb, text| wb.update_code_hover(text, p)).unwrap_or(false);
        if let Some(at) = self.workbench.hover_deadline()
            && let Some(window) = &self.window
        {
            window.request_redraw_at(at);
        }
        if self.workbench.has_link() {
            icon = CursorIcon::Pointer;
        }
        if icon != self.pointer.cursor_icon {
            self.pointer.cursor_icon = icon;
            if let Some(window) = &self.window {
                window.set_cursor_icon(icon);
            }
        }
        if redraw && let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn on_modifiers_changed(&mut self, m: platform_api::Modifiers) {
        if self.double_shift.modifiers(m, Instant::now()) && self.workbench.open_project_search() {
            self.relayout();
        }
        // Alt alone: the menu drawer (where Alt is the menu's key).
        if self.alt_tap.modifiers(m) && !MAC && self.workbench.toggle_menu_drawer() {
            self.relayout();
        }
        let p = self.pointer.cursor;
        // ⌘ alone: links to declarations; ⌘⌥: the mouse wheel zooms.
        self.workbench.set_wheel_zoom(m.primary() && m.alt);
        let redraw = self
            .with_text(|wb, text| wb.set_link_mode(text, m.primary() && !m.alt, p))
            .unwrap_or(false);
        let icon =
            if self.workbench.has_link() { CursorIcon::Pointer } else { self.workbench.hover(p).1 };
        if icon != self.pointer.cursor_icon {
            self.pointer.cursor_icon = icon;
            if let Some(window) = &self.window {
                window.set_cursor_icon(icon);
            }
        }
        if redraw {
            self.redraw();
        }
    }

    fn on_scroll(&mut self, delta: ScrollDelta) {
        self.alt_tap.cancel();
        let (dx, dy) = match delta {
            ScrollDelta::Lines { x, y } => {
                let line = self.workbench.line_height();
                (-x * 3.0 * line, -y * 3.0 * line)
            }
            ScrollDelta::Pixels { x, y } => (-x, -y),
        };
        if self.workbench.scroll(self.pointer.cursor, dx, dy) {
            self.redraw();
        }
    }

    /// The context menu of what was right-clicked, the first that has one.
    fn on_right_click(&mut self, position: Point) {
        // On a notification: nothing (no menu of what's under it).
        if self.workbench.over_toast(position) {
            return;
        }
        // A module's window over everything: its own menu, nothing of what's under it.
        if self.workbench.has_modal_view() {
            if let Some(entries) = self.workbench.viewer_menu_at(position) {
                self.show_entries(entries, Vec::new(), position);
            }
            return;
        }
        if !self.projects_list_menu(position)
            && !self.tool_strip_menu(position)
            && !self.tool_tab_context_menu(position)
            && !self.tool_view_context_menu(position)
            && !self.tree_context_menu(position)
            && !self.side_panel_menu(position)
            && !self.viewer_context_menu(position)
            && !self.editor_context_menu(position)
        {
            self.tab_context_menu(position);
        }
    }

    fn on_ime(&mut self, ime: ImeEvent) {
        match ime {
            ImeEvent::Preedit { text, .. } => {
                if self.workbench.set_preedit(Some(text)) {
                    self.redraw();
                }
            }
            // Control characters (an Enter, a Tab) come as keys: a commit carrying one (some
            // input methods send `\r` after the key) would land wherever the key's action
            // left the keyboard — e.g. in the editor, after Enter closed a modal.
            ImeEvent::Commit(typed) if typed.chars().any(char::is_control) => {
                self.workbench.set_preedit(None);
            }
            ImeEvent::Commit(typed) => {
                self.workbench.set_preedit(None);
                if self.with_text(|wb, text| wb.type_text(text, &typed)).unwrap_or(false) {
                    self.redraw();
                }
            }
        }
    }

    fn on_appearance_changed(&mut self, appearance: SystemAppearance) {
        let follows =
            self.shared.themes.borrow_mut().follow_appearance(theme_appearance(appearance));
        if follows {
            self.theme_changed();
            self.relayout();
        }
    }

    /// Hidden: after [`super::HIDDEN_AFTER`] still hidden, the image on screen goes too
    /// (`EDITOR_KEEP_HIDDEN_SURFACE`: never). The wait wakes this window (a hidden one gets
    /// no frames to look at the time in).
    fn wait_while_hidden(&mut self) {
        if std::env::var_os("EDITOR_KEEP_HIDDEN_SURFACE").is_some() {
            return;
        }
        let Some(window) = &self.window else { return };
        let wake = window.waker();
        let wait = kernel::work::Later::run("hidden-window", super::HIDDEN_AFTER, move || wake());
        self.window_state.hidden = wait.map(|wait| (Instant::now(), wait));
    }

    /// Hidden long enough (woken by [`Self::wait_while_hidden`]'s wait): the image on screen
    /// given back.
    fn release_if_hidden(&mut self) {
        let due = self.window_state.occluded
            && self
                .window_state
                .hidden
                .as_ref()
                .is_some_and(|(since, _)| since.elapsed() >= super::HIDDEN_AFTER);
        if !due {
            return;
        }
        self.window_state.hidden = None;
        if let Some(renderer) = &mut self.frames.renderer {
            renderer.release_hidden();
            tracing::debug!("hidden a while: the window's image on screen given back");
        }
    }

    /// Background work woke the loop: collects what it finished.
    fn on_wake(&mut self) {
        self.release_if_hidden();
        let mut changed = self.workbench.poll_syntax();
        changed |= self.workbench.poll_index();
        changed |= self.workbench.poll_symbols();
        changed |= self.workbench.poll_indexer();
        changed |= self.workbench.poll_fs();
        changed |= self.workbench.poll_format_reads();
        changed |= self.workbench.poll_viewers();
        changed |= self.workbench.poll_decorations();
        changed |= self.workbench.poll_jobs();
        changed |= self.workbench.poll_saves();
        changed |= self.workbench.poll_entry_changes();
        changed |= self.workbench.poll_diagnostics();
        // A list a module's assist made, its data come since (the columns it asked for).
        changed |= self.workbench.poll_assists();
        // What the language servers answered (a list, a card, a jump: used at the next frame).
        changed |= self.workbench.poll_servers();
        // The active document checked by the project's frameworks (in the background).
        self.workbench.poll_frameworks();
        // A large document's find searched whole on a worker: its matches.
        changed |= self.with_text(|wb, text| wb.poll_find(text)).unwrap_or(false);
        self.with_text(|wb, text| wb.run_panel_actions(text));
        // Background work finished with a request (a project to open).
        if let Some(request) = self.workbench.take_request() {
            self.mouse_result(request);
        }
        if let Some(sample) = self.resources.as_ref().and_then(ResourceMonitor::latest) {
            let gpu = self.frames.renderer.as_ref().and_then(|r| r.gpu_memory());
            changed |= self.workbench.set_resources(sample, gpu);
        }
        if changed && let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn on_screen_color(&mut self, color: Option<kernel::Rgba>) {
        tracing::debug!(?color, "eyedropper result");
        self.window_state.sampling = false;
        self.redraw();
        if self.workbench.screen_color(color) {
            self.relayout();
        }
    }

    /// A menu's command: a tool tab's, a row a module listed (a comparison), one on a tab
    /// (`id@doc`) or a plain one.
    fn on_menu_command(&mut self, id: String) -> Control {
        // A row of the bottom window's view's menu (Git's history): back to that view.
        if let Some(command) = id.strip_prefix(crate::workbench::TOOL_VIEW_MENU) {
            let command = command.to_owned();
            self.with_text(|wb, text| wb.run_tool_view_menu(text, &command));
            if let Some(request) = self.workbench.take_request() {
                self.mouse_result(request);
            }
            self.relayout();
            return Control::Continue;
        }
        // A row of a view's own menu (a grid's): back to the view.
        if let Some(command) = id.strip_prefix(crate::workbench::VIEW_MENU) {
            let command = command.to_owned();
            self.with_text(|wb, text| wb.run_view_menu(text, &command));
            if let Some(request) = self.workbench.take_request() {
                self.mouse_result(request);
            }
            self.relayout();
            return Control::Continue;
        }
        // A row of a bar's own menu (a console's): back to it.
        if let Some(command) = id.strip_prefix(crate::workbench::BAR_MENU) {
            let command = command.to_owned();
            self.with_text(|wb, text| wb.run_bar_menu(text, &command));
            if let Some(request) = self.workbench.take_request() {
                self.mouse_result(request);
            }
            self.relayout();
            return Control::Continue;
        }
        // The app's own rows of a window's options ("⋮"): close its tabs, its size, hide.
        if let Some(row) = id.strip_prefix(crate::workbench::WINDOW_MENU) {
            if self.workbench.window_menu(row) {
                self.relayout();
            }
            return Control::Continue;
        }
        // A row of a side panel's own menu (the "Database" window's): back to it.
        if let Some(command) = id.strip_prefix(crate::workbench::PANEL_MENU) {
            let command = command.to_owned();
            self.with_text(|wb, text| wb.run_panel_menu(text, &command));
            if let Some(request) = self.workbench.take_request() {
                self.mouse_result(request);
            }
            self.relayout();
            return Control::Continue;
        }
        // A row of the status bar's `…` for an item with a window: its window, over the `…`
        // (the resources' Task Manager, when there is one).
        if let Some(action) = id.strip_prefix(crate::workbench::STATUS_MENU) {
            if let Some(command) = self.workbench.status_more_chosen(action) {
                return self.on_menu_command(command.to_owned());
            }
            self.redraw();
            return Control::Continue;
        }
        if let Some(action) = id.strip_prefix(crate::workbench::RECENT_MENU) {
            if action == "delete" {
                self.ask_delete_project();
            } else if self.workbench.recent_menu(action) {
                self.relayout();
            }
            return Control::Continue;
        }
        if let Some(path) = id.strip_prefix(projects::OPEN_RECENT) {
            self.open_recent(Path::new(path));
            return Control::Continue;
        }
        // File › Open Recent: a file of this window's project.
        if let Some(path) = id.strip_prefix(cmd::OPEN_RECENT_FILE) {
            let path = Path::new(path);
            let opened = self.workbench.open_file(path);
            if self.succeeded(opened, Doing::Opening(path)) {
                self.relayout();
            }
            return Control::Continue;
        }
        // A row of View › Tool Windows for a window without a toggle (Git): shown / hidden.
        if let Some(tool) = id.strip_prefix(cmd::TOOL_WINDOW_PREFIX) {
            let context = self.workbench.menu_context();
            if self.workbench.row_state(&context, &id) != crate::workbench::RowState::Enabled {
                return Control::Continue;
            }
            let toggled = self.workbench.toggle_tool(tool);
            if let Some(request) = self.workbench.take_request() {
                self.mouse_result(request);
            }
            if toggled {
                self.relayout();
            }
            return Control::Continue;
        }
        if id.starts_with(cmd::TOOL_TAB_PREFIX) {
            if let Some((action, i)) = cmd::parse_tool_tab(&id)
                && self.workbench.tool_tab_action(action, i)
            {
                self.relayout();
            }
            return Control::Continue;
        }
        if let Some(opened) = self.workbench.run_listed(&id) {
            if opened {
                self.relayout();
            }
            return Control::Continue;
        }
        if id.contains('@') {
            match cmd::parse_with_arg(&id) {
                Some((command, doc)) => {
                    let Some(tab) = self.workbench.tab_index_raw(doc) else {
                        return Control::Continue;
                    };
                    if self.tab_command(command, tab) {
                        self.relayout();
                    } else {
                        self.redraw();
                    }
                }
                None => tracing::warn!(%id, "unknown menu command"),
            }
            return Control::Continue;
        }
        match cmd::by_id(&id) {
            Some(id) => self.command(id),
            None => {
                tracing::warn!(%id, "unknown menu command");
                Control::Continue
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent) -> Control {
        // Modifier keys alone arrive as `Unidentified`; anything else breaks a
        // double Shift.
        if key.pressed && key.key != platform_api::Key::Unidentified {
            self.double_shift.cancel();
            self.alt_tap.cancel();
        }
        self.key(&key)
    }
}
