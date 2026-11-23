//! A module's view over everything: in the app's modal frame (a wizard, e.g. opening a
//! remote project — as settings: the title and the view's subtitle, `esc` to close, the
//! view's keys in the footer), or bare — drawn by the view itself over the whole window,
//! as a modal or a popover (the color picker next to its swatch). It alone gets the
//! keyboard, the text and the mouse while open. A framed one closes with `esc` (the key or
//! its cap) or the view's buttons, never on a click outside; a bare one when the view says
//! (`Close`, or `closed()` after what it asked for) — a popover's view closes itself on a
//! click outside it.

use super::*;

pub(super) struct ModalView {
    pub(super) viewer: Box<dyn ui_workbench::Viewer>,
    /// The app's frame and its title (`None`: bare, the view draws all of it).
    frame: Option<(ui_workbench::Modal, String)>,
    /// A popover (bare): the scroll goes on to what's under it.
    pub(super) popover: bool,
    /// A press started in the view (its drags go to it).
    dragging: bool,
    /// The view's virtual extension (which one it is).
    pub(super) extension: String,
    /// The text options' generation its viewer was last told (0: never).
    pub(super) text_told: u64,
}

impl std::fmt::Debug for ModalView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModalView").finish_non_exhaustive()
    }
}

impl Workbench {
    /// Opens the view contributed for `extension` in the modal window, with `data`.
    pub(super) fn open_modal_view(
        &mut self,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        let mut frame = ui_workbench::Modal::default();
        frame.open();
        self.open_module_overlay(extension, data, Some((frame, title.to_owned())), false)
    }

    /// Opens the view contributed for `extension` bare, over the whole window (it draws
    /// itself): a modal, or a popover.
    pub(super) fn open_bare_view(
        &mut self,
        extension: &str,
        data: &dyn std::any::Any,
        popover: bool,
    ) -> bool {
        self.open_module_overlay(extension, data, None, popover)
    }

    fn open_module_overlay(
        &mut self,
        extension: &str,
        data: &dyn std::any::Any,
        frame: Option<(ui_workbench::Modal, String)>,
        popover: bool,
    ) -> bool {
        let path = PathBuf::from(format!("modal.{extension}"));
        let provider = self
            .support
            .viewers
            .iter()
            .find(|p| p.kind(&path) == Some(ui_workbench::ViewerKind::Replace));
        let Some(provider) = provider else { return false };
        // Over the window: the editor's suggestions close.
        self.completion.close();
        let mut viewer = match provider.open(&path) {
            Ok(viewer) => viewer,
            Err(e) => {
                tracing::warn!(error = %e, "the modal view didn't open");
                return false;
            }
        };
        self.give_sizes(viewer.as_mut());
        // What a view over the window may need from the app (a calm screen's widgets: the
        // resources), as a side panel is given it.
        viewer.set_data(&workbench_model::ResourceData(
            Arc::clone(&self.task_data) as Arc<dyn workbench_model::ResourceSource>
        ));
        viewer.set_data(data);
        self.code_hover.hide();
        self.completion.close();
        self.tooltips.hide();
        self.context_menu.close();
        self.modal_view = Some(ModalView {
            viewer,
            frame,
            popover,
            dragging: false,
            extension: extension.to_owned(),
            text_told: 0,
        });
        true
    }

    /// Where the view is drawn: the frame's content (from the last paint), or the window.
    /// The frame's footer status under `p`, when it was cut short: all of it.
    fn modal_frame_status_tip(&self, p: Point) -> Option<(Rect, String)> {
        let (frame, _) = self.modal_view.as_ref()?.frame.as_ref()?;
        frame.status_tip(p)
    }

    fn modal_content(&self) -> Rect {
        let window = Rect::new(0.0, 0.0, self.size.width as f32, self.size.height as f32);
        match self.modal_view.as_ref().map(|m| &m.frame) {
            Some(Some((frame, _))) => frame.content(),
            Some(None) => window,
            None => Rect::default(),
        }
    }

