//! The hints the modules contribute (`workbench_model::hints`), shown one at a time when a
//! module asks (`host.show_hint`): a card by `ui_components::hint_card` next to its anchor
//! (found among the rects reported while this frame was drawn, `ui_components::anchors`),
//! as an overlay that floats — no key of the editor's taken, nothing blocked, a click
//! elsewhere leaves it be. A click on it gives it the keyboard (← → / Tab move between its
//! buttons, ↵ presses, Esc closes); a press elsewhere takes the keyboard back.
//!
//! Which may show is the app's for every window ([`BOOK`]): those seen for good come from the
//! settings (`[hints] seen`) and are saved there; those closed are kept for the run.

use std::sync::Mutex;

use workbench_model::{HintBook, HintButton, HintOutcome, HintTour};

use super::overlay_stack::{OverlayKind, OverlaySlot};
use super::*;

/// Which hints may show, for every window: read from the settings the first time.
static BOOK: Mutex<Option<HintBook>> = Mutex::new(None);

/// The book as the settings have it (nothing closed yet).
fn read_book() -> HintBook {
    HintBook {
        seen: crate::config::settings().get(&crate::settings::HINTS_SEEN),
        closed: Vec::new(),
    }
}

/// The book, read from the settings if it wasn't yet, given to `f` (in the tests, each
/// thread's own, as their settings are).
fn with_book<R>(f: impl FnOnce(&mut HintBook) -> R) -> R {
    #[cfg(test)]
    {
        thread_local! {
            static TEST: std::cell::RefCell<Option<HintBook>> = const { std::cell::RefCell::new(None) };
        }
        let _ = &BOOK;
        TEST.with(|book| f(book.borrow_mut().get_or_insert_with(read_book)))
    }
    #[cfg(not(test))]
    {
        let mut book = BOOK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(book.get_or_insert_with(read_book))
    }
}

impl Workbench {
    /// Shows hint `id` (a module's) unless one shows already, it's unknown, seen for good or
    /// closed during this run. `true` if it shows.
    pub(crate) fn show_hint(&mut self, id: &str) -> bool {
        self.show_hint_by(id, None)
    }

    /// [`Self::show_hint`], pointing at `anchor` instead of its steps' anchors when given.
    pub(crate) fn show_hint_by(
        &mut self,
        id: &str,
        anchor: Option<workbench_model::HintAnchor>,
    ) -> bool {
        // One whose tab was closed is let go first: it never holds the place of a new one.
        self.let_go_of_orphan_hint();
        if self.hint.is_some() {
            return false;
        }
        let Some(hint) = self.support.hints.iter().find(|h| h.hint.id == id) else {
            return false;
        };
        if hint.hint.steps.is_empty() || !with_book(|book| book.may_show(id)) {
            return false;
        }
        // The tab it's about, found now by what opened it (its identity kept from here):
        // not open here, the hint isn't shown.
        let tab = match &anchor {
            Some(workbench_model::HintAnchor::Tab(view)) => match self.view_tab_id(view) {
                Some(tab) => Some(tab),
                None => return false,
            },
            None => None,
        };
        tracing::debug!(hint = id, "a hint shown");
        let mut tour = HintTour::new(hint.hint.clone());
        // The tab it's about tinted in the accent while it shows (fading in, a pulse or two).
        if let Some(tab) = tab {
            self.tint_tab_id(tab, Some(workbench_model::TabTone::Accent));
        }
        tour.anchor = anchor;
        self.hint = Some(tour);
        self.hint_tab = tab;
        self.hint_focused = false;
        true
    }

    /// The hint shown given the keyboard, its main button focused (the snapshots).
    #[cfg(feature = "snapshot")]
    pub fn focus_hint_for_snapshot(&mut self) {
        if let Some(tour) = self.hint.as_mut() {
            self.hint_focused = true;
            tour.move_focus(0);
        }
    }

