//! The search's panel as its view's one control: over the whole window (a press outside it
//! still closes the folders suggested), its scrollbar's thumb held while dragged (the
//! folders suggested's too: their list, the shared one, comes first wherever it lies over
//! the panel), and with
//! the focus its keys and what's typed. Its fields keep their own order (Tab opens the
//! folder's row the first time, goes on through the rows shown) and its keys their own
//! meaning (↑ ↓ through the results, ↵ by the field, esc the suggestions first), so it
//! keeps Tab; what needs the view (an action, a search again, the folders suggested) is
//! written to `ask`.

use kernel::Point;
use ui_components::control::{Control, NOWHERE, Press};
use ui_components::tool_panel::KeyReach;
use ui_components::{Tip, ViewerCursor, ViewerKey};

use crate::search_panel::{ProjectSearch, ProjectSearchAction};

/// The wheel's points that move the results by one row (at a scale of 1).
const WHEEL_ROW: f32 = 20.0;

/// The rows the page keys move the selection by.
const PAGE_ROWS: isize = 8;

/// What the panel asks of its view after a press, a key or text typed.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Ask {
    /// An action of the panel's (a row, a button, a key's).
    Act(ProjectSearchAction),
    /// A field pressed at a point: its action, then the caret put there.
    Field(ProjectSearchAction, Point),
    /// A field's text changed: searched again (or the folders suggested).
    Typed,
    /// The keyboard went on to the next field: the folders it suggests.
    Suggest,
}

/// The panel and what it asks, with the scale its wheel is read at.
pub(super) struct PanelPart<'a> {
    pub panel: &'a mut ProjectSearch,
    pub scale: f32,
    pub ask: &'a mut Option<Ask>,
}

impl std::fmt::Debug for PanelPart<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelPart").field("ask", &self.ask).finish_non_exhaustive()
    }
}

impl PanelPart<'_> {
    /// The keys of the panel (as the IDEs' search: esc closes, ↵ opens what's lit).
    fn panel_key(&mut self, key: ViewerKey, reach: KeyReach) {
        let panel = &mut *self.panel;
        let suggesting = panel.has_suggestions();
        match key {
            // Esc closes the folders suggested first; then it all, a search typed or not.
            ViewerKey::Escape if suggesting => panel.clear_suggestions(),
            ViewerKey::Escape => *self.ask = Some(Ask::Act(ProjectSearchAction::Close)),
            // In the replacement's field: back to the search (Replace All is its button).
            ViewerKey::Enter if panel.in_replace() => panel.focus_replace(false),
            // In the folder's field: the folder lit is taken (to go on into it); with none
            // suggested, back to the search.
            ViewerKey::Enter if panel.in_scope() => {
                if panel.take_suggestion(None) {
                    *self.ask = Some(Ask::Typed);
                } else {
                    panel.focus_scope(false);
                }
            }
            ViewerKey::Enter => *self.ask = panel.selected_action().map(Ask::Act),
            // To the next field shown (the folder's row the first time).
            ViewerKey::Tab => {
                if panel.scope_open() || panel.replace_open() {
                    panel.next_field();
                    *self.ask = Some(Ask::Suggest);
                } else {
                    *self.ask = Some(Ask::Act(ProjectSearchAction::ToggleScope));
                }
            }
            ViewerKey::Up if suggesting => panel.move_suggestion(-1),
            ViewerKey::Down if suggesting => panel.move_suggestion(1),
            ViewerKey::Up => panel.move_selection(-1),
            ViewerKey::Down => panel.move_selection(1),
            ViewerKey::PageUp => panel.move_selection(-PAGE_ROWS),
            ViewerKey::PageDown => panel.move_selection(PAGE_ROWS),
            ViewerKey::Left => panel.typing().move_left(reach.word, reach.extend),
            ViewerKey::Right => panel.typing().move_right(reach.word, reach.extend),
            ViewerKey::Home => panel.typing().home(reach.extend),
            ViewerKey::End => panel.typing().end(reach.extend),
            // On a recent search (the search's field empty, the keyboard in it): ⌫ / Delete
            // forget it.
            ViewerKey::Backspace | ViewerKey::Delete
                if panel.in_query() && panel.showing_recent() && panel.input.text().is_empty() =>
            {
                if let Some(ProjectSearchAction::Recent(i)) = panel.selected_action() {
                    *self.ask = Some(Ask::Act(ProjectSearchAction::ForgetRecent(i)));
                }
            }
            ViewerKey::Backspace => {
                panel.typing().backspace(reach.word);
                *self.ask = Some(Ask::Typed);
            }
            ViewerKey::Delete => {
                panel.typing().delete();
                *self.ask = Some(Ask::Typed);
            }
        }
    }
}

