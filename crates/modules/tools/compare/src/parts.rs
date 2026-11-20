//! The comparison's parts as a form's controls: the field written by hand, each column's
//! bar across, the buttons (and a difference's rows), the separators between the columns.
//! The view hands them to its [`Controls`] in that order (the topmost first) and reads back
//! what they did; nothing that routes names one.

use kernel::{Point, Rect};
use ui_components::control::{BarThumb, Control, Controls, Divider, Press, Shown};
use ui_components::{KeyReach, MultilineField, Scrollbar, ViewerCursor, ViewerKey};

use super::{Across, Compare, Hit};

/// The field written by hand, first among the parts.
pub(crate) const FIELD: usize = 0;
/// The buttons (and a difference's rows), after the field and the three bars across.
pub(crate) const BUTTONS: usize = 4;
/// The separators between the columns, last.
pub(crate) const DIVIDERS: [usize; 2] = [5, 6];

/// The result written by hand, as a control, the way it has always answered here: a press
/// puts the caret (one click, never extending), a drag selects, the wheel over it scrolls
/// only it; while it's open it has the keys (Tab included: nothing moves the focus away),
/// the text typed and the edit commands. Its scrollbar answers only a press (it doesn't
/// widen under the mouse).
struct ByHand<'a> {
    field: Option<&'a mut MultilineField>,
}

impl Control for ByHand<'_> {
    fn hit(&self, p: Point) -> bool {
        self.field.as_ref().is_some_and(|f| f.contains(p))
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        let Some(field) = &mut self.field else { return Press::Ignored };
        field.press(p, 1, false);
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.field.as_mut().is_some_and(|f| f.drag(p))
    }

    fn release(&mut self) -> bool {
        self.field.as_mut().is_some_and(|f| f.release())
    }

    /// The wheel over it never reaches the columns behind it.
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let Some(field) = &mut self.field else { return false };
        field.scroll_by(dy);
        true
    }

    fn focusable(&self) -> bool {
        self.field.is_some()
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(field) = &mut self.field else { return false };
        Control::key(&mut **field, key, reach)
    }

    fn text(&mut self, typed: &str) -> bool {
        let Some(field) = &mut self.field else { return false };
        field.insert(typed);
        true
    }

    fn command(&mut self, id: &str) -> bool {
        let Some(field) = &mut self.field else { return false };
        match id {
            "select_all" => field.select_all(),
            "cut" => field.insert(""),
            "undo" => {
                field.undo();
            }
            "redo" => {
                field.redo();
            }
            _ => return false,
        }
        true
    }
}

/// The places drawn last (the buttons, a difference's rows) as one control over the
/// separators: a button pressed is kept in `pressed` for the view (a difference's rows let
/// the press go on: a separator over them is grabbed); the mouse over any of them leaves a
/// separator unlit, the pointer's hand shown there.
struct Buttons<'a> {
    hits: &'a [(Rect, Hit)],
    pressed: &'a mut Option<Hit>,
}

impl Buttons<'_> {
    /// The topmost place at `p`.
    fn at(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }
}

impl Control for Buttons<'_> {
    fn hit(&self, p: Point) -> bool {
        self.at(p).is_some()
    }

    fn press(&mut self, p: Point, _clicks: u32) -> Press {
        match self.at(p) {
            None | Some(Hit::Select(_)) => Press::Ignored,
            hit => {
                *self.pressed = hit;
                Press::Took
            }
        }
    }

    fn cursor(&self, _p: Point) -> Option<ViewerCursor> {
        Some(ViewerCursor::Pointer)
    }
}

/// A column's bar across as a thumb over its offset, and whether it's drawn (a column whose
/// lines all fit has none: `spare` stands in, never hit).
fn across_thumb<'a>(across: &'a mut Across, spare: &'a mut Scrollbar) -> (BarThumb<'a>, bool) {
    let metrics = across.metrics();
    let Across { bar, offset, .. } = across;
    let shown = bar.is_some();
    (BarThumb { bar: bar.as_mut().unwrap_or(spare), metrics, offset }, shown)
}

impl Compare {
    /// Its parts (see the module's doc), with their state of use. While the field written by
    /// hand is open it has the focus; a separator moved resizes its columns.
    pub(crate) fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut spare: [Scrollbar; 3] = Default::default();
        let mut at: [Option<f32>; 2] = [None, None];
        let answer = {
            let field = self.resolving.as_mut().and_then(|r| r.by_hand.as_mut());
            let mut field = ByHand { field };
            let [a0, a1, a2] = &mut self.across;
            let [s0, s1, s2] = &mut spare;
            let (mut t0, v0) = across_thumb(a0, s0);
            let (mut t1, v1) = across_thumb(a1, s1);
            let (mut t2, v2) = across_thumb(a2, s2);
            let mut b0 = Shown { part: &mut t0, shown: v0 };
            let mut b1 = Shown { part: &mut t1, shown: v1 };
            let mut b2 = Shown { part: &mut t2, shown: v2 };
            let mut buttons = Buttons { hits: &self.hits, pressed: &mut self.pressed };
            let [d0, d1] = &mut self.splitters;
            let [at0, at1] = &mut at;
            let mut d0 = Divider { splitter: d0, at: at0 };
            let mut d1 = Divider { splitter: d1, at: at1 };
            let mut parts: [&mut dyn Control; 7] =
                [&mut field, &mut b0, &mut b1, &mut b2, &mut buttons, &mut d0, &mut d1];
            let open = parts[FIELD].focusable();
            if open != (self.controls.focused() == Some(FIELD)) {
                self.controls.focus(&mut parts, open.then_some(FIELD));
            }
            f(&mut self.controls, &mut parts)
        };
        for (i, x) in at.into_iter().enumerate() {
            if let Some(x) = x {
                self.drag_separator(i, x);
            }
        }
        answer
    }
}
