//! The window's keyboard and mouse (including a bottom view taking the whole keyboard,
//! the terminal).

use super::chords::{KeysOf, TerminalClip, altgr_text, chord, terminal_clip};
use super::*;

impl EditorApp {
    /// A key for a bottom view taking the whole keyboard (the terminal): almost everything
    /// goes to it (Ctrl+C, arrows, Tab, Esc). Only ⌘ shortcuts (macOS) or Ctrl+Shift
    /// (Windows / Linux, as in the terminals there) stay with the app, plus the tool
    /// windows' toggles.
    pub(super) fn panel_key(&mut self, event: &KeyEvent) -> Control {
        let m = event.modifiers;
        let selected = || self.workbench.view_selection().is_some();
        match terminal_clip(event, selected, KeysOf::HERE) {
            Some(TerminalClip::Copy) => {
                if let (Some(text), Some(window)) = (self.workbench.copy(), &self.window) {
                    window.set_clipboard_text(&text);
                }
                if !m.shift && self.workbench.clear_view_selection() {
                    self.redraw();
                }
                return Control::Continue;
            }
            Some(TerminalClip::Paste) => return self.command(cmd::PASTE),
            None => {}
        }
        // The panel's own view (the terminal's) is where its keys are looked up.
        let place: Vec<command_model::When> = self.workbench.key_places();
        if let Some(chord) = chord(event)
            && let Some(id) = self.keymap.lookup_in(&chord, &place)
        {
            let app_shortcut = if MAC { m.meta } else { m.ctrl && m.shift };
            if app_shortcut || self.workbench.tool_toggled_by(id).is_some() {
                return self.command(id);
            }
        }
        use ui_workbench::{KeyPress, PressedKey};
        let key = match &event.key {
            Key::Named(named) => Some(match named {
                NamedKey::ArrowUp => PressedKey::Up,
                NamedKey::ArrowDown => PressedKey::Down,
                NamedKey::ArrowLeft => PressedKey::Left,
                NamedKey::ArrowRight => PressedKey::Right,
                NamedKey::PageUp => PressedKey::PageUp,
                NamedKey::PageDown => PressedKey::PageDown,
                NamedKey::Home => PressedKey::Home,
                NamedKey::End => PressedKey::End,
                NamedKey::Enter => PressedKey::Enter,
                NamedKey::Escape => PressedKey::Escape,
                NamedKey::Tab => PressedKey::Tab,
                NamedKey::Backspace => PressedKey::Backspace,
                NamedKey::Delete => PressedKey::Delete,
                NamedKey::Insert => PressedKey::Insert,
                NamedKey::Space => PressedKey::Char(' '),
                NamedKey::F(n) => PressedKey::F(*n),
            }),
            // Ctrl+letter (Ctrl+C, Ctrl+D, Ctrl+R…): the control code.
            Key::Character(c) if m.ctrl => c.chars().next().map(PressedKey::Char),
            _ => None,
        };
        let sent = match key {
            Some(key) => self.workbench.view_key_press(KeyPress {
                key,
                shift: m.shift,
                ctrl: m.ctrl,
                alt: m.alt,
            }),
            None => match event.text.as_deref() {
                // Plain text (including diacritics and Option characters on macOS).
                Some(typed) if !(MAC && m.meta) => {
                    self.workbench.type_into_view(typed).unwrap_or(false)
                }
                _ => false,
            },
        };
        if sent {
            self.redraw();
        }
        Control::Continue
    }

    /// The same keys for more commands (the user's choice): a small list to pick one from,
    /// with where each works.
    fn choose_command(&mut self, chord: &KeyChord, ids: Vec<CommandId>) {
        let keys = cmd::display_shortcut(&chord.spec());
        let list = workbench_model::ChoiceList {
            title: i18n::tr!("shortcuts.choose_title", keys = keys.clone()),
            subject: keys,
            icon: "keyboard",
            action: i18n::t("shortcuts.choose_run"),
            items: ids
                .iter()
                .map(|id| workbench_model::ChoiceItem {
                    icon: "keyboard",
                    label: cmd::label(*id),
                    detail: cmd::menu_place(*id).unwrap_or_default(),
                })
                .collect(),
        };
        let then: workbench_model::OnChoice = Box::new(move |i, host| {
            if let Some(id) = ids.get(i) {
                host.request_command(*id);
            }
        });
        workbench_model::WorkbenchHost::choose(&mut self.workbench, list, then);
        self.relayout();
    }

