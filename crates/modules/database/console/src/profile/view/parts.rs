//! The view's own parts as controls: the steps' filter having the keys, a side panel's
//! list (the findings' cards, a step's details) with its scrollbar.

use super::*;

/// The steps' filter as the part having the keys: the app's search field (its press, drag,
/// editing keys, ⌘A / ⌘X / ⌘Z, × emptying it) while it's `typing`; before that, a character
/// typed with nothing else to take it starts it. Esc empties it and gives the keys back, ↵
/// gives them back, the arrows and the pages give them back to the steps. What it changed
/// is said back (`edited`; the view filters).
pub(super) struct FilterPart<'a> {
    pub(super) field: &'a mut SearchField,
    pub(super) typing: &'a mut bool,
    pub(super) edited: &'a mut bool,
}

impl FilterPart<'_> {
    /// Runs `edit` on the field, noting whether its text changed.
    pub(super) fn editing(&mut self, edit: impl FnOnce(&mut SearchField) -> bool) -> bool {
        let used = edit(&mut *self.field);
        *self.edited |= self.field.take_edited();
        used
    }
}

impl Control for FilterPart<'_> {
    /// Where it was drawn as a field (none while it's an icon alone).
    fn area(&self) -> Option<Rect> {
        Control::area(&*self.field)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let press = Control::press(self.field, p, clicks);
        // Its × emptied it and let the keys go; elsewhere its caret is put.
        let cleared = self.field.take_cleared();
        *self.typing = !cleared;
        *self.edited |= cleared | self.field.take_edited();
        press
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(self.field, p)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(self.field, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.field, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    /// Focused from code only: Tab doesn't stop at it.
    fn in_tab_order(&self) -> bool {
        false
    }

    /// Tab while it's typed into stays its own (nothing else to go to).
    fn keeps_tab(&self) -> bool {
        *self.typing
    }

    /// Any but a control character or a space starts it.
    fn char_key(&mut self, key: char) -> bool {
        if key.is_control() || key == ' ' {
            return false;
        }
        *self.typing = true;
        let mut buf = [0; 4];
        let typed: &str = key.encode_utf8(&mut buf);
        self.editing(|f| Control::text(f, typed))
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if !*self.typing {
            return false;
        }
        match key {
            ViewerKey::Escape => {
                *self.edited |= !SearchField::text(self.field).is_empty();
                self.field.set_text("");
                *self.typing = false;
            }
            ViewerKey::Enter => *self.typing = false,
            ViewerKey::Up | ViewerKey::Down | ViewerKey::PageUp | ViewerKey::PageDown => {
                *self.typing = false;
                return false;
            }
            _ => return self.editing(|f| Control::key(f, key, reach)),
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        *self.typing && self.editing(|f| Control::text(f, typed))
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.field).filter(|_| *self.typing)
    }

    fn command(&mut self, id: &str) -> bool {
        *self.typing && self.editing(|f| Control::command(f, id))
    }
}

/// What a side panel's list (the findings' cards, a step's details) answers as a part of the
/// view.
pub(super) trait PanelRows {
    /// What a press on it hits.
    type Hit;
    fn thumb(&mut self) -> Thumb<'_>;
    fn hit_at(&self, p: Point) -> Option<Self::Hit>;
    /// The mouse at `p` (anywhere: off it, nothing lit): `true` if what it shows changed.
    fn hover_at(&mut self, p: Point) -> bool;
    fn scroll_by(&mut self, dy: f32) -> bool;
    fn tip_at(&self, p: Point) -> Option<Tip>;
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor>;
}

impl PanelRows for FindingsList {
    type Hit = FindingHit;
    fn thumb(&mut self) -> Thumb<'_> {
        FindingsList::thumb(self)
    }
    fn hit_at(&self, p: Point) -> Option<FindingHit> {
        self.hit(p)
    }
    fn hover_at(&mut self, p: Point) -> bool {
        self.hover(p)
    }
    fn scroll_by(&mut self, dy: f32) -> bool {
        FindingsList::scroll_by(self, dy)
    }
    fn tip_at(&self, p: Point) -> Option<Tip> {
        self.tooltip_at(p)
    }
    /// A card and its buttons are pressed.
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.hit(p).map(|_| ViewerCursor::Pointer)
    }
}

impl PanelRows for DetailsList {
    type Hit = DetailHit;
    fn thumb(&mut self) -> Thumb<'_> {
        DetailsList::thumb(self)
    }
    fn hit_at(&self, p: Point) -> Option<DetailHit> {
        self.hit(p)
    }
    fn hover_at(&mut self, p: Point) -> bool {
        self.hover(p)
    }
    fn scroll_by(&mut self, dy: f32) -> bool {
        DetailsList::scroll_by(self, dy)
    }
    fn tip_at(&self, p: Point) -> Option<Tip> {
        self.tooltip_at(p)
    }
    /// Its rows are copied by a press, without a hand over them.
    fn cursor_at(&self, _p: Point) -> Option<ViewerCursor> {
        None
    }
}

/// A side panel as a part of the view, there while it's drawn: a press on its scrollbar
/// holds the thumb, elsewhere on it what its list hit is kept for the view; the wheel over
/// it is its list's (`scrolled`: it moved), never the body's under it.
pub(super) struct Panel<'a, L: PanelRows> {
    pub(super) rows: &'a mut L,
    /// Where it was drawn this frame (empty: not drawn).
    pub(super) area: Rect,
    pub(super) pressed: &'a mut Option<L::Hit>,
    pub(super) scrolled: &'a mut bool,
}

impl<L: PanelRows> Control for Panel<'_, L> {
    fn area(&self) -> Option<Rect> {
        Some(self.area).filter(|a| a.size.width > 0.0 && a.size.height > 0.0)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let mut thumb = self.rows.thumb();
        if thumb.hit(p) && thumb.press(p, clicks) == Press::Held {
            return Press::Held;
        }
        *self.pressed = self.rows.hit_at(p);
        Press::Took
    }

    fn drag(&mut self, p: Point) -> bool {
        self.rows.thumb().drag(p)
    }

    fn release(&mut self) -> bool {
        self.rows.thumb().release()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let bar = self.rows.thumb().hover(p);
        bar | self.rows.hover_at(p.unwrap_or(NOWHERE))
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        self.rows.cursor_at(p)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        *self.scrolled |= self.rows.scroll_by(dy);
        true
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        self.rows.tip_at(p)
    }
}