    pub(super) fn paint_modal_view(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        let window = Rect::new(0.0, 0.0, self.size.width as f32, self.size.height as f32);
        let waker = self.waker.as_ref().map(|w| Arc::clone(&w.0));
        let scale = self.scale;
        let window_focused = self.window_focused();
        let Some(modal) = &mut self.modal_view else { return };
        let Some((frame, title)) = &mut modal.frame else {
            // Bare: the view draws itself over the window (one drawing all of it told whether
            // the window has the keyboard: unfocused, it slows down).
            let focused = window_focused || !modal.viewer.covers_window();
            modal.viewer.paint(ui_workbench::ViewerPaint {
                bounds: window,
                scale,
                text,
                scene,
                palette: &self.palette,
                icons: &self.icons,
                focused,
                waker,
            });
            return;
        };
        frame.set_viewport(window, scale);
        let subtitle = modal.viewer.subtitle();
        let close = i18n::t("keys.close");
        let view_hints = modal.viewer.key_hints();
        let mut hints: Vec<ui_kit::KeyHint<'_>> =
            view_hints.iter().map(|(keys, what)| (keys.as_slice(), what.as_str())).collect();
        // Nothing to close it with (work under way): no `esc` said either.
        if modal.viewer.modal_closable() {
            hints.push((&["esc"], close.as_str()));
        }
        if modal.viewer.draws_footer() {
            hints.clear();
        }
        // Its buttons and status, in the window's footer.
        let buttons = modal.viewer.modal_buttons();
        let status = modal.viewer.modal_status();
        let spec = ui_workbench::ModalSpec {
            title: title.as_str(),
            icon: modal.viewer.modal_icon(),
            subtitle: subtitle.as_deref(),
            size: ui_workbench::ModalSize::Large,
            sections: &[],
            nav_header: 0.0,
            header_field: 0.0,
            active: None,
            hints: &hints,
            buttons: &buttons,
            status: status.as_ref(),
            fit: modal.viewer.modal_height(),
            fit_width: modal.viewer.modal_width(),
            title_widens: modal.viewer.modal_title_widens(),
            zoom: modal.viewer.zoom_level(),
            closable: modal.viewer.modal_closable(),
        };
        // A dialog to a screen reader, named by its title: what's in it is under it.
        let (icons, palette) = (&self.icons, &self.palette);
        let dialog = || {
            use ui_kit::access::{Described, Role};
            Described::new(Role::Dialog, title.as_str(), window)
        };
        ui_kit::access::group(dialog, || {
            let Some((content, _)) = frame.paint(&spec, icons, text, scene, palette) else {
                return;
            };
            modal.viewer.paint(ui_workbench::ViewerPaint {
                bounds: content,
                scale,
                text,
                scene,
                palette,
                icons,
                focused: true,
                waker,
            });
        });
    }

    /// What the view asked for; "close" closes the window, and so does a view that's done
    /// once its request is made (`closed()`).
    fn modal_action(
        &mut self,
        text: &mut TextSystem,
        action: ui_workbench::ViewerAction,
    ) -> MouseResult {
        let done = self.modal_view.as_ref().is_some_and(|m| m.viewer.closed());
        if done && !matches!(action, ui_workbench::ViewerAction::Close) {
            self.modal_view = None;
        }
        match action {
            ui_workbench::ViewerAction::Close => {
                self.modal_view = None;
                MouseResult::Redraw
            }
            // A project opened ends the wizard, another window's brought to the front ends
            // the list of the open ones (other requests, e.g. a folder picked, don't).
            request @ ui_workbench::ViewerAction::Request(
                ui_workbench::AppRequest::OpenProject(_)
                | ui_workbench::AppRequest::FocusProject(_),
            ) => {
                self.modal_view = None;
                self.viewer_action(text, request)
            }
            // A plan from a window over the editor (Replace in Files): applied, no tab closed.
            ui_workbench::ViewerAction::ApplyRefactoring(plan) => {
                if let Err(why) = self.apply_refactoring(&plan) {
                    workbench_model::WorkbenchHost::notify(
                        self,
                        workbench_model::Notification::error(why),
                    );
                }
                MouseResult::Redraw
            }
            other => self.viewer_action(text, other),
        }
    }

