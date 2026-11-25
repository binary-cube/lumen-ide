//! Parts that wrap another: one that isn't there, one that may be missing, one pressed as
//! a single click whatever the series, one shown only sometimes.

use std::time::Instant;

use kernel::{Point, Rect};

use crate::tool_panel::KeyReach;
use crate::tooltip::Tip;
use crate::{PressMods, ViewMenuEntry, ViewerCursor, ViewerKey};

use super::{Control, Press};

/// A part that isn't there (a place kept in a view's list for one that may be missing):
/// never hit, takes nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct Absent;

impl Control for Absent {}

/// A part that may be missing: itself when it's there, nothing otherwise.
impl<C: Control> Control for Option<C> {
    fn area(&self) -> Option<Rect> {
        self.as_ref()?.area()
    }
    fn hit(&self, p: Point) -> bool {
        self.as_ref().is_some_and(|c| c.hit(p))
    }
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        self.as_mut().map_or(Press::Ignored, |c| c.press_with(p, clicks, mods))
    }
    fn keeps_tab(&self) -> bool {
        self.as_ref().is_some_and(|c| c.keeps_tab())
    }
    fn in_tab_order(&self) -> bool {
        self.as_ref().is_some_and(|c| c.in_tab_order())
    }
    fn captures_keys(&self) -> bool {
        self.as_ref().is_some_and(|c| c.captures_keys())
    }
    fn drag(&mut self, p: Point) -> bool {
        self.as_mut().is_some_and(|c| c.drag(p))
    }
    fn release(&mut self) -> bool {
        self.as_mut().is_some_and(|c| c.release())
    }
    fn hover(&mut self, p: Option<Point>) -> bool {
        self.as_mut().is_some_and(|c| c.hover(p))
    }
    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.as_ref()?.cursor(p)
    }
    fn owns_cursor(&self, p: Point) -> bool {
        self.as_ref().is_some_and(|c| c.owns_cursor(p))
    }
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.as_mut().is_some_and(|c| c.scroll(dx, dy))
    }
    fn takes_wheel(&self, p: Point) -> bool {
        self.as_ref().is_some_and(|c| c.takes_wheel(p))
    }
    fn focusable(&self) -> bool {
        self.as_ref().is_some_and(|c| c.focusable())
    }
    fn set_focus(&mut self, focused: bool) {
        if let Some(c) = self {
            c.set_focus(focused);
        }
    }
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.as_mut().is_some_and(|c| c.key(key, reach))
    }
    fn char_key(&mut self, key: char) -> bool {
        self.as_mut().is_some_and(|c| c.char_key(key))
    }
    fn text(&mut self, typed: &str) -> bool {
        self.as_mut().is_some_and(|c| c.text(typed))
    }
    fn paste(&mut self, text: &str) -> bool {
        self.as_mut().is_some_and(|c| c.paste(text))
    }
    fn selected_text(&self) -> Option<String> {
        self.as_ref()?.selected_text()
    }
    fn command(&mut self, id: &str) -> bool {
        self.as_mut().is_some_and(|c| c.command(id))
    }
    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.as_ref()?.tooltip(p)
    }
    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.as_mut()?.menu(p)
    }
    fn animating(&self) -> bool {
        self.as_ref().is_some_and(|c| c.animating())
    }
    fn frame_deadline(&self) -> Option<Instant> {
        self.as_ref()?.frame_deadline()
    }
}

/// A part pressed as one click whatever the series (a list choosing on the first press, not
/// selecting a word in its search on the second).
pub struct OneClick<'a>(pub &'a mut dyn Control);

impl std::fmt::Debug for OneClick<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OneClick").finish_non_exhaustive()
    }
}

