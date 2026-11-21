//! The studio's parts as a form's controls (see [`Studio::with_controls`]), each with the
//! behavior it has here: the color picker (over everything while open, its keys too), the
//! sliders (held while dragged, stepped by the arrows once pressed), what an effect's menu
//! or the gallery lets through (a press elsewhere closes the menu, or does nothing), the
//! name's field (a press puts the caret, the keys move and erase one character, Enter and
//! Esc let go of them), the search (the app's search field: its press, drag, editing keys,
//! ⌘A / ⌘X / ⌘Z and ×; ↵ shows its first result, Esc empties it and lets go), an effect's
//! handle (held while dragged).
//! What a part asks of the studio (a slider's stop, the search's first result, a color
//! picked) is written to the cell it's given; the studio does it after.

use kernel::{Point, Rect};
use ui_components::control::{Control, Press};
use ui_components::search_field::SearchField;
use ui_components::tool_panel::KeyReach;
use ui_components::{SliderArea, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

use crate::rows::Row;
use crate::{Hit, effects};

/// The edit command choosing a whole field.
const SELECT_ALL: &str = "select_all";

/// The topmost of `hits` at `p`.
fn hit_at(hits: &[(Rect, Hit)], p: Point) -> Option<&Hit> {
    hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h)
}

/// What a slider asks of the studio.
#[derive(Debug, Clone)]
pub(crate) enum SliderAsk {
    /// Its stop nearest `x` chosen.
    Slide(SliderArea, Row, f32),
    /// Stepped by `delta` stops (a bigger step with ⇧).
    Step(Row, i32, bool),
}

/// The sliders where the panel shows them: pressed, one is held (dragged) and its stop
/// chosen at once; with the keys afterwards (the one pressed last), the arrows step it.
#[derive(Debug)]
pub(crate) struct SliderPart<'a> {
    pub sliders: &'a [(SliderArea, Row)],
    /// The panel showing them (a slider scrolled out of it isn't pressed).
    pub list: Rect,
    pub held: &'a mut Option<Row>,
    /// The one the keys step (pressed last).
    pub focused: &'a mut Option<Row>,
    pub asked: &'a mut Option<SliderAsk>,
}

impl Control for SliderPart<'_> {
    fn hit(&self, p: Point) -> bool {
        self.list.contains(p) && self.sliders.iter().any(|(a, _)| a.rect.contains(p))
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let Some((area, row)) = self.sliders.iter().find(|(a, _)| a.rect.contains(p)) else {
            return Press::Ignored;
        };
        *self.held = Some(*row);
        *self.focused = Some(*row);
        *self.asked = Some(SliderAsk::Slide(area.clone(), *row, p.x));
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(row) = *self.held else { return false };
        let Some((area, _)) = self.sliders.iter().find(|(_, r)| *r == row) else { return false };
        *self.asked = Some(SliderAsk::Slide(area.clone(), row, p.x));
        true
    }

    fn release(&mut self) -> bool {
        self.held.take().is_some()
    }

    fn focusable(&self) -> bool {
        true
    }

    fn set_focus(&mut self, focused: bool) {
        if !focused {
            *self.focused = None;
        }
    }

    /// ← ↓ a stop down, → ↑ up (⇧: a bigger step); Esc is the studio's (a menu, the
    /// gallery close first).
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(row) = *self.focused else { return false };
        let delta = match key {
            ViewerKey::Left | ViewerKey::Down => -1,
            ViewerKey::Right | ViewerKey::Up => 1,
            _ => return false,
        };
        *self.asked = Some(SliderAsk::Step(row, delta, reach.extend));
        true
    }
}

/// What the open gallery lets through: a press anywhere else does nothing (the gallery
/// stays: it closes by ×, Esc or a choice).
#[derive(Debug)]
pub(crate) struct Catch<'a> {
    pub hits: &'a [(Rect, Hit)],
    pub gallery_open: bool,
}

impl Catch<'_> {
    /// A press at `p` isn't on the open gallery's parts.
    fn outside_gallery(&self, p: Point) -> bool {
        self.gallery_open
            && !matches!(
                hit_at(self.hits, p),
                Some(Hit::Gallery(_) | Hit::Choice(Row::Add | Row::Preset, _))
            )
    }
}

