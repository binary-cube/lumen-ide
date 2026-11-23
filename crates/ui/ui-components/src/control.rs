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
        self.cursor = parts.get(i).and_then(|c| c.cursor(p));
        Some((i, moved))
    }

    /// The pointer let go: the part that held it, and whether it changed.
    pub fn release(&mut self, parts: Parts<'_, '_>) -> Option<(usize, bool)> {
        let i = self.held.take()?;
        Some((i, parts.get_mut(i)?.release()))
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
        self.cursor = p.and_then(|p| self.cursor(parts, p));
        self.tip = p.and_then(|p| self.tooltip(parts, p));
        changed
    }

    /// The cursor where the mouse last was ([`Self::hover`]), when `p` is there.
    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.cursor.filter(|_| self.mouse == Some(p))
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
        parts.iter().filter(|c| c.hit(p)).find_map(|c| c.cursor(p))
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

/// A press on an edge scrollbar: its thumb held, the offset it gives handed to `set` (the
/// parts below differ only in where the offset goes).
fn bar_press(bar: &mut crate::scrollbar::EdgeScrollbar, p: Point, set: impl FnOnce(f32)) -> Press {
    match bar.press(p) {
        Some(offset) => {
            set(offset);
            Press::Held
        }
        None => Press::Ignored,
    }
}

/// `offset` moved to `to`: whether it changed.
fn move_to(offset: &mut f32, to: f32) -> bool {
    let changed = to != *offset;
    *offset = to;
    changed
}

/// A scrollbar and the offset it moves, as one control: pressed on its lane, its thumb is
/// held (or the offset jumps there) and follows the pointer.
#[derive(Debug)]
pub struct Thumb<'a> {
    pub bar: &'a mut crate::scrollbar::EdgeScrollbar,
    pub offset: &'a mut f32,
}

impl Control for Thumb<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        bar_press(self.bar, p, |offset| *self.offset = offset)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.bar.drag(p).is_some_and(|offset| move_to(self.offset, offset))
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.bar.next_frame()
    }
}

/// A scrollbar and the [`crate::ScrollState`] it moves, as one control (as [`Thumb`] for
/// a list whose scroll keeps its own bounds).
#[derive(Debug)]
pub struct ScrollThumb<'a> {
    pub bar: &'a mut crate::scrollbar::EdgeScrollbar,
    pub scroll: &'a mut crate::ScrollState,
}

impl Control for ScrollThumb<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        bar_press(self.bar, p, |offset| {
            self.scroll.scroll_to(offset);
        })
    }

    fn drag(&mut self, p: Point) -> bool {
        self.bar.drag(p).is_some_and(|offset| self.scroll.scroll_to(offset))
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.bar.next_frame()
    }
}

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

/// A scrolled area with its bar: the bar's thumb held (or the offset jumps where its track
/// is pressed), the wheel anywhere over `area`, the offset kept within `0..=max`.
#[derive(Debug)]
pub struct Scrolled<'a> {
    pub bar: &'a mut crate::scrollbar::EdgeScrollbar,
    pub offset: &'a mut f32,
    pub area: Rect,
    pub max: f32,
}

impl Control for Scrolled<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p) || self.area.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let max = self.max.max(0.0);
        bar_press(self.bar, p, |offset| *self.offset = offset.clamp(0.0, max))
    }

    fn drag(&mut self, p: Point) -> bool {
        let max = self.max.max(0.0);
        self.bar.drag(p).is_some_and(|offset| move_to(self.offset, offset.clamp(0.0, max)))
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let to = (*self.offset + dy).clamp(0.0, self.max.max(0.0));
        move_to(self.offset, to)
    }

    fn frame_deadline(&self) -> Option<Instant> {
        self.bar.next_frame()
    }
}

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

/// A scrollbar drawn with its own metrics (across a code block, along a column) and the
/// offset it moves, as one control.
#[derive(Debug)]
pub struct BarThumb<'a> {
    pub bar: &'a mut crate::scrollbar::Scrollbar,
    pub metrics: crate::scrollbar::ScrollMetrics,
    pub offset: &'a mut f32,
}

impl BarThumb<'_> {
    /// `offset` kept within what can be scrolled.
    fn within(&self, offset: f32) -> f32 {
        let m = self.metrics;
        offset.clamp(0.0, (m.content - m.viewport).max(0.0))
    }
}

impl Control for BarThumb<'_> {
    fn hit(&self, p: Point) -> bool {
        self.bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        match self.bar.begin_drag(p, self.metrics) {
            Some(offset) => {
                *self.offset = self.within(offset);
                Press::Held
            }
            None => Press::Ignored,
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag_to(p, self.metrics) else { return false };
        let offset = self.within(offset);
        move_to(self.offset, offset)
    }

    fn release(&mut self) -> bool {
        self.bar.end_drag()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.bar.hover(p.unwrap_or(NOWHERE))
    }
}

/// A separator between panels as a control: pressed on it, it's held and the pointer moves
/// it; where its line's center goes is written to `at` (the owner turns it into a size).
#[derive(Debug)]
pub struct Divider<'a> {
    pub splitter: &'a mut crate::splitter::Splitter,
    pub at: &'a mut Option<f32>,
}

impl Control for Divider<'_> {
    fn hit(&self, p: Point) -> bool {
        self.splitter.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        if self.splitter.begin_drag(p) { Press::Held } else { Press::Ignored }
    }

    fn drag(&mut self, p: Point) -> bool {
        let to = self.splitter.drag_to(p);
        let changed = to.is_some() && to != *self.at;
        *self.at = to;
        changed
    }

    fn release(&mut self) -> bool {
        self.splitter.end_drag()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.splitter.hover(p.unwrap_or(NOWHERE))
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(match self.splitter.orientation() {
            crate::splitter::Orientation::Vertical => ViewerCursor::ResizeHorizontal,
            crate::splitter::Orientation::Horizontal => ViewerCursor::ResizeVertical,
        })
    }

    fn animating(&self) -> bool {
        self.splitter.is_animating()
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

#[cfg(test)]
#[path = "../tests/unit/control.rs"]
mod tests;