    /// Takes what the view's keys produced; a view done with nothing more to ask for is
    /// closed (never left open unseen, taking every click and key).
    pub(super) fn take_modal_action(&mut self, text: &mut TextSystem) {
        let action = self.modal_view.as_mut().and_then(|m| m.viewer.take_action());
        let Some(action) = action else {
            if self.modal_view.as_ref().is_some_and(|m| m.viewer.closed()) {
                self.modal_view = None;
            }
            return;
        };
        if let result @ (MouseResult::ViewerRequest(_) | MouseResult::OpenProject(_)) =
            self.modal_action(text, action)
        {
            self.request = Some(result);
        }
    }

    /// A click while the window is open: the view's, or nothing (outside it).
    pub(super) fn click_modal_view(
        &mut self,
        text: &mut TextSystem,
        p: Point,
        clicks: u32,
    ) -> Option<MouseResult> {
        let modal = self.modal_view.as_mut()?;
        // Framed: its × closes it; its header, or outside: nothing (a stray click loses
        // nothing). Bare: the view gets every click (it knows its own outside).
        if let Some((frame, _)) = &modal.frame {
            match frame.hit(p) {
                Some(ui_workbench::ModalHit::Close) => {
                    // The view may ask first (unsaved changes): then it closes itself.
                    if modal.viewer.modal_closable() && modal.viewer.may_close() {
                        self.modal_view = None;
                    }
                    return Some(MouseResult::Redraw);
                }
                Some(ui_workbench::ModalHit::Content) => {}
                // The footer's zoom: the view's, around its middle.
                Some(ui_workbench::ModalHit::Zoom(step)) => {
                    let middle = frame.content().center();
                    match step {
                        ui_workbench::ZoomStep::In => modal.viewer.zoom(1.25, middle),
                        ui_workbench::ZoomStep::Out => modal.viewer.zoom(0.8, middle),
                        ui_workbench::ZoomStep::Reset => modal.viewer.reset_zoom(),
                    };
                    return Some(MouseResult::Redraw);
                }
                // A footer's button: the view's command (when it's enabled).
                Some(ui_workbench::ModalHit::Button(i)) => {
                    let button = modal.viewer.modal_buttons().into_iter().nth(i);
                    if let Some(button) = button.filter(|b| b.enabled) {
                        modal.viewer.command(button.id);
                        self.take_modal_action(text);
                    }
                    return Some(MouseResult::Redraw);
                }
                _ => return Some(MouseResult::None),
            }
        }
        modal.dragging = true;
        // A press with its clicks in a row (a double click, a drag that starts there).
        let action = modal.viewer.press(p, clicks.max(1));
        // A menu of a button of its own: opened under it, nothing else done.
        if self.open_asked_view_menu() {
            return Some(MouseResult::Redraw);
        }
        Some(match action {
            Some(action) => self.modal_action(text, action),
            None => {
                self.take_modal_action(text);
                MouseResult::Redraw
            }
        })
    }