impl Control for OneClick<'_> {
    fn area(&self) -> Option<Rect> {
        self.0.area()
    }
    fn hit(&self, p: Point) -> bool {
        self.0.hit(p)
    }
    fn press_with(&mut self, p: Point, _clicks: u32, mods: PressMods) -> Press {
        self.0.press_with(p, 1, mods)
    }
    fn keeps_tab(&self) -> bool {
        self.0.keeps_tab()
    }
    fn in_tab_order(&self) -> bool {
        self.0.in_tab_order()
    }
    fn captures_keys(&self) -> bool {
        self.0.captures_keys()
    }
    fn drag(&mut self, p: Point) -> bool {
        self.0.drag(p)
    }
    fn release(&mut self) -> bool {
        self.0.release()
    }
    fn hover(&mut self, p: Option<Point>) -> bool {
        self.0.hover(p)
    }
    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.0.cursor(p)
    }
    fn owns_cursor(&self, p: Point) -> bool {
        self.0.owns_cursor(p)
    }
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.0.scroll(dx, dy)
    }
    fn takes_wheel(&self, p: Point) -> bool {
        self.0.takes_wheel(p)
    }
    fn focusable(&self) -> bool {
        self.0.focusable()
    }
    fn set_focus(&mut self, focused: bool) {
        self.0.set_focus(focused);
    }
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.0.key(key, reach)
    }
    fn char_key(&mut self, key: char) -> bool {
        self.0.char_key(key)
    }
    fn text(&mut self, typed: &str) -> bool {
        self.0.text(typed)
    }
    fn paste(&mut self, text: &str) -> bool {
        self.0.paste(text)
    }
    fn selected_text(&self) -> Option<String> {
        self.0.selected_text()
    }
    fn command(&mut self, id: &str) -> bool {
        self.0.command(id)
    }
    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.0.tooltip(p)
    }
    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.0.menu(p)
    }
    fn animating(&self) -> bool {
        self.0.animating()
    }
    fn frame_deadline(&self) -> Option<Instant> {
        self.0.frame_deadline()
    }
}

/// A part shown only sometimes (a panel's bar while the panel is drawn, a tab's part on
/// its tab, a list while it's asked for): while not shown it's never hit and takes
/// nothing; shown, it's `part` itself.
pub struct Shown<'a> {
    pub part: &'a mut dyn Control,
    pub shown: bool,
}

impl std::fmt::Debug for Shown<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shown").field("shown", &self.shown).finish_non_exhaustive()
    }
}

impl Control for Shown<'_> {
    fn area(&self) -> Option<Rect> {
        self.part.area().filter(|_| self.shown)
    }
    fn hit(&self, p: Point) -> bool {
        self.shown && self.part.hit(p)
    }
    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.press_with(p, clicks, PressMods::default())
    }
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        if self.shown { self.part.press_with(p, clicks, mods) } else { Press::Ignored }
    }
    fn keeps_tab(&self) -> bool {
        self.shown && self.part.keeps_tab()
    }
    fn in_tab_order(&self) -> bool {
        self.shown && self.part.in_tab_order()
    }
    fn captures_keys(&self) -> bool {
        self.shown && self.part.captures_keys()
    }
    fn takes_wheel(&self, p: Point) -> bool {
        self.shown && self.part.takes_wheel(p)
    }
    fn char_key(&mut self, key: char) -> bool {
        self.shown && self.part.char_key(key)
    }
    fn paste(&mut self, text: &str) -> bool {
        self.shown && self.part.paste(text)
    }
    fn drag(&mut self, p: Point) -> bool {
        self.part.drag(p)
    }
    fn release(&mut self) -> bool {
        self.part.release()
    }
    fn hover(&mut self, p: Option<Point>) -> bool {
        self.part.hover(p.filter(|_| self.shown))
    }
    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.part.cursor(p).filter(|_| self.shown)
    }
    fn owns_cursor(&self, p: Point) -> bool {
        self.shown && self.part.owns_cursor(p)
    }
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.shown && self.part.scroll(dx, dy)
    }
    fn focusable(&self) -> bool {
        self.shown && self.part.focusable()
    }
    fn set_focus(&mut self, focused: bool) {
        self.part.set_focus(focused);
    }
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.shown && self.part.key(key, reach)
    }
    fn text(&mut self, typed: &str) -> bool {
        self.shown && self.part.text(typed)
    }
    fn selected_text(&self) -> Option<String> {
        self.part.selected_text().filter(|_| self.shown)
    }
    fn command(&mut self, id: &str) -> bool {
        self.shown && self.part.command(id)
    }
    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.part.tooltip(p).filter(|_| self.shown)
    }
    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.part.menu(p).filter(|_| self.shown)
    }
    fn animating(&self) -> bool {
        self.shown && self.part.animating()
    }
    fn frame_deadline(&self) -> Option<Instant> {
        self.part.frame_deadline().filter(|_| self.shown)
    }
}
