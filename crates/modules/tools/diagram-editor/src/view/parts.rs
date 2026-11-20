//! The view's parts as a form's controls: a number typed in the panel, the palette (its
//! wheel and its search), the text edited in place, then the zones whose presses the view
//! does itself (the bar, the panel, the quick bar, the palette's buttons, the page tabs) —
//! the topmost first. The one with the focus is always the first that has a field open (a
//! number typed wins over the search; a text and the other two never are open together),
//! so its keys, typing and edit commands go to it; what a part asks that needs the whole
//! view (an end, an add) is done after.

use i18n::t;
use kernel::{Point, Rect};
use ui_components::ViewerCursor;
use ui_components::control::{Control, Controls, Press};
use ui_components::tooltip::Tip;

use super::DiagramView;
use super::palette::PalettePart;
use super::properties::TypingPart;
use super::text::TextPart;

/// What a part asked of the view (it needs the whole diagram).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Asked {
    /// The text edited in place ends (its text set, one step).
    EndText,
    /// The number typed ends: set (`true`) or let go.
    EndTyping(bool),
    /// The first shape the palette's search found is added.
    AddFound,
}

/// The parts' places (see [`DiagramView::with_controls`]): the fields first.
pub(super) const TYPING: usize = 0;
pub(super) const PALETTE: usize = 1;
pub(super) const TEXT: usize = 2;
/// The zones after them, in the order a press tries them.
pub(super) const BAR: usize = 3;
pub(super) const PANEL: usize = 4;
pub(super) const QUICK: usize = 5;
pub(super) const PALETTE_BUTTONS: usize = 6;
pub(super) const TABS: usize = 7;

/// A zone whose press the view does itself (it needs the whole diagram): hit in its `area`
/// (none: on one of its buttons), a press taken; over its buttons the hand when `pointer`,
/// and the tooltip `tip` names.
pub(super) struct Zone<'a, T: Copy> {
    pub(super) area: Option<Rect>,
    pub(super) buttons: &'a [(Rect, T)],
    pub(super) pointer: bool,
    pub(super) tip: Option<fn(T) -> &'static str>,
}

impl<T: Copy> Zone<'_, T> {
    fn button_at(&self, p: Point) -> Option<(Rect, T)> {
        self.buttons.iter().find(|(r, _)| r.contains(p)).copied()
    }
}

impl<T: Copy> Control for Zone<'_, T> {
    fn hit(&self, p: Point) -> bool {
        match self.area {
            Some(area) => area.contains(p),
            None => self.button_at(p).is_some(),
        }
    }

    fn press(&mut self, _p: Point, _clicks: u32) -> Press {
        Press::Took
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        (self.pointer && self.button_at(p).is_some()).then_some(ViewerCursor::Pointer)
    }

    fn tooltip(&self, p: Point) -> Option<Tip> {
        let (r, button) = self.button_at(p)?;
        Some(Tip::new(r, t((self.tip?)(button))))
    }
}

impl DiagramView {
    /// Its parts, with their state of use (the focus on the first with a field open), and
    /// what they asked done once they're let go (the focus then set again: what they did
    /// may have closed a field).
    pub(super) fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let area = self.show_palette.then(|| self.palette_area());
        let room = self.palette_room();
        let (bar, panel) = (self.toolbar_area(), self.properties_area());
        let mut asked = [None, None, None];
        let answer = {
            let [a_typing, a_palette, a_text] = &mut asked;
            let typing = self.props.typing.as_mut().map(|(_, input)| input);
            let mut typing = TypingPart { typing, asked: a_typing };
            let mut palette =
                PalettePart { palette: &mut self.palette, area, room, asked: a_palette };
            let mut text = TextPart { editing: self.editing.as_mut(), asked: a_text };
            let mut bar = Zone {
                area: Some(bar),
                buttons: &self.tools,
                pointer: false,
                tip: Some(super::toolbar::Tool::tip),
            };
            let none: &[(Rect, ())] = &[];
            let mut panel = Zone { area: Some(panel), buttons: none, pointer: false, tip: None };
            let (quick_area, quick_buttons) = match &self.quick_bar {
                Some((area, buttons)) => (Some(*area), buttons.as_slice()),
                None => (None, &[][..]),
            };
            let mut quick = Zone {
                // Not shown: no buttons either, so nowhere.
                area: quick_area,
                buttons: quick_buttons,
                pointer: true,
                tip: Some(super::quick::Quick::tip),
            };
            // Hidden: no area and no buttons, so nowhere.
            let mut palette_buttons = Zone { area, buttons: none, pointer: false, tip: None };
            let mut tabs = Zone { area: None, buttons: &self.tabs, pointer: true, tip: None };
            let mut parts: [&mut dyn Control; 8] = [
                &mut typing,
                &mut palette,
                &mut text,
                &mut bar,
                &mut panel,
                &mut quick,
                &mut palette_buttons,
                &mut tabs,
            ];
            let first = parts.iter().position(|c| c.focusable());
            if self.controls.focused() != first {
                self.controls.focus(&mut parts, first);
            }
            f(&mut self.controls, &mut parts)
        };
        let any = asked.iter().any(Option::is_some);
        for asked in asked.into_iter().flatten() {
            match asked {
                Asked::EndText => {
                    self.end_text();
                }
                Asked::EndTyping(set) => {
                    self.end_typing(set);
                }
                Asked::AddFound => {
                    self.add_found();
                }
            }
        }
        if any {
            self.sync_focus();
        }
        answer
    }

    /// The focus set again on the first part with a field open (a field opened or closed by
    /// what was just done, so what's asked from `&self` reads the right one).
    pub(super) fn sync_focus(&mut self) {
        self.with_controls(|_, _| ());
    }
}
