//! How a view's parts are used, as a form's controls are: each part (a field, a list, a
//! scrollbar's thumb, a slider, a text to read) answers one contract, [`Control`], and the
//! view keeps their state of use in one [`Controls`]: which one took the press and holds
//! the pointer until it's let go, which one has the focus (the keys and the text typed go
//! to it; Tab moves it), which one the mouse is over (its hover, its cursor, its tooltip,
//! its menu, the wheel).
//!
//! The view hands its parts as a list each time (they're its fields: nothing is kept but
//! their places in the list) and gets back the place of the one that answered, so only
//! the view knows what each one means; nothing that routes names a part. Every method has
//! a default: a part implements what it uses.

use std::time::Instant;

use kernel::{Point, Rect};

use crate::PressMods;
use crate::tool_panel::KeyReach;
use crate::tooltip::Tip;
use crate::{ViewMenuEntry, ViewerCursor, ViewerKey};

mod bars;
mod views;
mod wrappers;

pub use bars::{BarThumb, Divider, ScrollThumb, Scrolled, Thumb};
pub use views::{AskedMenu, Embedded, TabView};
pub use wrappers::{Absent, OneClick, Shown};

/// The point a part's [`crate::bar_cursor!`] methods are asked at, named for the macro.
#[doc(hidden)]
pub use kernel::Point as BarPoint;

/// A point no part is at: what a part told the mouse left it hovers (`hover(None)` as a
/// point for the components that take one).
pub const NOWHERE: Point = Point::new(f32::MIN, f32::MIN);

/// What a press on a part did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Nothing here for it: it goes on to the parts under it.
    Ignored,
    /// Taken (a click: a button pressed, a row chosen).
    Took,
    /// Taken, and the part holds the pointer until it's let go (a thumb, a selection being
    /// made, a slider): the moves go to it.
    Held,
}

/// A part of a view, used as a form's control.
pub trait Control {
    /// Where it was drawn last (`None`: not drawn, nowhere to be hit).
    fn area(&self) -> Option<Rect> {
        None
    }

    /// `p` is on it (by default: in its area).
    fn hit(&self, p: Point) -> bool {
        self.area().is_some_and(|r| r.contains(p))
    }

    /// A press on it, the `clicks`-th of a quick series.
    fn press(&mut self, _p: Point, _clicks: u32) -> Press {
        Press::Ignored
    }

    /// [`Self::press`] with the modifiers held (⇧ extends, ⌘ / Ctrl adds); by default the
    /// same as without.
    fn press_with(&mut self, p: Point, clicks: u32, _mods: PressMods) -> Press {
        self.press(p, clicks)
    }

    /// With the focus it keeps Tab for itself (a list choosing with it), instead of the
    /// focus moving on.
    fn keeps_tab(&self) -> bool {
        false
    }

    /// Tab stops at it (by default: when it takes the focus). A part focused only from code
    /// or a press (a wizard's fields moved through by its own keys) says `false`.
    fn in_tab_order(&self) -> bool {
        self.focusable()
    }

    /// While shown it takes the keys, the text and the commands whatever has the focus (a
    /// picker over everything, a menu open over its part).
    fn captures_keys(&self) -> bool {
        false
    }

    /// The wheel at `p` over it is its own even when nothing moves (a list at its end
    /// keeps the wheel from what's under it).
    fn takes_wheel(&self, _p: Point) -> bool {
        false
    }

    /// A character key with nothing to type into (`+`, `0`, `r`: a view's own keys), while
    /// it has the focus: `true` if it used it.
    fn char_key(&mut self, _key: char) -> bool {
        false
    }

    /// Text pasted while it has the focus (by default, as if typed).
    fn paste(&mut self, text: &str) -> bool {
        self.text(text)
    }

    /// The pointer moved while it holds it: `true` if what it shows changed.
    fn drag(&mut self, _p: Point) -> bool {
        false
    }