impl Control for PanelPart<'_> {
    /// Over the whole window: a press anywhere is its (outside the panel it does nothing
    /// but close the folders suggested).
    fn hit(&self, _p: Point) -> bool {
        true
    }

    /// The folders suggested first (a row taken, their thumb held), then its scrollbar's
    /// thumb (held), else what's there: a field's caret, a row, a button. Anywhere but on
    /// the folders suggested or the folder's field, their list closes.
    fn press(&mut self, p: Point, clicks: u32) -> Press {
        if self.panel.on_suggestions(p) {
            let (press, chosen) = self.panel.press_suggestions(p, clicks);
            *self.ask = chosen.map(|i| Ask::Act(ProjectSearchAction::Suggestion(i)));
            return press;
        }
        if self.panel.press_bar(p) {
            return Press::Held;
        }
        let hit = self.panel.hit(p);
        let keeps_list = matches!(
            hit,
            Some(ProjectSearchAction::Suggestion(_) | ProjectSearchAction::FocusScope)
        );
        if !keeps_list {
            self.panel.clear_suggestions();
        }
        *self.ask = hit.map(|action| match action {
            ProjectSearchAction::FocusQuery
            | ProjectSearchAction::FocusScope
            | ProjectSearchAction::FocusMask
            | ProjectSearchAction::FocusReplace => Ask::Field(action, p),
            action => Ask::Act(action),
        });
        Press::Took
    }

    fn drag(&mut self, p: Point) -> bool {
        self.panel.drag_suggestions(p) || self.panel.drag_bar(p)
    }

    fn release(&mut self) -> bool {
        self.panel.release_suggestions() | self.panel.release_bar()
    }

    /// The folders suggested lit under the mouse; what's under their list isn't hovered.
    fn hover(&mut self, p: Option<Point>) -> bool {
        let p = p.unwrap_or(NOWHERE);
        let list = self.panel.hover_suggestions(p);
        let under = if self.panel.on_suggestions(p) { NOWHERE } else { p };
        let bar = self.panel.hover_bar(under);
        self.panel.hover(under) | bar | list
    }

    /// A hand over what's clicked (a row, a button, the `esc` key, the search's ×); the
    /// text's over the search.
    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        if self.panel.on_suggestions(p) {
            return self.panel.suggestions_cursor(p);
        }
        if self.panel.hit(p) == Some(ProjectSearchAction::FocusQuery) {
            return Control::cursor(&self.panel.input, p);
        }
        let clickable = matches!(
            self.panel.hit(p),
            Some(
                ProjectSearchAction::Open(_)
                    | ProjectSearchAction::OpenFolder(_)
                    | ProjectSearchAction::Recent(_)
                    | ProjectSearchAction::ForgetRecent(_)
                    | ProjectSearchAction::ClearRecent
                    | ProjectSearchAction::EnableSemantic
                    | ProjectSearchAction::Toggle(_)
                    | ProjectSearchAction::ToggleReplace
                    | ProjectSearchAction::ReplaceAll
            )
        ) || self.panel.over_esc(p);
        clickable.then_some(ViewerCursor::Pointer)
    }

    /// Over the folders suggested, their rows; else by rows (as the editor's lines).
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if let Some(moved) = self.panel.scroll_suggestions(dy) {
            return moved;
        }
        let line = (WHEEL_ROW * self.scale.max(1.0)).max(1.0);
        let rows = (dy / line).round() as isize;
        if rows != 0 {
            self.panel.scroll(rows);
        }
        rows != 0
    }

    fn focusable(&self) -> bool {
        true
    }

    /// Tab goes through its own fields.
    fn keeps_tab(&self) -> bool {
        true
    }

    /// Every key is its (one it has no use for does nothing).
    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.panel_key(key, reach);
        true
    }

    /// Into the field with the keyboard, on one line (the search field's own way: its
    /// breaks as spaces).
    fn text(&mut self, typed: &str) -> bool {
        Control::text(self.panel.typing(), typed);
        *self.ask = Some(Ask::Typed);
        true
    }

    /// The search's own selection (what's copied is what's searched).
    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&self.panel.input)
    }

    /// ⌘A in the field with the keyboard.
    fn command(&mut self, id: &str) -> bool {
        if id != "select_all" {
            return false;
        }
        self.panel.typing().select_all();
        true
    }

    /// A name cut short: the whole of it (its path); the folder's button, the replacement's,
    /// an option's: what it does.
    fn tooltip(&self, p: Point) -> Option<Tip> {
        if self.panel.on_suggestions(p) {
            return self.panel.suggestion_tooltip(p);
        }
        let button = self.panel.scope_button();
        if button.contains(p) {
            return Some(Tip::new(button, i18n::t("search.folder")));
        }
        let replace = self.panel.replace_button();
        if replace.contains(p) {
            return Some(Tip::new(replace, i18n::t("search.replace")));
        }
        if let Some((rect, option)) = self.panel.option_at(p) {
            return Some(Tip::new(rect, i18n::t(option.tooltip_key())));
        }
        let (rect, path) = self.panel.tip_at(p)?;
        Some(Tip::new(rect, path))
    }

    /// Its appearance (a fade, a slight slide).
    fn animating(&self) -> bool {
        self.panel.is_animating()
    }

    /// When the folders suggested's scrollbar next fades.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.panel.suggestions_deadline()
    }
}