    /// The hint shown now, by its id (the tests).
    #[cfg(test)]
    pub(super) fn hint_shown(&self) -> Option<&'static str> {
        self.hint.as_ref().map(|h| h.hint.id)
    }

    /// `button` of the hint pressed: it moves to another step, or closes (remembered as the
    /// book says), its command run then.
    fn press_hint(&mut self, button: HintButton) -> MouseResult {
        let Some(tour) = self.hint.as_mut() else { return MouseResult::None };
        match tour.press(button) {
            HintOutcome::Stay => MouseResult::Redraw,
            HintOutcome::Close { command, seen } => {
                self.close_hint(seen);
                match command {
                    Some(command) => MouseResult::Command(command.0),
                    None => MouseResult::Redraw,
                }
            }
        }
    }

    /// The hint shown about a tab that's no longer open (closed, its project gone): let go
    /// whole — its card hidden, its tab's tint let go — and never counted as closed by the
    /// user (the next session that asks may show it). Cheap: a look at the open tabs.
    pub(crate) fn let_go_of_orphan_hint(&mut self) {
        let Some(tab) = self.hint_tab else { return };
        if self.tab_open(tab) {
            return;
        }
        // Its tab gone but its view open still (a copy dropped where it was open already):
        // the hint and its tint follow the one kept.
        let view = self
            .hint
            .as_ref()
            .and_then(|h| h.anchor.as_ref())
            .map(|workbench_model::HintAnchor::Tab(view)| view.clone());
        if let Some(kept) = view.and_then(|view| self.view_tab_id(&view)) {
            self.tint_tab_id(tab, None);
            self.tint_tab_id(kept, Some(workbench_model::TabTone::Accent));
            self.hint_tab = Some(kept);
            return;
        }
        self.hint = None;
        self.hint_tab = None;
        self.hint_focused = false;
        self.hint_card.hide();
        self.tint_tab_id(tab, None);
        tracing::debug!("a hint let go: its tab closed");
    }

    /// The hint closed: for this run, or (`seen`) for good, saved in the settings.
    pub(crate) fn close_hint(&mut self, seen: bool) {
        let Some(tour) = self.hint.take() else { return };
        self.hint_focused = false;
        self.hint_card.hide();
        if let Some(tab) = self.hint_tab.take() {
            self.tint_tab_id(tab, None);
        }
        let id = tour.hint.id;
        let saved = with_book(|book| book.close(id, seen).then(|| book.seen.clone()));
        if let Some(seen) = saved
            && let Err(e) = crate::config::settings().set(&crate::settings::HINTS_SEEN, seen)
        {
            tracing::warn!(error = %e, "a hint seen wasn't saved");
        }
    }

    /// The area a hint stays in: the window under its top bar, above its status bar.
    fn hint_area(&self) -> Rect {
        let s = self.scale;
        let top = self.card.min_y();
        let bottom = self.size.height as f32 - StatusBar::height(s);
        Rect::new(0.0, top, self.size.width as f32, (bottom - top).max(0.0))
    }

    /// Draws the hint shown (nothing while none is).
    fn paint_hint(&mut self, text: &mut TextSystem, scene: &mut Scene) {
        self.let_go_of_orphan_hint();
        // Under a modal: not drawn (it comes back once that closes).
        if self.hint.is_some() && open_above(self, true) {
            self.hint_card.hide();
            return;
        }
        let now = std::time::Instant::now();
        let Some(tour) = &self.hint else { return };
        let Some(step) = tour.current() else { return };
        let title = i18n::t(step.title);
        let body = i18n::t(step.body);
        let labels: Vec<(String, bool, bool)> = tour
            .buttons()
            .into_iter()
            .map(|b| {
                let label = match b {
                    HintButton::Dismiss => i18n::t("hints.dismiss"),
                    HintButton::Back => i18n::t("hints.back"),
                    HintButton::Next => i18n::t("hints.next"),
                    HintButton::Done => i18n::t("hints.done"),
                    HintButton::Action(i) => {
                        step.actions.get(i).map_or_else(String::new, |a| i18n::t(a.label))
                    }
                };
                (label, tour.is_main(b), b == HintButton::Dismiss)
            })
            .collect();
        let buttons: Vec<ui_workbench::hint_card::CardButton<'_>> = labels
            .iter()
            .map(|(label, main, aside)| ui_workbench::hint_card::CardButton {
                label,
                main: *main,
                aside: *aside,
            })
            .collect();
        let eyebrow = i18n::t(step.eyebrow.unwrap_or("hints.tip"));
        let place = step.place.map(i18n::t);
        let spec = ui_workbench::hint_card::HintCardSpec {
            icon: step.icon,
            eyebrow: &eyebrow,
            title: &title,
            body: &body,
            place: place.as_deref(),
            counter: tour.counter(),
            buttons: &buttons,
            focus: if self.hint_focused { tour.focus } else { None },
        };
        // Its own tab (wherever it is, active or not), else its step's anchor.
        let anchor = match self.hint_tab {
            Some(tab) => {
                let found =
                    ui_workbench::anchors::find_identified(workbench_model::hints::ANCHOR_TAB, tab);
                // Its tab out of sight (scrolled away in its bar): the card waits, hidden,
                // and comes back with it — never pointing at nothing.
                if found.is_none() {
                    self.hint_card.hide();
                    // Unseen, it doesn't keep the keys (they go back to what's in sight).
                    self.hint_focused = false;
                    return;
                }
                found
            }
            None => step.anchor.and_then(ui_workbench::anchors::find),
        };
        // A tab it's about is tinted instead (`tint_tab`): no ring drawn over it.
        let tinted = self.hint_tab.is_some_and(|tab| self.tab_tints.of(tab, now).is_some());
        let place = ui_workbench::hint_card::HintPlace {
            anchor,
            area: self.hint_area(),
            scale: self.scale,
            spotlight: !tinted,
        };
        let painter =
            ui_workbench::Painter { text, scene, palette: &self.palette, icons: &self.icons };
        self.hint_card.paint(painter, &spec, place);
    }
}