    /// The pointer let go: `true` if what it shows changed.
    fn release(&mut self) -> bool {
        false
    }

    /// The mouse is at `p` over it, or left it (`None`): `true` if what it shows changed.
    fn hover(&mut self, _p: Option<Point>) -> bool {
        false
    }

    /// The cursor over it at `p` (`None`: the view's default).
    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        None
    }

    /// Its cursor at `p` is a bar's (a scrollbar's thumb or lane, one held): it wins over
    /// whatever the view shows there (its rows' hand, its code's I-beam). A view knows a
    /// bar by this ([`Controls::bar_cursor`]), never by the cursor's value.
    fn owns_cursor(&self, _p: Point) -> bool {
        false
    }

    /// The wheel over it: `true` if it scrolled (else it goes on to what's under it).
    fn scroll(&mut self, _dx: f32, _dy: f32) -> bool {
        false
    }

    /// It takes the focus (a field, a list with keys): a press or Tab gives it.
    fn focusable(&self) -> bool {
        false
    }

    /// It got (`true`) or lost the focus.
    fn set_focus(&mut self, _focused: bool) {}

    /// A key while it has the focus: `true` if it used it.
    fn key(&mut self, _key: ViewerKey, _reach: KeyReach) -> bool {
        false
    }

    /// Text typed while it has the focus: `true` if it took it.
    fn text(&mut self, _typed: &str) -> bool {
        false
    }

    /// What it has selected (the copy's), while it has the focus.
    fn selected_text(&self) -> Option<String> {
        None
    }

    /// An edit command while it has the focus (`copy`, `cut`, `select_all`, `undo`,
    /// `redo`): `true` if it took it.
    fn command(&mut self, _id: &str) -> bool {
        false
    }

    /// Its tooltip at `p`.
    fn tooltip(&self, _p: Point) -> Option<Tip> {
        None
    }

    /// Its right-click menu at `p`.
    fn menu(&mut self, _p: Point) -> Option<Vec<ViewMenuEntry>> {
        None
    }

    /// It's moving (an animation asks for the next frame).
    fn animating(&self) -> bool {
        false
    }

    /// When it next needs a frame at its own pace (a fade).
    fn frame_deadline(&self) -> Option<Instant> {
        None
    }
}

/// A view's parts' state of use: by their places in the list the view hands each time
/// (the first one is on top: it's hit first). What a view's read-only questions need (the
/// cursor and the tooltip where the mouse last was, whether a part moves, when one wants a
/// frame) is kept from the last [`Self::hover`] and [`Self::settle`], so they're answered
/// without the parts.
#[derive(Debug, Clone, Default)]
pub struct Controls {
    /// A press on no part that takes it leaves the focus where it was (a page with one
    /// field that always has the keys), instead of nowhere.
    sticky: bool,
    held: Option<usize>,
    focused: Option<usize>,
    hovered: Option<usize>,
    /// Where the mouse last was, the cursor and the tooltip of the part under it there.
    mouse: Option<Point>,
    cursor: Option<ViewerCursor>,
    /// The cursor kept is a bar's ([`Control::owns_cursor`]).
    on_bar: bool,
    tip: Option<Tip>,
    /// From the last [`Self::settle`]: a part moving, the soonest frame one wants.
    moving: bool,
    due: Option<Instant>,
}

/// The list of a view's parts, the topmost first.
pub type Parts<'a, 'b> = &'a mut [&'b mut dyn Control];

impl Controls {
    /// Its focus stays where it is on a press that no part takes (see `sticky`).
    pub fn sticky() -> Self {
        Self { sticky: true, ..Self::default() }
    }

    /// Part `i` holds the pointer from now (a drag begun by code: a row dragged from a
    /// menu), until it's let go.
    pub fn hold(&mut self, i: usize) {
        self.held = Some(i);
    }

    /// A part holds the pointer (the moves must reach [`Self::drag`]).
    pub fn held(&self) -> bool {
        self.held.is_some()
    }

