//! The log's own parts as a form's controls: the filter bar's field typed in (over the
//! other parts' focus), the Date filter's picker (Space picks the day its keys are on), the
//! separator between the details' files and the commit (a double click puts it back).

use kernel::Point;
use ui_components::control::{Control, Press};
use ui_components::date_range::RangePart;
use ui_components::search_field::SearchField;
use ui_components::{KeyReach, Splitter, ViewerCursor, ViewerKey};

use super::filters::Field;

/// What the bar's field did that the view follows (its filters, its lists).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldAnswer {
    /// Its text changed: what it filters follows.
    Typed(Field),
    /// ↵: the text applied now, the list's row chosen, the first branch shown.
    Entered(Field),
    /// ↑ ↓ in the open list: its row with the keys moved by this many.
    Stepped(isize),
    /// Esc with a list open: it closes.
    Closed,
}

/// The filter bar's field being typed in ("Text or hash", the open list's search, the
/// branches' filter: which one is `focus`). While one is, it takes the keys, the text and
/// the edit commands over the part with the focus (the files' tree keeps it: a file's menu
/// asked for, or a filter's list opened from a command, leaves the typing as it was) — but
/// the text while the files' search has it (`text_elsewhere`), and what it doesn't use, go
/// on to that part. Esc leaves it (a list open: closes it first); the focus stays where it
/// is. What the view follows is written to `answer`.
pub(super) struct BarField<'a> {
    pub(super) focus: &'a mut Option<Field>,
    pub(super) text: &'a mut SearchField,
    pub(super) branches: &'a mut SearchField,
    /// The open list's search (`None`: no list open).
    pub(super) query: Option<&'a mut SearchField>,
    /// The Date picker is open: it has the keys, over the field.
    pub(super) picker_open: bool,
    /// The files' search has the text typed (their tree has the focus and searches).
    pub(super) text_elsewhere: bool,
    pub(super) answer: &'a mut Option<FieldAnswer>,
}

impl BarField<'_> {
    /// The field typed in (`None`: none, or the list's search with no list).
    fn input(&mut self) -> Option<&mut SearchField> {
        match (*self.focus)? {
            Field::Text => Some(&mut *self.text),
            Field::Branches => Some(&mut *self.branches),
            Field::Query => self.query.as_deref_mut(),
        }
    }
}

impl Control for BarField<'_> {
    fn captures_keys(&self) -> bool {
        self.focus.is_some() && !self.picker_open
    }

    /// Editing, `↑ ↓` among the open list's rows, `↵` choosing (the text: applied now), Esc
    /// closing the list or leaving the field; every other key is kept (nothing behind the
    /// field moves while it's typed in).
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(field) = *self.focus else { return false };
        let in_list = field == Field::Query && self.query.is_some();
        match key {
            ViewerKey::Escape => {
                if self.query.is_some() {
                    *self.answer = Some(FieldAnswer::Closed);
                } else {
                    *self.focus = None;
                }
                return true;
            }
            ViewerKey::Enter => {
                *self.answer = Some(FieldAnswer::Entered(field));
                return true;
            }
            ViewerKey::Up | ViewerKey::Down if in_list => {
                let by = if key == ViewerKey::Up { -1 } else { 1 };
                *self.answer = Some(FieldAnswer::Stepped(by));
                return true;
            }
            _ => {}
        }
        let Some(input) = self.input() else { return false };
        Control::key(input, key, reach);
        if input.take_edited() {
            *self.answer = Some(FieldAnswer::Typed(field));
        }
        true
    }

    /// Text typed, on one line (the search field's own way), unless the files' search has it.
    fn text(&mut self, typed: &str) -> bool {
        let Some(field) = *self.focus else { return false };
        if self.text_elsewhere {
            return false;
        }
        let Some(input) = self.input() else { return false };
        let took = Control::text(input, typed);
        if input.take_edited() {
            *self.answer = Some(FieldAnswer::Typed(field));
        }
        took
    }

    /// The app's ⌘A, ⌘X, ⌘Z, ⇧⌘Z for the field typed in (taken even with nothing to undo).
    fn command(&mut self, id: &str) -> bool {
        let Some(field) = *self.focus else { return false };
        let Some(input) = self.input() else { return false };
        Control::command(input, id);
        if input.take_edited() {
            *self.answer = Some(FieldAnswer::Typed(field));
        }
        matches!(id, "select_all" | "cut" | "undo" | "redo")
    }
}

/// The open Date picker: the shared one ([`RangePart`]), hit anywhere; Space typed picks the
/// day its keys are on when its fields don't take it; over it, the text's cursor on its
/// fields and a hand elsewhere; outside it, the arrow.
pub(super) struct DatePart<'a> {
    pub(super) part: RangePart<'a>,
}

impl Control for DatePart<'_> {
    fn hit(&self, p: Point) -> bool {
        self.part.hit(p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        self.part.press(p, clicks)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.part.hover(p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        let picker = &self.part.picker;
        Some(match (picker.rect().contains(p), picker.over_field(p)) {
            (false, _) => ViewerCursor::Arrow,
            (true, true) => ViewerCursor::Text,
            (true, false) => ViewerCursor::Pointer,
        })
    }

    fn focusable(&self) -> bool {
        self.part.focusable()
    }

    fn keeps_tab(&self) -> bool {
        self.part.keeps_tab()
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.part.key(key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        self.part.text(typed) || (typed == " " && self.part.picker.space())
    }

    fn selected_text(&self) -> Option<String> {
        self.part.selected_text()
    }

    fn command(&mut self, id: &str) -> bool {
        self.part.command(id)
    }
}

/// The separator between the details' files and the commit: pressed, it's held and the
/// pointer moves it (where to, physical pixels, written to `at`); two clicks put it back
/// (`reset`).
pub(super) struct FilesSeam<'a> {
    pub(super) splitter: &'a mut Splitter,
    pub(super) at: &'a mut Option<f32>,
    pub(super) reset: &'a mut bool,
}

impl Control for FilesSeam<'_> {
    fn hit(&self, p: Point) -> bool {
        self.splitter.contains(p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        if clicks >= 2 {
            *self.reset = true;
            return Press::Took;
        }
        self.splitter.begin_drag(p);
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(y) = self.splitter.drag_to(p) else { return false };
        *self.at = Some(y);
        true
    }

    fn release(&mut self) -> bool {
        self.splitter.end_drag()
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        self.splitter.hover(p.unwrap_or(ui_components::control::NOWHERE))
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::ResizeVertical)
    }

    fn animating(&self) -> bool {
        self.splitter.is_animating()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/log_view/parts.rs"]
mod tests;