    pub(super) fn key(&mut self, event: &KeyEvent) -> Control {
        if !event.pressed {
            return Control::Continue;
        }
        // The calm screen shown by another window: a key here ends it everywhere.
        if self.workbench.everywhere_input() {
            self.redraw();
        }
        // Esc while a bottom window's tab is dragged puts it back (before a shell gets it).
        if matches!(event.key, Key::Named(NamedKey::Escape))
            && self.workbench.cancel_tool_tab_drag()
        {
            self.redraw();
            return Control::Continue;
        }
        // Settings → Shortcuts records the keys pressed (Esc gives up): they don't act.
        if self.workbench.recording_shortcut() {
            if matches!(event.key, Key::Named(NamedKey::Escape)) {
                self.workbench.settings_escape();
            } else if let Some(chord) = chord(event) {
                let spec = chord.spec();
                if let Some(action) =
                    self.workbench.record_shortcut(&spec, &cmd::display_shortcut(&spec))
                {
                    self.settings_action(action);
                }
            }
            self.relayout();
            return Control::Continue;
        }
        if self.workbench.view_takes_keyboard() && !self.workbench.overlay_open() {
            return self.panel_key(event);
        }
        if let Some(chord) = chord(event) {
            let ids = self.keymap.lookup_all_in(&chord, &self.workbench.key_places());
            match ids.as_slice() {
                [] => {}
                [id] => return self.command(*id),
                // The user left these keys to more commands: which one, now?
                _ => {
                    self.choose_command(&chord, ids);
                    return Control::Continue;
                }
            }
        }

        let m = event.modifiers;
        // macOS: Alt = word, Cmd = line / document. Windows/Linux: Ctrl = word.
        let word = if MAC { m.alt } else { m.ctrl };
        let line = MAC && m.meta;
        let extend = m.shift;
        let mv = |motion| Some(EditKey::Move { motion, extend });
        let edit_key = match &event.key {
            Key::Named(NamedKey::ArrowLeft) if line => mv(Motion::LineStart),
            Key::Named(NamedKey::ArrowRight) if line => mv(Motion::LineEnd),
            Key::Named(NamedKey::ArrowUp) if line => mv(Motion::DocumentStart),
            Key::Named(NamedKey::ArrowDown) if line => mv(Motion::DocumentEnd),
            Key::Named(NamedKey::ArrowLeft) => {
                mv(if word { Motion::WordLeft } else { Motion::Left })
            }
            Key::Named(NamedKey::ArrowRight) => {
                mv(if word { Motion::WordRight } else { Motion::Right })
            }
            Key::Named(NamedKey::ArrowUp) => mv(Motion::Up),
            Key::Named(NamedKey::ArrowDown) => mv(Motion::Down),
            Key::Named(NamedKey::Home) if m.primary() => mv(Motion::DocumentStart),
            Key::Named(NamedKey::End) if m.primary() => mv(Motion::DocumentEnd),
            Key::Named(NamedKey::Home) => mv(Motion::LineStart),
            Key::Named(NamedKey::End) => mv(Motion::LineEnd),
            Key::Named(NamedKey::PageUp) => mv(Motion::PageUp(0)),
            Key::Named(NamedKey::PageDown) => mv(Motion::PageDown(0)),
            Key::Named(NamedKey::Enter) => Some(EditKey::Enter { shift: m.shift }),
            Key::Named(NamedKey::Backspace) => Some(EditKey::Backspace { word }),
            Key::Named(NamedKey::Delete) => Some(EditKey::Delete),
            Key::Named(NamedKey::Tab) => Some(EditKey::Tab { shift: m.shift }),
            Key::Named(NamedKey::Escape) => Some(EditKey::Escape),
            _ => None,
        };
        let changed = if let Some(key) = edit_key {
            self.with_text(|wb, text| wb.edit_key(text, key)).unwrap_or(false)
        } else {
            // Typed text: only without Cmd/Ctrl (those combinations are shortcuts), but for
            // what AltGr types (Ctrl + Alt on Windows and Linux).
            let shortcut = !altgr_text(event) && (m.primary() || (MAC && m.ctrl));
            match event.text.as_deref() {
                Some(typed) if !shortcut && !typed.chars().any(char::is_control) => {
                    self.with_text(|wb, text| wb.type_text(text, typed)).unwrap_or(false)
                }
                _ => false,
            }
        };
        if let Some(request) = self.workbench.take_request() {
            self.mouse_result(request);
        }
        if changed {
            self.redraw();
        }
        Control::Continue
    }

    // ─── Mouse ───