    /// The part with the focus.
    pub fn focused(&self) -> Option<usize> {
        self.focused
    }

    /// The part under the mouse.
    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    /// The topmost part at `p`.
    fn at(parts: &[&mut dyn Control], p: Point) -> Option<usize> {
        parts.iter().position(|c| c.hit(p))
    }

    /// Gives the focus to part `to` (`None`: to none), telling both.
    pub fn focus(&mut self, parts: Parts<'_, '_>, to: Option<usize>) {
        if self.focused == to {
            return;
        }
        if let Some(c) = self.focused.and_then(|i| parts.get_mut(i)) {
            c.set_focus(false);
        }
        self.focused = to.filter(|i| parts.get(*i).is_some_and(|c| c.focusable()));
        if let Some(c) = self.focused.and_then(|i| parts.get_mut(i)) {
            c.set_focus(true);
        }
    }

    /// A press at `p`: the topmost part taking it, and what it did (the focus moves to it
    /// when it takes the focus; a press on no part that takes it leaves the focus nowhere).
    pub fn press(&mut self, parts: Parts<'_, '_>, p: Point, clicks: u32) -> Option<(usize, Press)> {
        self.press_with(parts, p, clicks, PressMods::default())
    }

    /// [`Self::press`] with the modifiers held.
    pub fn press_with(
        &mut self,
        parts: Parts<'_, '_>,
        p: Point,
        clicks: u32,
        mods: PressMods,
    ) -> Option<(usize, Press)> {
        let found = (0..parts.len()).find_map(|i| {
            if !parts[i].hit(p) {
                return None;
            }
            match parts[i].press_with(p, clicks, mods) {
                Press::Ignored => None,
                press => Some((i, press)),
            }
        });
        let Some((i, press)) = found else {
            if !self.sticky {
                self.focus(parts, None);
            }
            return None;
        };
        if press == Press::Held {
            self.held = Some(i);
        }
        if parts[i].focusable() {
            self.focus(parts, Some(i));
        }
        Some((i, press))
    }

    /// The pointer moved while a part holds it: `Some((part, changed))`; `None` when none
    /// holds it.
    pub fn drag(&mut self, parts: Parts<'_, '_>, p: Point) -> Option<(usize, bool)> {
        let i = self.held?;
        let moved = parts.get_mut(i)?.drag(p);
        // The cursor while dragging is the held part's (a separator's, a selection's).
        self.mouse = Some(p);
        self.note_cursor(parts, Some(p));
        Some((i, moved))
    }

    /// The pointer let go: the part that held it, and whether it changed. The cursor kept
    /// is the one under the mouse again (a bar let go over the rows gives them theirs back:
    /// the app asks for it at once, before the mouse moves).
    pub fn release(&mut self, parts: Parts<'_, '_>) -> Option<(usize, bool)> {
        let i = self.held.take()?;
        let released = parts.get_mut(i)?.release();
        self.note_cursor(parts, self.mouse);
        Some((i, released))
    }

    /// The cursor at `p` kept for the read-only questions: the held part's while one is
    /// held (a bar dragged over the rows keeps the bar's, whatever is under the mouse),
    /// else the topmost part's there that says one; and whether it's a bar's.
    fn note_cursor(&mut self, parts: &[&mut dyn Control], p: Option<Point>) {
        let Some(p) = p else {
            (self.cursor, self.on_bar) = (None, false);
            return;
        };
        let shown = match self.held {
            Some(i) => parts.get(i).and_then(|c| Some((i, c.cursor(p)?))),
            None => Self::cursor_part(parts, p),
        };
        self.cursor = shown.map(|(_, cursor)| cursor);
        self.on_bar = shown.is_some_and(|(i, _)| parts[i].owns_cursor(p));
    }

