//! A shared control used the way one git view needs it ([`Tune`]): Tab kept by a list
//! that answers it itself, the wheel kept by a list over everything. What it doesn't tune
//! is the part's own (a press taken as one click is the shared `OneClick`).

use std::time::Instant;

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, PressMods, Tip, ViewMenuEntry, ViewerCursor, ViewerKey};

/// How a part is used here beyond its own way.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Tune {
    /// With the focus it answers Tab itself (a files tree: its search takes it, else it's
    /// not used), instead of the focus moving on.
    pub(crate) keeps_tab: bool,
    /// While it's hit (an open list hit anywhere) nothing under it gets the wheel: it's its
    /// own even at its end.
    pub(crate) keeps_wheel: bool,
}

/// `part` used as [`Tune`] says.
pub(crate) struct Tuned<'a> {
    pub(crate) part: &'a mut dyn Control,
    pub(crate) tune: Tune,
}

impl Control for Tuned<'_> {
    fn area(&self) -> Option<Rect> {
        self.part.area()
    }
    fn hit(&self, p: Point) -> bool {
        self.part.hit(p)
    }
    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.press_with(p, clicks, PressMods::default())
    }
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Press {
        self.part.press_with(p, clicks, mods)
    }
    fn keeps_tab(&self) -> bool {
        self.tune.keeps_tab || self.part.keeps_tab()
    }
    fn in_tab_order(&self) -> bool {
        self.part.in_tab_order()
    }
    fn captures_keys(&self) -> bool {
        self.part.captures_keys()
    }
    fn takes_wheel(&self, p: Point) -> bool {
        self.tune.keeps_wheel || self.part.takes_wheel(p)
    }
    fn drag(&mut self, p: Point) -> bool {
        self.part.drag(p)
    }
    fn release(&mut self) -> bool {
        self.part.release()
    }
    fn hover(&mut self, p: Option<Point>) -> bool {
        self.part.hover(p)
    }
    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.part.cursor(p)
    }
    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.part.scroll(dx, dy)
    }
    fn focusable(&self) -> bool {
        self.part.focusable()
    }
    fn set_focus(&mut self, focused: bool) {
        self.part.set_focus(focused);
    }
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.part.key(key, reach)
    }
    fn char_key(&mut self, key: char) -> bool {
        self.part.char_key(key)
    }
    fn text(&mut self, typed: &str) -> bool {
        self.part.text(typed)
    }
    fn paste(&mut self, text: &str) -> bool {
        self.part.paste(text)
    }
    fn selected_text(&self) -> Option<String> {
        self.part.selected_text()
    }
    fn command(&mut self, id: &str) -> bool {
        self.part.command(id)
    }
    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.part.tooltip(p)
    }
    fn menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        self.part.menu(p)
    }
    fn animating(&self) -> bool {
        self.part.animating()
    }
    fn frame_deadline(&self) -> Option<Instant> {
        self.part.frame_deadline()
    }
}

#[cfg(test)]
#[path = "../tests/unit/parts.rs"]
mod tests;
