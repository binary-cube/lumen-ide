//! A view inside a view as a control: a tab's view with its press, or any view in an area,
//! the menu it asked for under a button of its own kept for the owner.

use std::time::Instant;

use kernel::{Point, Rect};

use crate::tool_panel::KeyReach;
use crate::tooltip::Tip;
use crate::{PressMods, ViewMenuEntry, ViewerCursor, ViewerKey};

use super::{Control, NOWHERE, Press};

/// A tab's view inside a view as a control: as [`Embedded`], its press the one a tab sends
/// (`pointer_down`, with the modifiers).
pub struct TabView<'a> {
    pub view: &'a mut dyn crate::Viewer,
    pub area: Rect,
    pub action: &'a mut Option<crate::ViewerAction>,
    /// The menu it asked for under a button of its own: the owner's to give the app
    /// (its own [`crate::Viewer::take_menu`]).
    pub menu: &'a mut Option<AskedMenu>,
}

/// A menu a view asked for under a button of its own: the button's area and its rows
/// ([`crate::Viewer::take_menu`]).
pub type AskedMenu = (Rect, Vec<ViewMenuEntry>);

/// What `view` asked for under a button of its own, kept in `menu` for its owner (a menu
/// asked before and not taken yet stays when it asked for none since).
fn keep_asked_menu(view: &mut dyn crate::Viewer, menu: &mut Option<AskedMenu>) {
    if let Some(asked) = view.take_menu() {
        *menu = Some(asked);
    }
}

impl std::fmt::Debug for TabView<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabView").field("area", &self.area).finish_non_exhaustive()
    }
}

impl Control for TabView<'_> {
    fn area(&self) -> Option<Rect> {
        Some(self.area)
    }
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        *self.action = self.view.pointer_down(p, clicks, mods);
        keep_asked_menu(self.view, self.menu);
        Press::Held
    }
    fn drag(&mut self, p: Point) -> bool {
        self.view.drag(p)
    }
    fn release(&mut self) -> bool {
        let changed = self.view.release();
        if let Some(action) = self.view.take_action() {
            *self.action = Some(action);
        }
        keep_asked_menu(self.view, self.menu);
        changed
    }
    fn hover(&mut self, p: Option<Point>) -> bool {
        self.view.hover(p.unwrap_or(NOWHERE))
    }
    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.view.cursor_at(p)
    }
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.view.scroll(dx, dy)
    }
    fn focusable(&self) -> bool {
        true
    }
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.view.edit_key_with(key, reach)
    }
    fn char_key(&mut self, key: char) -> bool {
        self.view.key(key)
    }
    fn text(&mut self, typed: &str) -> bool {
        self.view.wants_text() && self.view.text_input(typed)
    }
    fn paste(&mut self, text: &str) -> bool {
        self.view.paste(text)
    }
    fn selected_text(&self) -> Option<String> {
        self.view.selected_text()
    }
    fn command(&mut self, id: &str) -> bool {
        let used = self.view.command(id);
        keep_asked_menu(self.view, self.menu);
        used
    }
    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.view.tooltip_at(p)
    }
    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.view.context_menu(p)
    }
    fn animating(&self) -> bool {
        self.view.is_animating()
    }
    fn frame_deadline(&self) -> Option<Instant> {
        self.view.frame_deadline()
    }
}

/// A view inside a view (a log's lines, a text to read, any [`crate::Viewer`]) as a
/// control in `area`: the input it takes is the view's own; what a press decided is kept in
/// `action` for the owner, a menu it asked for under a button of its own in `menu` (the
/// owner gives it to the app from its own [`crate::Viewer::take_menu`]).
pub struct Embedded<'a> {
    pub view: &'a mut dyn crate::Viewer,
    pub area: Rect,
    pub action: &'a mut Option<crate::ViewerAction>,
    pub menu: &'a mut Option<AskedMenu>,
}

impl std::fmt::Debug for Embedded<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Embedded").field("area", &self.area).finish_non_exhaustive()
    }
}

impl Control for Embedded<'_> {
    fn area(&self) -> Option<Rect> {
        Some(self.area)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.press_with(p, clicks, PressMods::default())
    }

    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        *self.action = self.view.press_with(p, clicks, mods);
        keep_asked_menu(self.view, self.menu);
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.view.drag(p)
    }

    fn release(&mut self) -> bool {
        let changed = self.view.release();
        if let Some(action) = self.view.take_action() {
            *self.action = Some(action);
        }
        keep_asked_menu(self.view, self.menu);
        changed
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.view.hover(p.unwrap_or(NOWHERE))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.view.cursor_at(p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.view.scroll(dx, dy)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.view.edit_key_with(key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        self.view.wants_text() && self.view.text_input(typed)
    }

    fn char_key(&mut self, key: char) -> bool {
        self.view.key(key)
    }

    fn paste(&mut self, text: &str) -> bool {
        self.view.paste(text)
    }

    fn selected_text(&self) -> Option<String> {
        self.view.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        let used = self.view.command(id);
        keep_asked_menu(self.view, self.menu);
        used
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.view.tooltip_at(p)
    }

    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.view.context_menu(p)
    }

    fn animating(&self) -> bool {
        self.view.is_animating()
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.view.frame_deadline()
    }
}