    pub(super) fn mouse_down(&mut self, position: Point, shift: bool, alt: bool, primary: bool) {
        // The calm screen shown by another window: a click here ends it everywhere.
        if self.workbench.everywhere_input() {
            self.redraw();
        }
        // The diagnostics overlay's × (drawn over everything): hidden, nothing else clicked.
        if self.frames.overlay.click(position) {
            self.redraw();
            return;
        }
        let now = Instant::now();
        let clicks = match self.pointer.last_click {
            Some((at, p, n))
                if now.duration_since(at) < MULTI_CLICK
                    && (p.x - position.x).abs() < 4.0
                    && (p.y - position.y).abs() < 4.0 =>
            {
                n % 3 + 1
            }
            _ => 1,
        };
        self.pointer.last_click = Some((now, position, clicks));

        let result = self
            .with_text(|wb, text| wb.mouse_down(text, position, shift, alt, primary, clicks))
            .unwrap_or(MouseResult::None);
        self.mouse_result(result);
        // A press in a side panel may become a drag (the tree's entry into a folder).
        if self.workbench.drag_pending() {
            self.pointer.dragging = true;
        }
        // A click can also leave a request (e.g. the color picker's eyedropper).
        if let Some(request) = self.workbench.take_request() {
            self.mouse_result(request);
        }
    }

    /// What the workbench requests after a click (or after keys, see `take_request`).
    pub(super) fn mouse_result(&mut self, result: MouseResult) {
        match result {
            MouseResult::None => {}
            MouseResult::Redraw => self.relayout(),
            MouseResult::StartDrag => {
                self.pointer.dragging = true;
                self.redraw();
            }
            // A folder (from a search) is shown in the project tree.
            MouseResult::Open(path) if path.is_dir() => {
                self.workbench.show_folder(&path);
                self.relayout();
            }
            MouseResult::Open(path) => {
                let result = self.workbench.open_file(&path);
                self.succeeded(result, Doing::Opening(&path));
                self.relayout();
            }
            MouseResult::CloseTab(i) => {
                self.close_tab(i);
                self.relayout();
            }
            MouseResult::Settings(action) => {
                self.settings_action(action);
                self.relayout();
            }
            MouseResult::MenuCommand(id) => {
                // Quit or Close from the drawer: the window goes after this event.
                let control = self.event(PlatformEvent::Command(id));
                self.exit_after(control);
                // A tree file's menu acted on that file for this one command.
                self.workbench.clear_menu_target();
                self.relayout();
            }
            MouseResult::CopyText(value) => {
                if let Some(window) = &self.window {
                    window.set_clipboard_text(&value);
                }
                self.relayout();
            }
            MouseResult::NumberProject(number) => {
                if let Some(window) = &self.window {
                    self.shared.number_asks.borrow_mut().push((window.id().0, number));
                }
            }
            MouseResult::FocusWindow(id) => {
                if let Some(window) = &self.window {
                    window.focus_window(platform_api::WindowId(id));
                }
            }
            MouseResult::Reveal(path) => {
                if let Some(window) = &self.window {
                    window.reveal_in_file_manager(&path);
                }
            }
            MouseResult::OpenUrl(url) => {
                if let Some(window) = &self.window {
                    window.open_url(&url);
                }
            }
            MouseResult::OpenProject(path) => {
                self.open_project(&path);
                self.relayout();
            }
            MouseResult::RestoreSession(session) => {
                if let Some(window) = &self.window {
                    self.shared.reopen_ask.set(Some((session, window.id())));
                }
            }
            MouseResult::DragWindow => {
                // The system has the mouse now: its release may never reach the window.
                self.pointer.held = false;
                if let Some(window) = &self.window {
                    window.drag_window();
                }
            }
            MouseResult::ToggleMaximize => {
                if let Some(window) = &self.window {
                    window.toggle_maximize();
                }
            }
            MouseResult::MinimizeWindow => {
                if let Some(window) = &self.window {
                    window.minimize();
                }
            }
            // As the system's close button: the work still running, then the unsaved files
            // asked about, then the window goes (after this event).
            MouseResult::CloseWindow => {
                let control = self.close_window();
                self.exit_after(control);
            }
            MouseResult::ViewerRequest(request) => self.viewer_request(request),
            MouseResult::Command(id) => match cmd::by_id(id) {
                Some(id) => {
                    let control = self.command(id);
                    self.exit_after(control);
                }
                None => tracing::warn!(%id, "unknown command"),
            },
        }
    }

    /// A window's closing or the app's quitting that `control` asks for, done after this
    /// event (as the event loop is answered); anything else changes nothing.
    pub(super) fn exit_after(&mut self, control: Control) {
        if matches!(control, Control::Close | Control::Exit) {
            self.pending.exit_request.get_or_insert(control);
        }
    }
}
