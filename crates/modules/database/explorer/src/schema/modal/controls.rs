//! The modal's parts as controls: the open list (a choice's or a field's suggestions) over
//! everything, the focused field with the keys, and the order they're asked in.

use super::*;

/// What an open list is over the form, for the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listing {
    /// A choice's list: every key, its search the text.
    Choice,
    /// A field's suggestions: only what moves in them, takes or closes them.
    Suggest,
}

/// The open list as a part, over everything while it's shown (until the next paint
/// closes it, a list no longer asked for takes nothing): its press, its hover, its cursor
/// and its wheel are the picker's; of the keys, a choice's list takes every one and, in
/// its search, the text; a field's suggestions ↑ ↓ ⇞ ⇟, ↵ ⇥ on the lit one (taken) and esc
/// (closed) — the other keys and the text go on to the field, which keeps the typing.
struct ListPart<'a> {
    picker: &'a mut Picker,
    shown: bool,
    /// What it is for the keys (`None`: no list open).
    listing: Option<Listing>,
    picked: &'a mut Option<usize>,
    closed: &'a mut bool,
}

impl ListPart<'_> {
    /// The picker as the part shown only while asked for.
    pub(super) fn shown(&mut self) -> Shown<'_> {
        Shown { part: &mut *self.picker, shown: self.shown }
    }
}

impl Control for ListPart<'_> {
    fn area(&self) -> Option<Rect> {
        Control::area(&*self.picker).filter(|_| self.shown)
    }

    fn hit(&self, p: Point) -> bool {
        self.shown && Control::hit(&*self.picker, p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> ui_components::control::Press {
        self.shown().press(p, clicks)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.shown().drag(p)
    }

    fn release(&mut self) -> bool {
        self.shown().release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.shown().hover(p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.picker, p).filter(|_| self.shown)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.shown().scroll(dx, dy)
    }

    fn focusable(&self) -> bool {
        self.shown && Control::focusable(&*self.picker)
    }

    fn keeps_tab(&self) -> bool {
        self.shown && Control::keeps_tab(&*self.picker)
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        Control::frame_deadline(&*self.picker).filter(|_| self.shown)
    }

    fn captures_keys(&self) -> bool {
        self.listing.is_some()
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match self.listing {
            Some(Listing::Choice) => Control::key(self.picker, key, reach),
            Some(Listing::Suggest) => match key {
                ViewerKey::Down | ViewerKey::Up | ViewerKey::PageDown | ViewerKey::PageUp => {
                    Picker::key(self.picker, key, reach);
                    true
                }
                ViewerKey::Enter | ViewerKey::Tab => {
                    let lit = self.picker.highlighted();
                    *self.picked = lit;
                    lit.is_some()
                }
                ViewerKey::Escape => {
                    *self.closed = true;
                    true
                }
                _ => false,
            },
            None => false,
        }
    }

    fn text(&mut self, typed: &str) -> bool {
        self.listing == Some(Listing::Choice) && self.picker.text_input(typed)
    }
}

/// The focused field as the part having the keys (focused from code; a press on it is the
/// view's, by its hit): the editing keys and the text typed, but none while a choice's
/// list is open over it. `edited`: its text changed.
struct FieldKeys<'a> {
    /// `None`: no field focused.
    field: Option<Text>,
    input: Option<&'a mut ui_kit::TextInput>,
    /// It takes the keys (no choice's list over it).
    open: bool,
    edited: &'a mut bool,
}

impl Control for FieldKeys<'_> {
    fn focusable(&self) -> bool {
        true
    }

    /// Focused from code only: the view moves among its fields with Tab and ↑ ↓.
    fn in_tab_order(&self) -> bool {
        false
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let (true, Some(input)) = (self.open, self.input.as_deref_mut()) else { return false };
        match key {
            ViewerKey::Left => input.move_left(reach.word, reach.extend),
            ViewerKey::Right => input.move_right(reach.word, reach.extend),
            ViewerKey::Home => input.home(reach.extend),
            ViewerKey::End => input.end(reach.extend),
            ViewerKey::Backspace => input.backspace(reach.word),
            ViewerKey::Delete => input.delete(),
            _ => return false,
        }
        *self.edited |= matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
        true
    }

    /// Typed into the field (control characters dropped).
    fn text(&mut self, typed: &str) -> bool {
        if !self.open || self.field.is_none() {
            return false;
        }
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        if let Some(input) = self.input.as_deref_mut() {
            input.insert(&typed);
        }
        *self.edited = true;
        true
    }
}

/// The open list's and the focused field's places among the modal's parts.
pub(super) const LIST: usize = 0;

const FIELD: usize = 3;

impl SchemaView {
    /// Its parts (a choice's list or a field's suggestions while asked for, over
    /// everything, and its keys; the preview and its scrollbar; the form and its
    /// scrollbar: the rest of the modal; the focused field, having the keys), the topmost
    /// first, with their state of use. An open list's keys come first, those it doesn't
    /// use and the text go on to the field (its suggestions take ↑ ↓ ↵ ⇥ esc, the field
    /// keeps the typing). The view moves among its fields (Tab, ↑ ↓, a press on one): the
    /// field's part is always the focused one.
    pub(super) fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let listing = if self.choosing() {
            Some(Listing::Choice)
        } else if self.suggesting() {
            Some(Listing::Suggest)
        } else {
            None
        };
        let field = self.focus;
        let shown = self.popup.is_some();
        let mut list = ListPart {
            picker: &mut self.picker,
            shown,
            listing,
            picked: &mut self.heard.picked,
            closed: &mut self.heard.closed,
        };
        let input = field.and_then(|f| self.form.as_mut()?.input_mut(f));
        let mut field_keys = FieldKeys {
            field,
            input,
            open: listing != Some(Listing::Choice),
            edited: &mut self.heard.edited,
        };
        let mut preview = Scrolled {
            bar: &mut self.preview_bar,
            offset: &mut self.preview_scroll,
            area: self.preview_area,
            max: self.preview_max,
        };
        let mut form = Scrolled {
            bar: &mut self.form_bar,
            offset: &mut self.form_scroll,
            area: self.bounds,
            max: self.form_max,
        };
        let mut parts: [&mut dyn Control; 4] =
            [&mut list, &mut preview, &mut form, &mut field_keys];
        self.controls.focus(&mut parts, Some(FIELD));
        let answer = f(&mut self.controls, &mut parts);
        // A press on the open list doesn't take the keys from the field.
        self.controls.focus(&mut parts, Some(FIELD));
        answer
    }

    /// Its part at `i`, read-only (the copy's selection asked from `&self`): the focused
    /// field's.
    pub(super) fn part(&self, i: usize) -> Option<&dyn Control> {
        let field = self.focus.filter(|_| i == FIELD)?;
        Some(self.form.as_ref()?.input(field)? as &dyn Control)
    }

    /// What the keys' parts heard, acted on: a suggestion taken or closed, the field's
    /// text changed. `true` if a suggestion was taken or closed.
    pub(super) fn heard_keys(&mut self) -> bool {
        let heard = std::mem::take(&mut self.heard);
        if heard.edited
            && let Some(field) = self.focus
        {
            self.edited(field);
        }
        if let Some(at) = heard.picked {
            self.pick(at);
            return true;
        }
        if heard.closed {
            self.popup = None;
            self.picker.close();
            return true;
        }
        false
    }
}