/// Whether an overlay above the hint in the stack is open (`modal`: a modal only): a menu,
/// a list, a dialog, a modal window. Never asks the hint itself (its own `is_open` asks this).
fn open_above(wb: &Workbench, modal: bool) -> bool {
    Overlay::ALL.into_iter().take_while(|o| *o != Overlay::Hint).any(|o| {
        let slot = o.slot();
        !slot.floats(wb) && (!modal || slot.kind(wb) == OverlayKind::Modal) && wb.overlay_is_open(o)
    })
}

/// The hint as an overlay: floating while it doesn't have the keyboard (see the module).
pub(super) struct HintSlot;

impl OverlaySlot for HintSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    /// Shown unless a modal is open over it (it comes back once that closes).
    fn is_open(&self, wb: &Workbench) -> bool {
        wb.hint.is_some() && !open_above(wb, true)
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    /// Floating until clicked: then it has the keyboard.
    fn floats(&self, wb: &Workbench) -> bool {
        !wb.hint_focused
    }

    /// Its card, unless a menu or a window over it is open (that one takes what falls on it).
    fn holds(&self, wb: &Workbench, p: Point) -> bool {
        wb.hint_card.contains(p) && !open_above(wb, false)
    }

    fn area(&self, wb: &Workbench) -> Option<kernel::Rect> {
        wb.hint_card.bounds()
    }

    /// A press elsewhere: the keyboard goes back where it was (the hint stays).
    fn press_elsewhere(&self, wb: &mut Workbench) {
        wb.hint_focused = false;
    }

    /// × closes it for now; a button does what it says; elsewhere on it, it takes the
    /// keyboard.
    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        use ui_workbench::hint_card::CardHit;
        match wb.hint_card.hit(p) {
            Some(CardHit::Close) => {
                wb.close_hint(false);
                MouseResult::Redraw
            }
            Some(CardHit::Button(i)) => {
                let button = wb.hint.as_ref().and_then(|t| t.buttons().get(i).copied());
                match button {
                    Some(button) => wb.press_hint(button),
                    None => MouseResult::None,
                }
            }
            Some(CardHit::Card) => {
                wb.hint_focused = true;
                if let Some(tour) = wb.hint.as_mut()
                    && tour.focus.is_none()
                {
                    tour.move_focus(0);
                }
                MouseResult::Redraw
            }
            None => MouseResult::None,
        }
    }

    /// With the keyboard: ← → / Tab move between its buttons, ↵ presses one, Esc closes it
    /// for now.
    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        let Some(tour) = wb.hint.as_mut() else { return false };
        match key {
            EditKey::Escape => wb.close_hint(false),
            EditKey::Enter { .. } => {
                let button = tour.enter();
                let result = wb.press_hint(button);
                if let MouseResult::Command(_) = result {
                    wb.request = Some(result);
                }
            }
            EditKey::Tab { shift: true }
            | EditKey::Move { motion: Motion::Left | Motion::Up, .. } => tour.move_focus(-1),
            EditKey::Tab { shift: false }
            | EditKey::Move { motion: Motion::Right | Motion::Down, .. } => tour.move_focus(1),
            _ => {}
        }
        true
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        let changed = wb.hint_card.hover(Some(p));
        let icon =
            if wb.hint_card.over_button() { CursorIcon::Pointer } else { CursorIcon::Default };
        (changed, icon)
    }

    /// Nothing scrolls under it while the mouse is on it; with the keyboard, the wheel goes
    /// on to what's under the mouse.
    fn scroll(&self, wb: &mut Workbench, _dx: f32, _dy: f32) -> Option<bool> {
        (!wb.hint_focused).then_some(false)
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.paint_hint(text, scene);
    }

    fn animating(&self, _wb: &Workbench) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/hints.rs"]
mod tests;