    /// A press in the view: the following moves drag (the color picker's square).
    pub(super) fn modal_view_dragging(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| m.dragging)
    }

    /// The view's press followed: `None` when it holds none (a move behind a popover goes
    /// on to what's under it).
    pub(super) fn drag_modal_view(&mut self, p: Point) -> Option<bool> {
        let modal = self.modal_view.as_mut().filter(|m| m.dragging)?;
        Some(modal.viewer.drag(p))
    }

    /// The view's press let go: `None` when it held none.
    pub(super) fn release_modal_view(&mut self) -> Option<bool> {
        let modal = self.modal_view.as_mut().filter(|m| m.dragging)?;
        modal.dragging = false;
        Some(modal.viewer.release())
    }

    pub(super) fn hover_modal_view(&mut self, p: Point) -> Option<(bool, CursorIcon)> {
        let inside = self.modal_content().contains(p);
        let modal = self.modal_view.as_mut()?;
        let framed = modal.frame.as_mut().is_some_and(|(frame, _)| frame.hover(p));
        let bare = modal.frame.is_none();
        let viewer = &mut modal.viewer;
        // Outside the card: nothing lit in the view.
        let at = if inside { p } else { Point::new(f32::MIN, f32::MIN) };
        let redraw = viewer.hover(at);
        let icon = match inside.then(|| viewer.cursor_at(p)).flatten() {
            Some(cursor) => super::cursor_icon(cursor),
            None if inside && !bare => CursorIcon::Text,
            None => CursorIcon::Default,
        };
        let over_close = modal.frame.as_ref().and_then(|(frame, _)| frame.hit(p));
        let icon = if matches!(
            over_close,
            Some(ui_workbench::ModalHit::Close | ui_workbench::ModalHit::Button(_))
        ) {
            CursorIcon::Pointer
        } else {
            icon
        };
        Some((redraw || framed, icon))
    }

    /// The view's tooltip at `p`, inside its card only; on the footer's status cut short,
    /// all of it.
    pub(super) fn modal_view_tooltip(&self, p: Point) -> Option<ui_workbench::Tip> {
        if let Some((at, status)) = self.modal_frame_status_tip(p) {
            return Some(ui_workbench::Tip::new(at, status));
        }
        if !self.modal_content().contains(p) {
            return None;
        }
        self.modal_view.as_ref()?.viewer.tooltip_at(p)
    }

    pub(super) fn scroll_modal_view(&mut self, dx: f32, dy: f32) -> Option<bool> {
        // ⌘⌥ (Ctrl+Alt) + wheel over a view that zooms: its zoom, around its middle (a
        // pinch zooms around the fingers).
        if self.wheel_zoom {
            let middle = Point::new(f32::MIN, f32::MIN);
            if let Some(zoomed) = self.zoom_modal_view((-dy * 0.004).exp(), middle) {
                return Some(zoomed);
            }
        }
        Some(self.modal_view.as_mut()?.viewer.scroll(dx, dy))
    }

    /// The window's view zoomed by `factor` around `at` (its middle when `at` is outside
    /// it), when it zooms: `None` otherwise (no such window, or a view without a zoom).
    pub(super) fn zoom_modal_view(&mut self, factor: f32, at: Point) -> Option<bool> {
        let content = self.modal_content();
        let modal = self.modal_view.as_mut()?;
        modal.viewer.zoom_level()?;
        let at = if content.contains(at) { at } else { content.center() };
        Some(modal.viewer.zoom(factor, at))
    }

    /// The window's view back to 100%, when it zooms.
    pub(super) fn reset_modal_zoom(&mut self) -> Option<bool> {
        let modal = self.modal_view.as_mut()?;
        modal.viewer.zoom_level()?;
        Some(modal.viewer.reset_zoom())
    }

    /// The window's view has a zoom (the zoom's commands are its while it's open).
    pub fn modal_view_zooms(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| m.viewer.zoom_level().is_some())
    }

    /// A key for the window's view (it has the keyboard while open).
    pub(super) fn modal_view_key(&mut self, text: &mut TextSystem, key: &EditKey) -> Option<bool> {
        self.modal_view.as_ref()?;
        // With how far it reaches (a word, the selection extended) for the view's fields.
        let used = match super::editing::panel_key(key) {
            Some((key, reach)) => {
                self.modal_view.as_mut().is_some_and(|m| m.viewer.edit_key_with(key, reach))
            }
            None => false,
        };
        self.take_modal_action(text);
        Some(used)
    }

    /// Typed text (or pasted) for the window's view.
    pub(super) fn modal_view_text(&mut self, text: &mut TextSystem, typed: &str) -> Option<bool> {
        let viewer = &mut self.modal_view.as_mut()?.viewer;
        // A view without a field: a single character is one of its keys (a diagram's `0`,
        // `1`, `+`), as in a tab.
        let mut chars = typed.chars();
        let used = match (viewer.wants_text(), chars.next(), chars.next()) {
            (false, Some(c), None) => viewer.key(c),
            _ => viewer.text_input(typed),
        };
        self.take_modal_action(text);
        Some(used)
    }

    /// A module's view is open over everything (its right-click menu is the only one).
    pub fn has_modal_view(&self) -> bool {
        self.modal_view.is_some()
    }

    /// The window's view's right-click menu at `p` (`None`: no such window; `Some(None)`:
    /// nothing there, or outside its content).
    pub(super) fn modal_view_menu(
        &mut self,
        p: Point,
    ) -> Option<Option<Vec<ui_workbench::ViewMenuEntry>>> {
        let content = self.modal_content();
        let modal = self.modal_view.as_mut()?;
        Some(if content.contains(p) { modal.viewer.context_menu(p) } else { None })
    }

    /// A command for the window's view (⌘A in its field).
    pub(super) fn modal_view_command(&mut self, id: &str) -> Option<bool> {
        Some(self.modal_view.as_mut()?.viewer.command(id))
    }

    /// An edit command (undo, redo, cut) for the window's field, while one has the keys and
    /// no dialog of the app is over it: `None` when there's no such field.
    pub(super) fn modal_view_field_command(&mut self, id: &str) -> Option<bool> {
        if self.dialogs.dialog.is_open() || !self.modal_view_has_input() {
            return None;
        }
        let modal = self.modal_view.as_mut()?;
        Some(modal.viewer.command(id))
    }

    /// The window has a text field (it takes the clipboard commands).
    pub fn modal_view_has_input(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| m.viewer.wants_text())
    }

    /// Its background work. `true`: a redraw.
    pub(super) fn poll_modal_view(&mut self) -> bool {
        let Some(modal) = self.modal_view.as_mut() else { return false };
        let changed = modal.viewer.poll();
        // What the view asks for when its work ends (a project downloaded: open it).
        match modal.viewer.take_action() {
            Some(ui_workbench::ViewerAction::Close) => self.modal_view = None,
            Some(ui_workbench::ViewerAction::Request(request)) => {
                if matches!(request, ui_workbench::AppRequest::OpenProject(_)) {
                    self.modal_view = None;
                }
                self.request = Some(MouseResult::ViewerRequest(request));
            }
            Some(other) => {
                tracing::debug!(?other, "a modal view's action after its work is ignored")
            }
            // Done with nothing to ask for: closed.
            None if modal.viewer.closed() => self.modal_view = None,
            None => {}
        }
        changed
    }

    /// The window's view shows a loader (work going on).
    pub(super) fn modal_view_loading(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| m.viewer.is_loading())
    }

    /// A bare view drawing the whole window is open: nothing behind it is drawn.
    /// A popover's view takes the wheel now (the mouse over its list).
    pub(super) fn modal_view_takes_scroll(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| m.viewer.takes_scroll())
    }

    pub(super) fn modal_view_covers(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| m.frame.is_none() && m.viewer.covers_window())
    }

    /// When the window's view wants its next frame (its own pace).
    pub fn modal_view_deadline(&self) -> Option<std::time::Instant> {
        self.modal_view.as_ref()?.viewer.frame_deadline()
    }

    pub(super) fn modal_view_animating(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| {
            m.viewer.is_animating() || m.frame.as_ref().is_some_and(|(f, _)| f.is_animating())
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/modal_view.rs"]
mod tests;