impl Control for Catch<'_> {
    fn hit(&self, p: Point) -> bool {
        self.outside_gallery(p)
    }

    fn press(&mut self, _p: Point, _clicks: u32) -> Press {
        Press::Took
    }
}

/// What a field's key asks of the studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldAsk {
    /// It lets go of the keys.
    Leave,
    /// The search's first result shown.
    Jump,
    /// The search typed into: its results from the top.
    Searched,
}

/// The name's field: hit where it's the topmost hit (`own`); a press puts the caret; ⌫ ← →
/// edit one character at a time; it lets go of the keys on ↵ and Esc.
#[derive(Debug)]
pub(crate) struct FieldPart<'a> {
    pub input: &'a mut TextInput,
    pub own: Hit,
    pub hits: &'a [(Rect, Hit)],
    pub asked: &'a mut Option<FieldAsk>,
}

impl Control for FieldPart<'_> {
    fn hit(&self, p: Point) -> bool {
        hit_at(self.hits, p) == Some(&self.own)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        self.input.click(p.x);
        Press::Took
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
        match key {
            ViewerKey::Backspace => self.input.backspace(false),
            ViewerKey::Left => self.input.move_left(false, false),
            ViewerKey::Right => self.input.move_right(false, false),
            ViewerKey::Enter | ViewerKey::Escape => *self.asked = Some(FieldAsk::Leave),
            _ => return false,
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.input.insert(typed);
        true
    }

    fn command(&mut self, id: &str) -> bool {
        if id != SELECT_ALL {
            return false;
        }
        self.input.select_all();
        true
    }
}

/// The search across every setting: hit where it's the topmost hit; the app's search field
/// otherwise (see the module's doc), what changes its text asking for its results from the
/// top.
#[derive(Debug)]
pub(crate) struct SearchPart<'a> {
    pub field: &'a mut SearchField,
    pub hits: &'a [(Rect, Hit)],
    pub asked: &'a mut Option<FieldAsk>,
}

impl Control for SearchPart<'_> {
    fn hit(&self, p: Point) -> bool {
        hit_at(self.hits, p) == Some(&Hit::Search)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let press = Control::press(&mut *self.field, p, clicks);
        // × emptied it: what it finds from the top.
        if self.field.take_cleared() {
            *self.asked = Some(FieldAsk::Searched);
        }
        press
    }

    fn drag(&mut self, p: Point) -> bool {
        Control::drag(&mut *self.field, p)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(&mut *self.field, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.field, p)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter => *self.asked = Some(FieldAsk::Jump),
            ViewerKey::Escape => {
                self.field.set_text("");
                *self.asked = Some(FieldAsk::Leave);
            }
            _ => {
                let erases = matches!(key, ViewerKey::Backspace | ViewerKey::Delete);
                return self.edited(erases, |f| Control::key(f, key, reach));
            }
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.edited(true, |f| Control::text(f, typed))
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.field)
    }

    fn command(&mut self, id: &str) -> bool {
        self.edited(id != SELECT_ALL, |f| Control::command(f, id))
    }
}

impl SearchPart<'_> {
    /// `edit` on the field; used, and `changes` its text (typing, erasing, ⌘X, ⌘Z), its
    /// results from the top.
    fn edited(&mut self, changes: bool, edit: impl FnOnce(&mut SearchField) -> bool) -> bool {
        let used = edit(self.field);
        if used && changes {
            *self.asked = Some(FieldAsk::Searched);
        }
        used
    }
}

/// An effect's handle: pressed, its card is held and follows the pointer up or down (where
/// it's dropped is the studio's, at the release).
#[derive(Debug)]
pub(crate) struct HandlePart<'a> {
    pub hits: &'a [(Rect, Hit)],
    pub drag: &'a mut Option<effects::Drag>,
}

impl Control for HandlePart<'_> {
    fn hit(&self, p: Point) -> bool {
        matches!(hit_at(self.hits, p), Some(Hit::Handle(_)))
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let Some(&Hit::Handle(layer)) = hit_at(self.hits, p) else { return Press::Ignored };
        *self.drag = Some(effects::Drag { layer, y: p.y });
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(drag) = self.drag.as_mut() else { return false };
        drag.y = p.y;
        true
    }
}

#[cfg(test)]
#[path = "../tests/unit/parts.rs"]
mod tests;