    /// The mouse at `p` (`None`: it left the view): the part it left and the one it's over
    /// are told; `true` if what they show changed.
    pub fn hover(&mut self, parts: Parts<'_, '_>, p: Option<Point>) -> bool {
        let under = p.and_then(|p| Self::at(parts, p));
        let mut changed = false;
        if self.hovered != under
            && let Some(c) = self.hovered.and_then(|i| parts.get_mut(i))
        {
            changed |= c.hover(None);
        }
        self.hovered = under;
        if let (Some(i), Some(p)) = (under, p) {
            changed |= parts[i].hover(Some(p));
        }
        // Kept for the read-only questions about where the mouse is.
        self.mouse = p;
        self.note_cursor(parts, p);
        self.tip = p.and_then(|p| self.tooltip(parts, p));
        changed
    }

    /// The cursor where the mouse last was ([`Self::hover`]), when `p` is there.
    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.cursor.filter(|_| self.mouse == Some(p))
    }

    /// The cursor where the mouse last was when it's a bar's (over a bar, or while one is
    /// held: [`Control::owns_cursor`]); `None` elsewhere, where the view says its own.
    pub fn bar_cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.cursor_at(p).filter(|_| self.on_bar)
    }

    /// A view's cursor at `p` when its own wins over its parts' but not over a bar's (a hand
    /// over its rows, an I-beam over its text): the bar's where the mouse last was (over a
    /// bar, or while one is held), else the view's `own`.
    pub fn bar_or(
        &self,
        p: Point,
        own: impl FnOnce() -> Option<ViewerCursor>,
    ) -> Option<ViewerCursor> {
        self.bar_cursor(p).or_else(own)
    }

    /// A view's cursor at `p`: its parts' where the mouse last was (a bar's over everything
    /// under it, a field's I-beam), else the view's `own` (a hand over what it presses).
    pub fn cursor_or(
        &self,
        p: Point,
        own: impl FnOnce() -> Option<ViewerCursor>,
    ) -> Option<ViewerCursor> {
        self.cursor_at(p).or_else(own)
    }

    /// The tooltip where the mouse last was ([`Self::hover`]), when `p` is there.
    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        self.tip.clone().filter(|_| self.mouse == Some(p))
    }

    /// Notes what the parts want of the frames (a view calls it as it paints): read back by
    /// [`Self::is_animating`] and [`Self::next_frame`].
    pub fn settle(&mut self, parts: &[&mut dyn Control]) {
        self.moving = Self::animating(parts);
        self.due = Self::frame_deadline(parts);
    }

    /// A part was moving at the last [`Self::settle`].
    pub fn is_animating(&self) -> bool {
        self.moving
    }

    /// The soonest frame a part wanted at the last [`Self::settle`].
    pub fn next_frame(&self) -> Option<Instant> {
        self.due
    }

    /// The wheel over the part the mouse is over (for a view told no point): `Some(part)`
    /// when it scrolled.
    pub fn scroll_hovered(&mut self, parts: Parts<'_, '_>, dx: f32, dy: f32) -> Option<usize> {
        let i = self.hovered?;
        parts.get_mut(i)?.scroll(dx, dy).then_some(i)
    }

    /// The cursor at `p`: the topmost part's there that says one.
    pub fn cursor(&self, parts: &[&mut dyn Control], p: Point) -> Option<ViewerCursor> {
        Self::cursor_part(parts, p).map(|(_, cursor)| cursor)
    }

    /// The topmost part at `p` that says a cursor, and its cursor.
    fn cursor_part(parts: &[&mut dyn Control], p: Point) -> Option<(usize, ViewerCursor)> {
        (0..parts.len()).filter(|&i| parts[i].hit(p)).find_map(|i| Some((i, parts[i].cursor(p)?)))
    }

    /// The wheel at `p`: the topmost part there that scrolls (or keeps the wheel,
    /// [`Control::takes_wheel`]), `Some(part)`.
    pub fn scroll(&mut self, parts: Parts<'_, '_>, p: Point, dx: f32, dy: f32) -> Option<usize> {
        (0..parts.len())
            .find(|&i| parts[i].hit(p) && (parts[i].scroll(dx, dy) || parts[i].takes_wheel(p)))
    }

    /// The wheel for a view told no point: the part over everything while it's shown (an
    /// open list), else the one the mouse is over. `Some((part, moved))`.
    pub fn wheel(&mut self, parts: Parts<'_, '_>, dx: f32, dy: f32) -> Option<(usize, bool)> {
        let i = parts.iter().position(|c| c.captures_keys()).or(self.hovered)?;
        Some((i, parts.get_mut(i)?.scroll(dx, dy)))
    }

    /// The wheel for the focused part (a view told no point, the keys' part scrolling).
    pub fn scroll_focused(&mut self, parts: Parts<'_, '_>, dx: f32, dy: f32) -> Option<usize> {
        let i = self.focused?;
        parts.get_mut(i)?.scroll(dx, dy).then_some(i)
    }

    /// The part the keys go to: one that captures them while shown, else the focused one.
    fn keys_to(&self, parts: &[&mut dyn Control]) -> Option<usize> {
        // A focused part that no longer takes the focus (a list closed) has no keys.
        let focused = self.focused.filter(|i| parts.get(*i).is_some_and(|c| c.focusable()));
        parts.iter().position(|c| c.captures_keys()).or(focused)
    }

    /// A key: Tab (⇧Tab back) moves the focus among the parts that take it; any other goes
    /// to the focused part. `Some(part)` when used.
    pub fn key(&mut self, parts: Parts<'_, '_>, key: ViewerKey, reach: KeyReach) -> Option<usize> {
        let target = self.keys_to(parts);
        let keeps_tab = target.and_then(|i| parts.get(i)).is_some_and(|c| c.keeps_tab());
        let captured = target != self.focused;
        if let Some(i) = target
            && (key != ViewerKey::Tab || keeps_tab || captured)
        {
            if parts.get_mut(i)?.key(key, reach) {
                return Some(i);
            }
            // A key the part over everything doesn't use goes on to the focused one (its
            // suggestions take ↑ ↓ ↵, the field under them goes on typing).
            let focused = self.focused.filter(|_| captured)?;
            return parts.get_mut(focused)?.key(key, reach).then_some(focused);
        }
        if key != ViewerKey::Tab {
            return None;
        }
        let focusable: Vec<usize> = (0..parts.len()).filter(|&i| parts[i].in_tab_order()).collect();
        if focusable.is_empty() {
            return None;
        }
        let at = self.focused.and_then(|f| focusable.iter().position(|&i| i == f));
        let n = focusable.len();
        let next = match (at, reach.extend) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(a), false) => (a + 1) % n,
            (Some(a), true) => (a + n - 1) % n,
        };
        let to = focusable[next];
        // Nowhere else to go (one part, already focused): Tab isn't used.
        if self.focused == Some(to) {
            return None;
        }
        self.focus(parts, Some(to));
        Some(to)
    }

    /// A character key with nothing to type into: to the part taking the keys.
    pub fn char_key(&mut self, parts: Parts<'_, '_>, key: char) -> Option<usize> {
        let i = self.keys_to(parts)?;
        parts.get_mut(i)?.char_key(key).then_some(i)
    }

    /// Text pasted: to the part taking the keys (passed on as typed text is). `Some(part)`
    /// when taken.
    pub fn paste(&mut self, parts: Parts<'_, '_>, text: &str) -> Option<usize> {
        let i = self.keys_to(parts)?;
        if parts.get_mut(i)?.paste(text) {
            return Some(i);
        }
        // As typed text: one over everything that takes none passes it to the focused one.
        let focused = self.focused.filter(|f| *f != i)?;
        parts.get_mut(focused)?.paste(text).then_some(focused)
    }

    /// What the focused part has selected, asked from `&self`: `part` gives a part by its
    /// place read-only (a view's own fields; `None` for those it can't lend so).
    pub fn selected_text_with<'p>(
        &self,
        part: impl Fn(usize) -> Option<&'p dyn Control>,
    ) -> Option<String> {
        part(self.focused?)?.selected_text()
    }

    /// Text typed: to the part taking the keys (one over everything that takes none passes
    /// it on to the focused one). `Some(part)` when taken.
    pub fn text(&mut self, parts: Parts<'_, '_>, typed: &str) -> Option<usize> {
        let i = self.keys_to(parts)?;
        if parts.get_mut(i)?.text(typed) {
            return Some(i);
        }
        let focused = self.focused.filter(|f| *f != i)?;
        parts.get_mut(focused)?.text(typed).then_some(focused)
    }

    /// What the part taking the keys has selected.
    pub fn selected_text(&self, parts: &[&mut dyn Control]) -> Option<String> {
        parts.get(self.keys_to(parts)?)?.selected_text()
    }

    /// An edit command for the part taking the keys (one over everything that doesn't take
    /// it passes it on to the focused one: ⌘Z for the field under its suggestions).
    /// `Some(part)` when taken.
    pub fn command(&mut self, parts: Parts<'_, '_>, id: &str) -> Option<usize> {
        let i = self.keys_to(parts)?;
        if parts.get_mut(i)?.command(id) {
            return Some(i);
        }
        let focused = self.focused.filter(|f| *f != i)?;
        parts.get_mut(focused)?.command(id).then_some(focused)
    }

    /// The tooltip at `p`: the topmost part's there that has one.
    pub fn tooltip(&self, parts: &[&mut dyn Control], p: Point) -> Option<Tip> {
        parts.iter().filter(|c| c.hit(p)).find_map(|c| c.tooltip(p))
    }

    /// The right-click menu at `p`: the topmost part's there that has one.
    pub fn menu(&mut self, parts: Parts<'_, '_>, p: Point) -> Option<(usize, Vec<ViewMenuEntry>)> {
        (0..parts.len()).find_map(|i| {
            if !parts[i].hit(p) {
                return None;
            }
            parts[i].menu(p).map(|m| (i, m))
        })
    }

    /// One of the parts is moving.
    pub fn animating(parts: &[&mut dyn Control]) -> bool {
        parts.iter().any(|c| c.animating())
    }

    /// When a part next needs a frame at its own pace: the soonest.
    pub fn frame_deadline(parts: &[&mut dyn Control]) -> Option<Instant> {
        parts.iter().filter_map(|c| c.frame_deadline()).min()
    }
}

/// A one-line field as a form's control: a press puts the caret (two select a word, three
/// all of it) and a drag selects; with the focus it takes the text typed, the editing keys
/// and the edit commands.
impl Control for ui_kit::TextInput {
    fn hit(&self, p: Point) -> bool {
        self.contains(p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        ui_kit::TextInput::press(self, p.x, clicks);
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.extend_to(p.x);
        true
    }

    fn release(&mut self) -> bool {
        false
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Text)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let KeyReach { word, extend } = reach;
        match key {
            ViewerKey::Backspace => self.backspace(word),
            ViewerKey::Delete => self.delete(),
            ViewerKey::Left => self.move_left(word, extend),
            ViewerKey::Right => self.move_right(word, extend),
            ViewerKey::Home => self.home(extend),
            ViewerKey::End => self.end(extend),
            _ => return false,
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.insert(&kernel::text::pasted_line(typed));
        true
    }

    fn selected_text(&self) -> Option<String> {
        Some(ui_kit::TextInput::selected_text(self).to_owned()).filter(|s| !s.is_empty())
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => self.select_all(),
            "cut" => self.insert(""),
            "undo" => return self.undo(),
            "redo" => return self.redo(),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/control.rs"]
mod tests;
